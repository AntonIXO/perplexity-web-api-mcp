use crate::auth::{AuthCookies, CSRF_TOKEN_COOKIE_NAME};
use crate::config::{
    API_BASE_URL, API_MODE_CONCISE, API_MODE_COPILOT, API_REFERER, API_VERSION,
    ENDPOINT_AUTH_CSRF, ENDPOINT_AUTH_SESSION, ENDPOINT_RATE_LIMITS, ENDPOINT_SSE_ASK,
};
use crate::error::{Error, Result};
use crate::parse::extract_thread_uuid_or_slug;
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

/// Default timeout for long-running modes (Deep Research, Computer, Document
/// Review): 10 minutes. Deep Research can legitimately run for several minutes
/// before the stream completes, so the short default would abort it prematurely.
const DEFAULT_LONG_TIMEOUT: Duration = Duration::from_secs(600);

/// Prefix of the per-account session cookie Perplexity switched to.
const PPLX_SESSION_COOKIE_PREFIX: &str = "__Secure-pplx.session.";

/// Header naming the active account for the request.
///
/// Perplexity supports several signed-in accounts at once, one
/// `__Secure-pplx.session.<account-uuid>` cookie each. Without this header the
/// backend cannot tell which session to resolve and answers anonymously — a
/// silent downgrade, not an error.
const ACCOUNT_HEADER: &str = "x-pplx-account";

/// Extracts the account UUID from a per-account session cookie name.
fn account_id_from_cookie_name(name: &str) -> Option<&str> {
    name.strip_prefix(PPLX_SESSION_COOKIE_PREFIX).filter(|id| !id.is_empty())
}

/// Adds the active-account header to a request when one is configured.
pub(crate) fn with_account(
    request: rquest::RequestBuilder,
    account_id: Option<&str>,
) -> rquest::RequestBuilder {
    match account_id {
        Some(id) => request.header(ACCOUNT_HEADER, id),
        None => request,
    }
}

/// Builds a `Set-Cookie`-style string for the jar, honouring the cookie name
/// prefixes browsers enforce.
///
/// `__Secure-` requires the `Secure` attribute, `__Host-` additionally forbids
/// `Domain` and demands `Path=/`. Perplexity's current session cookie uses the
/// `__Secure-` prefix, so a plain `name=value; Domain=...; Path=/` string would
/// be rejected by a conforming cookie store and the request would go out
/// unauthenticated.
fn cookie_str(name: &str, value: &str) -> String {
    if name.starts_with("__Host-") {
        format!("{name}={value}; Path=/; Secure")
    } else {
        format!("{name}={value}; Domain=www.perplexity.ai; Path=/; Secure")
    }
}

/// Builder for creating a configured [`Client`] instance.
pub struct ClientBuilder {
    cookies: Option<AuthCookies>,
    http_client: Option<HttpClient>,
    timeout: Duration,
    long_timeout: Duration,
}

impl ClientBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self {
            cookies: None,
            http_client: None,
            timeout: DEFAULT_TIMEOUT,
            long_timeout: DEFAULT_LONG_TIMEOUT,
        }
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

    /// Sets the request timeout for standard (fast) modes.
    ///
    /// Default is 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the request timeout for long-running modes (Deep Research, Computer,
    /// Document Review).
    ///
    /// Default is 10 minutes. These modes can hold the connection open for
    /// minutes before the stream completes, so they use a separate, larger
    /// budget than [`timeout`](Self::timeout).
    pub fn long_timeout(mut self, long_timeout: Duration) -> Self {
        self.long_timeout = long_timeout;
        self
    }

    /// Builds the client and performs initial session warm-up.
    ///
    /// When authentication cookies are provided, the CSRF token is fetched
    /// dynamically from `/api/auth/csrf` before the session warm-up.
    pub async fn build(self) -> Result<Client> {
        let Self { cookies, http_client, timeout, long_timeout } = self;
        let has_cookies = cookies.is_some();

        // Name the account this session belongs to. Perplexity keeps one
        // `__Secure-pplx.session.<uuid>` cookie per signed-in account; without the
        // header the backend cannot pick one and answers anonymously instead of
        // failing, which reads as a valid but empty response.
        //
        // It has to travel per request: `default_headers` swaps out the whole
        // header map the emulation installed, and the resulting bare request is
        // rejected by the bot filter in front of the API.
        let account_id = if std::env::var("PERPLEXITY_NO_ACCOUNT_HEADER").is_ok() {
            None
        } else {
            std::env::var("PERPLEXITY_ACCOUNT_ID").ok().or_else(|| {
                cookies
                    .as_ref()
                    .and_then(|auth| account_id_from_cookie_name(auth.session_cookie_name()))
                    .map(|id| id.to_owned())
            })
        };
        if std::env::var("PERPLEXITY_DEBUG_COOKIES").is_ok() {
            eprintln!("[debug] account header: {account_id:?}");
        }

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
                    jar.add_cookie_str(&cookie_str(name, value), &url);
                    if std::env::var("PERPLEXITY_DEBUG_COOKIES").is_ok() {
                        use rquest::cookie::CookieStore;
                        let names: Vec<String> = jar
                            .cookies(&url)
                            .and_then(|h| h.to_str().ok().map(|s| s.to_owned()))
                            .unwrap_or_default()
                            .split(';')
                            .filter_map(|p| p.split('=').next().map(|n| n.trim().to_owned()))
                            .filter(|n| !n.is_empty())
                            .collect();
                        eprintln!("[debug] jar cookies for {url}: {names:?}");
                    }
                }

                let http = HttpClient::builder()
                    .emulation(Emulation::Chrome136)
                    .cookie_provider(jar.clone())
                    .build()
                    .map_err(Error::HttpClientInit)?;

                // `/api/auth/csrf` is a NextAuth leftover. Accounts on the newer
                // per-account cookie scheme no longer get a JSON body there, so
                // asking for one aborts startup for no gain.
                if cookies.is_some() && account_id.is_none() {
                    let csrf_token = Self::fetch_csrf_token(&http, timeout).await?;
                    jar.add_cookie_str(&cookie_str(CSRF_TOKEN_COOKIE_NAME, &csrf_token), &url);
                }

                http
            }
        };

        let session_fut = with_account(
            http.get(format!("{}{}", API_BASE_URL, ENDPOINT_AUTH_SESSION)),
            account_id.as_deref(),
        )
        .send();
        let warmup = tokio::time::timeout(timeout, session_fut)
            .await
            .map_err(|_| Error::Timeout(timeout))?
            .map_err(Error::SessionWarmup)?;

        if std::env::var("PERPLEXITY_DEBUG_COOKIES").is_ok() {
            let status = warmup.status();
            let set_cookie: Vec<String> = warmup
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .filter_map(|s| s.split('=').next().map(|n| n.to_owned()))
                .collect();
            eprintln!(
                "[debug] warmup {ENDPOINT_AUTH_SESSION} -> {status}, set-cookie: {set_cookie:?}"
            );
        }

        Ok(Client { http, has_cookies, timeout, long_timeout, account_id })
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
    long_timeout: Duration,
    account_id: Option<String>,
}

