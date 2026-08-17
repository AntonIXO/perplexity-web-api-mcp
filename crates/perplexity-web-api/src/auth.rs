/// Default cookie name for the Perplexity session token.
///
/// Existing deployments used the secure NextAuth cookie. Perplexity migrated
/// to per-account cookies named `__Secure-pplx.session.<account-uuid>`;
/// override this default with [`AuthCookies::with_cookie_name`].
pub const SESSION_TOKEN_COOKIE_NAME: &str = "__Secure-next-auth.session-token";
/// Cookie name for the Perplexity CSRF token.
/// Set dynamically via `/api/auth/csrf`; no longer extracted from browser.
pub const CSRF_TOKEN_COOKIE_NAME: &str = "next-auth.csrf-token";

/// Session token required for authenticated Perplexity features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCookies {
    session_token: String,
    session_cookie_name: String,
}

impl AuthCookies {
    /// Creates a new authentication context with a session token.
    pub fn new(session_token: impl Into<String>) -> Self {
        Self {
            session_token: session_token.into(),
            session_cookie_name: SESSION_TOKEN_COOKIE_NAME.to_owned(),
        }
    }

    /// Overrides the cookie name the session token is sent under.
    ///
    /// Use this for the current Perplexity scheme, where the cookie is named
    /// `__Secure-pplx.session.<account-uuid>`.
    pub fn with_cookie_name(mut self, name: impl Into<String>) -> Self {
        self.session_cookie_name = name.into();
        self
    }

    /// Returns the session token value.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    /// Returns the cookie name the session token is sent under.
    pub fn session_cookie_name(&self) -> &str {
        &self.session_cookie_name
    }

    pub(crate) fn session_cookie_pair(&self) -> (&str, &str) {
        (self.session_cookie_name(), self.session_token())
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthCookies, SESSION_TOKEN_COOKIE_NAME};

    #[test]
    fn uses_perplexitys_secure_session_cookie_name() {
        let cookies = AuthCookies::new("test-session-token");

        assert_eq!(
            cookies.session_cookie_pair(),
            ("__Secure-next-auth.session-token", "test-session-token")
        );
        assert_eq!(SESSION_TOKEN_COOKIE_NAME, "__Secure-next-auth.session-token");
    }
}
