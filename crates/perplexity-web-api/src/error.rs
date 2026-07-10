use std::time::Duration;
use thiserror::Error;

/// All possible errors that can occur when using the Perplexity client.
#[derive(Error, Debug)]
pub enum Error {
    /// HTTP client initialization failed.
    #[error("HTTP client initialization failed: {0}")]
    HttpClientInit(#[source] rquest::Error),

    /// Session warm-up request failed.
    #[error("Session warmup failed: {0}")]
    SessionWarmup(#[source] rquest::Error),

    /// Search request failed.
    #[error("Search request failed: {0}")]
    SearchRequest(#[source] rquest::Error),

    /// File upload request failed.
    #[error("Upload request failed: {0}")]
    UploadRequest(#[source] rquest::Error),

    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Request timed out.
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),

    /// File uploads require authentication cookies.
    #[error("File uploads require authentication cookies")]
    FileUploadRequiresAuth,

    /// Connector sources require authentication cookies.
    #[error(
        "Connector sources (e.g. google_drive, gcal, notion_mcp) require authentication cookies"
    )]
    ConnectorRequiresAuth,

    /// Failed to get upload URL.
    #[error("Failed to get upload URL: {0}")]
    UploadUrlFailed(#[source] rquest::Error),

    /// S3 upload failed.
    #[error("S3 upload failed: {0}")]
    S3UploadFailed(#[source] rquest::Error),

    /// Batch upload response did not contain the expected file entry.
    #[error("Missing file entry in batch upload response")]
    MissingUploadResponse,

    /// Attachment processing SSE subscription or streaming failed.
    #[error("Attachment processing failed: {0}")]
    AttachmentProcessing(#[source] rquest::Error),

    /// Invalid MIME type.
    #[error("Invalid MIME type: {0}")]
    InvalidMimeType(String),

    /// Invalid UTF-8 in SSE stream.
    #[error("Invalid UTF-8 in SSE stream")]
    InvalidUtf8,

    /// Server returned an error response.
    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },

    /// Stream ended unexpectedly.
    #[error("Stream ended unexpectedly")]
    UnexpectedEndOfStream,

    #[error("Invalid API base url")]
    InvalidBaseUrl,

    /// Failed to fetch CSRF token from /api/auth/csrf.
    #[error("Failed to fetch CSRF token: {0}")]
    CsrfFetch(#[source] rquest::Error),

    /// CSRF token was not present in the /api/auth/csrf response.
    #[error("CSRF token missing from /api/auth/csrf response")]
    CsrfTokenMissing,

    /// Both a custom HTTP client and authentication cookies were provided.
    #[error(
        "cannot combine custom HTTP client with authentication cookies; manage cookies on the provided client directly"
    )]
    CustomClientWithCookies,

    /// Rate-limit inspection requires authentication cookies.
    #[error(
        "Rate-limit inspection requires authentication cookies (set PERPLEXITY_SESSION_TOKEN)"
    )]
    RateLimitRequiresAuth,

    /// Failed to fetch rate limits from /rest/rate-limit/all.
    #[error("Failed to fetch rate limits: {0}")]
    RateLimitFetch(#[source] rquest::Error),

    /// The relevant plan quota is exhausted for the requested search mode.
    ///
    /// Perplexity's SSE endpoint responds with an empty answer (rather than an
    /// HTTP error) once a quota runs out; this error is raised after detecting
    /// that condition via `/rest/rate-limit/all`, so callers get an actionable
    /// message instead of a silent empty result.
    #[error(
        "Perplexity quota exhausted: {feature} has {remaining} queries remaining. \
         The request was not answered because the plan limit for this mode is reached."
    )]
    RateLimited {
        /// Human-readable feature name (e.g. "Pro Search", "Deep Research").
        feature: &'static str,
        /// Remaining queries for that feature (0 when exhausted).
        remaining: i64,
    },
}

/// Convenience Result type for this crate.
pub type Result<T> = std::result::Result<T, Error>;