impl Client {
    /// Creates a new [`ClientBuilder`] for configuring the client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Returns a reference to the underlying HTTP client.
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Performs a search query and returns the complete response.
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
        let mut attachments =
            upload_files(&self.http, &file_refs, self.timeout, self.account_id.as_deref())
                .await?;

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

        let request_fut = with_account(
            self.http.post(format!("{}{}", API_BASE_URL, ENDPOINT_SSE_ASK)),
            self.account_id.as_deref(),
        )
        .json(&payload)
        .send();

        // Long-running modes (Deep Research, Computer, Document Review) can hold
        // the connection open for minutes; give them the larger timeout budget
        // so the request isn't aborted before the stream completes.
        let request_timeout =
            if request.mode.is_long_running() { self.long_timeout } else { self.timeout };

        let response = tokio::time::timeout(request_timeout, request_fut)
            .await
            .map_err(|_| Error::Timeout(request_timeout))?
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
        upload_files(&self.http, files, self.timeout, self.account_id.as_deref()).await
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

        let fut = with_account(
            self.http.get(format!("{}{}", API_BASE_URL, ENDPOINT_RATE_LIMITS)),
            self.account_id.as_deref(),
        )
        .send();
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

    /// Fetches existing thread / task details directly via API without generating a new search query.
    pub async fn read_thread(&self, uuid_or_slug: &str) -> Result<SearchResponse> {
        let clean_uuid = extract_thread_uuid_or_slug(uuid_or_slug);

        let endpoints = vec![
            format!(
                "{}/rest/threads/by_slug_or_id/{}?version={}&source=default",
                API_BASE_URL, clean_uuid, API_VERSION
            ),
            format!("{}/rest/threads/{}?version={}", API_BASE_URL, clean_uuid, API_VERSION),
            format!(
                "{}/rest/tasks/by_slug_or_id/{}?version={}",
                API_BASE_URL, clean_uuid, API_VERSION
            ),
        ];

        let mut last_err = None;

        for ep in endpoints {
            let fut = with_account(self.http.get(&ep), self.account_id.as_deref())
                .header("Referer", API_REFERER)
                .header("Accept", "application/json, text/plain, */*")
                .header("x-perplexity-version", API_VERSION)
                .send();

            match tokio::time::timeout(self.timeout, fut).await {
                Ok(Ok(response)) => {
                    if response.status().is_success() {
                        if let Ok(json_val) = response.json::<serde_json::Value>().await {
                            return crate::parse::parse_thread_response(&json_val);
                        }
                    } else {
                        last_err = Some(Error::Server {
                            status: response.status().as_u16(),
                            message: format!(
                                "HTTP status {} for thread read endpoint ({})",
                                response.status(),
                                ep
                            ),
                        });
                    }
                }
                Ok(Err(e)) => {
                    last_err = Some(Error::SearchRequest(e));
                }
                Err(_) => {
                    last_err = Some(Error::Timeout(self.timeout));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            Error::Custom("Failed to fetch thread content via API".to_string())
        }))
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

#[cfg(test)]
mod tests {
    use super::{account_id_from_cookie_name, cookie_str};

    #[test]
    fn derives_account_id_from_current_cookie_name() {
        assert_eq!(
            account_id_from_cookie_name("__Secure-pplx.session.account-123"),
            Some("account-123")
        );
        assert_eq!(account_id_from_cookie_name("__Secure-pplx.session."), None);
        assert_eq!(account_id_from_cookie_name("__Secure-next-auth.session-token"), None);
    }

    #[test]
    fn cookie_string_keeps_secure_prefix_valid() {
        assert_eq!(
            cookie_str("__Secure-pplx.session.account-123", "session-value"),
            "__Secure-pplx.session.account-123=session-value; Domain=www.perplexity.ai; Path=/; Secure"
        );
        assert_eq!(
            cookie_str("__Host-example", "session-value"),
            "__Host-example=session-value; Path=/; Secure"
        );
    }
}
