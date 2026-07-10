/// Cookie name for the Perplexity session token.
pub const SESSION_TOKEN_COOKIE_NAME: &str = "next-auth.session-token";
/// Cookie name for the Perplexity CSRF token.
/// Set dynamically via `/api/auth/csrf`; no longer extracted from browser.
pub const CSRF_TOKEN_COOKIE_NAME: &str = "next-auth.csrf-token";

/// Session token required for authenticated Perplexity features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCookies {
    session_token: String,
}

impl AuthCookies {
    /// Creates a new authentication context with a session token.
    pub fn new(session_token: impl Into<String>) -> Self {
        Self { session_token: session_token.into() }
    }

    /// Returns the session token value.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    pub(crate) fn session_cookie_pair(&self) -> (&str, &str) {
        (SESSION_TOKEN_COOKIE_NAME, self.session_token())
    }
}
