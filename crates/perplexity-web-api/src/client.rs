use crate::auth::{AuthCookies, CSRF_TOKEN_COOKIE_NAME};
use crate::config::{
    API_BASE_URL, API_MODE_CONCISE, API_MODE_COPILOT, API_VERSION, ENDPOINT_AUTH_CSRF,
    ENDPOINT_AUTH_SESSION, ENDPOINT_RATE_LIMITS, ENDPOINT_SSE_ASK,
};
use crate::error::{Error, Result};
use crate::rate_limit::RateLimits;
use crate::sse::SseStream;
use crate::types::{
    AskParams, AskPayload, FollowUpContext, SearchEvent, SearchMode, SearchRequest,
    SearchResponse, UploadFile,
};
use crate::upload::upload_files;
use futures_util::{Stream, StreamExt};
use rquest::{Client as HttpClient, cookie::Jar};
use rquest_util::Emulation;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Default request timeout (30 seconds).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Builder for creating a configured [`Client`] instance.
pub struct ClientBuilder {
    cookies: Option<AuthCookies>,
    http_client: Option<HttpClient>,
    timeout: Duration,
}

impl ClientBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self { cookies: None, http_client: None, timeout: DEFAULT_TIMEOUT }
    }

    /// Sets authentication cookies for the client.
    ///
    /// Required for enhanced features like file uploads and pro/reasoning modes.
    pub fn cookies(mut self, cookies: AuthCookies) -> Self {
        self.cookies = Some(cookies);
        self
    }

    /// Sets a custom HTTP client.
    ///
    /// Use this to provide a pre-configured rquest client with custom settings.
    pub fn http_client(mut self, client: HttpClient) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Sets the request timeout.
    ///
    /// Default is 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Builds the client and performs initial session warm-up.
    ///
    /// When authentication cookies are provided, the CSRF token is fetched
    /// dynamically from `/api/auth/csrf` before the session warm-up.
    pub async fn build(self) -> Result<Client> {
        let Self { cookies, http_client, timeout } = self;
        let has_cookies = cookies.is_some();

        let http = match http_client {
            Some(client) => {
                if cookies.is_some() {
                    return Err(Error::CustomClientWithCookies);
                }
                client
            }
            None => {
                let jar = Arc::new(Jar::default());
                let url = API_BASE_URL.parse().map_err(|_| Error::InvalidBaseUrl)?;

                if let Some(auth) = &cookies {
                    let (name, value) = auth.session_cookie_pair();
                    let cookie = format!("{name}={value}; Domain=www.perplexity.ai; Path=/");
                    jar.add_cookie_str(&cookie, &url);
                }

                let http = HttpClient::builder()
                    .emulation(Emulation::Chrome136)
                    .cookie_provider(jar.clone())
                    .build()
                    .map_err(Error::HttpClientInit)?;

                if cookies.is_some() {
                    let csrf_token = Self::fetch_csrf_token(&http, timeout).await?;
                    let cookie = format!(
                        "{}={}; Domain=www.perplexity.ai; Path=/",
                        CSRF_TOKEN_COOKIE_NAME, csrf_token
                    );
                    jar.add_cookie_str(&cookie, &url);
                }

                http
            }
        };

        let session_fut =
            http.get(format!("{}{}", API_BASE_URL, ENDPOINT_AUTH_SESSION)).send();
        tokio::time::timeout(timeout, session_fut)
            .await
            .map_err(|_| Error::Timeout(timeout))?
            .map_err(Error::SessionWarmup)?;

        Ok(Client { http, has_cookies, timeout })
    }

    /// Fetches the CSRF token from `/api/auth/csrf`.
    async fn fetch_csrf_token(http: &HttpClient, timeout: Duration) -> Result<String> {
        let fut = http.get(format!("{}{}", API_BASE_URL, ENDPOINT_AUTH_CSRF)).send();
        let response = tokio::time::timeout(timeout, fut)
            .await
            .map_err(|_| Error::Timeout(timeout))?
            .map_err(Error::CsrfFetch)?;

        let body: serde_json::Value = response.json().await.map_err(Error::CsrfFetch)?;

        body.get("csrfToken")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned())
            .ok_or(Error::CsrfTokenMissing)
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async client for interacting with the Perplexity AI Web API.
///
/// Create a client using [`Client::builder()`]:
///
/// ```no_run
/// # async fn example() -> perplexity_web_api::Result<()> {
/// let client = perplexity_web_api::Client::builder()
///     .build()
///     .await?;
///
/// let response = client.search(
///     perplexity_web_api::SearchRequest::new("What is Rust?")
/// ).await?;
///
/// if let Some(answer) = response.answer {
///     println!("{}", answer);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    http: HttpClient,
    has_cookies: bool,
    timeout: Duration,
}

