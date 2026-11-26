//! The base URL of the API.

use crate::Error;

/// Base URL of a GitHub REST API (`https://api.github.com`, or the fake server in tests).
///
/// Invariants (checked by [`ApiBase::parse`]):
/// - scheme `https`, or `http` only when the host is a loopback address (`localhost`, `127.0.0.0/8`, `::1`),
///   so the token never travels in clear text to another machine;
/// - no user info, query or fragment;
/// - a path is allowed (kept for a future `/api/v3` prefix) and stored without its trailing `/`, so routes
///   are joined as `{base}/user/repos`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiBase(url::Url);

impl ApiBase {
    /// Parses and validates a base URL (the value of `GHU_API_URL`, for example).
    ///
    /// Leading and trailing whitespace is ignored. Errors: [`Error::InvalidBaseUrl`] with a reason a user can act
    /// on (not a URL, wrong scheme, clear-text `http` to a non-loopback host, user info, query or fragment).
    pub fn parse(input: &str) -> Result<Self, Error> {
        todo!()
    }

    /// The base URL as text, without trailing `/` (`https://api.github.com`, `http://127.0.0.1:40123`).
    pub fn as_str(&self) -> &str {
        todo!()
    }
}

impl Default for ApiBase {
    /// [`crate::DEFAULT_API_BASE`].
    fn default() -> Self {
        todo!()
    }
}