impl Client {
    /// Creates a new [`ClientBuilder`] for configuring the client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Performs a search query and returns the final response.
    ///
    /// This method consumes the entire SSE stream and returns the final result.
    /// For streaming responses, use [`search_stream`](Self::search_stream) instead.
    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse> {
        let mut stream = Box::pin(self.search_stream(request).await?);
        let mut last_event: Option<SearchEvent> = None;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => last_event = Some(event),
                Err(e) => return Err(e),
            }
        }

        let SearchEvent { answer, web_results, backend_uuid, attachments, raw } =
            last_event.ok_or(Error::UnexpectedEndOfStream)?;
        let follow_up = FollowUpContext { backend_uuid, attachments };

        Ok(SearchResponse { answer, web_results, follow_up, raw })
    }

    /// Performs a search query and returns a stream of events.
    ///
    /// Each event contains partial or complete response data as it arrives.
    /// The stream ends when the server sends `event: end_of_stream`.
    pub async fn search_stream(
        &self,
        request: SearchRequest,
    ) -> Result<impl Stream<Item = Result<SearchEvent>>> {
        self.validate_request(&request)?;

        // Pre-flight quota check: Perplexity's SSE endpoint silently returns an
        // empty answer once a plan quota is exhausted, so proactively surface a
        // clear "out of limit" error for authenticated, metered modes.
        self.check_quota_for_mode(request.mode).await?;

        let file_refs: Vec<&UploadFile> = request.files.iter().collect();
        let mut attachments = upload_files(&self.http, &file_refs, self.timeout).await?;

        if let Some(ref follow_up) = request.follow_up {
            attachments.extend(follow_up.attachments.clone());
        }

        let mode_str = match request.mode {
            SearchMode::Auto => API_MODE_CONCISE,
            SearchMode::Pro
            | SearchMode::Reasoning
            | SearchMode::DeepResearch
            | SearchMode::Computer
            | SearchMode::Study
            | SearchMode::DocumentReview => API_MODE_COPILOT,
        };

        let model_pref = request
            .model_preference
            .map(|preference| preference.as_str())
            .unwrap_or_else(|| request.mode.default_preference());

        let sources_str: Vec<String> =
            request.sources.iter().map(|s| s.as_str().to_owned()).collect();

        let payload = AskPayload {
            query_str: &request.query,
            params: AskParams {
                attachments,
                frontend_context_uuid: Uuid::new_v4().to_string(),
                frontend_uuid: Uuid::new_v4().to_string(),
                is_incognito: request.incognito,
                language: &request.language,
                last_backend_uuid: request.follow_up.and_then(|f| f.backend_uuid),
                mode: mode_str,
                model_preference: model_pref,
                source: "default",
                sources: sources_str,
                query_source: request.mode.query_source(),
                version: API_VERSION,
            },
        };

        let request_fut = self
            .http
            .post(format!("{}{}", API_BASE_URL, ENDPOINT_SSE_ASK))
            .json(&payload)
            .send();

        let response = tokio::time::timeout(self.timeout, request_fut)
            .await
            .map_err(|_| Error::Timeout(self.timeout))?
            .map_err(Error::SearchRequest)?
            .error_for_status()
            .map_err(|e| Error::Server {
                status: e.status().map(|s| s.as_u16()).unwrap_or(0),
                message: e.to_string(),
            })?;

        Ok(SseStream::new(response.bytes_stream()))
    }

    /// Uploads multiple files in a single batch and returns their S3 object URLs.
    ///
    /// All files are registered with the backend in one request, then uploaded
    /// to S3 in parallel, and finally processed server-side.
    /// Requires authentication cookies.
    pub async fn upload_files(&self, files: &[&UploadFile]) -> Result<Vec<String>> {
        if !files.is_empty() && !self.has_cookies {
            return Err(Error::FileUploadRequiresAuth);
        }
        upload_files(&self.http, files, self.timeout).await
    }

    /// Fetches the current rate-limit / usage-quota status.
    ///
    /// Reads Perplexity's internal `/rest/rate-limit/all` endpoint (the same one
    /// the web UI uses to render usage counters). Requires authentication
    /// cookies; returns [`Error::RateLimitRequiresAuth`] otherwise.
    pub async fn rate_limits(&self) -> Result<RateLimits> {
        if !self.has_cookies {
            return Err(Error::RateLimitRequiresAuth);
        }

        let fut = self.http.get(format!("{}{}", API_BASE_URL, ENDPOINT_RATE_LIMITS)).send();
        let response = tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| Error::Timeout(self.timeout))?
            .map_err(Error::RateLimitFetch)?
            .error_for_status()
            .map_err(Error::RateLimitFetch)?;

        let limits: RateLimits = response.json().await.map_err(Error::RateLimitFetch)?;
        Ok(limits)
    }

    /// Verifies that the plan quota for `mode` is not exhausted.
    ///
    /// For authenticated, metered modes this fetches the current quota and
    /// returns [`Error::RateLimited`] when no queries remain, converting
    /// Perplexity's silent empty-answer behaviour into an actionable error.
    ///
    /// No-ops (returns `Ok`) for tokenless clients, for the free
    /// [`SearchMode::Auto`] path, or if the quota endpoint is unreachable — a
    /// transient failure to read quotas should not block an otherwise valid
    /// request.
    pub async fn check_quota_for_mode(&self, mode: SearchMode) -> Result<()> {
        if !self.has_cookies {
            return Ok(());
        }
        // Free/unmetered mode draws no plan quota.
        if RateLimits::default().quota_for_mode(mode).is_none() {
            return Ok(());
        }

        match self.rate_limits().await {
            Ok(limits) => {
                if let Some(status) = limits.quota_for_mode(mode)
                    && status.is_exhausted()
                {
                    return Err(Error::RateLimited {
                        feature: status.feature,
                        remaining: status.remaining,
                    });
                }
                Ok(())
            }
            // Don't fail an otherwise-valid request just because the quota probe
            // failed (network blip, endpoint change) — proceed with the request.
            Err(_) => Ok(()),
        }
    }

    fn validate_request(&self, request: &SearchRequest) -> Result<()> {
        if !request.files.is_empty() && !self.has_cookies {
            return Err(Error::FileUploadRequiresAuth);
        }

        let needs_auth = request.sources.iter().any(|s| !s.is_public());
        if needs_auth && !self.has_cookies {
            return Err(Error::ConnectorRequiresAuth);
        }

        Ok(())
    }
}
