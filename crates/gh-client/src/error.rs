//! The error type of the crate.

/// Every failure of this crate. Variants are what `ghu` tells apart to choose a message or an action.
///
/// `Display` is a short English text meant for logs; `ghu` writes its own user-facing messages from the variant.
/// No variant ever carries the token (only GitHub's `message` field, URLs without credentials, and reasons).
/// `Clone` so the UI can keep the last error of a view.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The API base URL was refused by [`crate::ApiBase::parse`].
    #[error("invalid API URL: {reason}")]
    InvalidBaseUrl {
        /// Why, in a form the user can act on.
        reason: String,
    },
    /// The text is not a valid `owner/name` ([`crate::RepoName`]).
    #[error("invalid repo name `{input}`: expected owner/name")]
    InvalidRepoName {
        /// The input as typed.
        input: String,
    },
    /// The HTTP client could not be built (TLS setup, OS trust store).
    #[error("cannot set up the HTTP client: {message}")]
    ClientSetup {
        /// Underlying cause.
        message: String,
    },
    /// No answer from GitHub: DNS, connection, TLS, timeout (connect 10 s, whole request 30 s).
    #[error("network error: {message}")]
    Network {
        /// Underlying cause (may contain the request URL, never the token).
        message: String,
    },
    /// HTTP 401: the token is invalid, expired or revoked.
    #[error("GitHub rejected the token (401)")]
    Unauthorized,
    /// HTTP 403 or 429 with `x-ratelimit-remaining: 0` (primary limit), or a 403/429 whose message mentions a
    /// secondary rate limit.
    #[error("GitHub rate limit exceeded")]
    RateLimited {
        /// When the limit resets, in seconds since the Unix epoch (`x-ratelimit-reset`), if GitHub said so.
        reset_at: Option<u64>,
    },
    /// HTTP 403 that is not a rate limit: missing rights or scope.
    #[error("GitHub denied access (403): {message}")]
    Forbidden {
        /// GitHub's `message` field (empty if the body had none).
        message: String,
    },
    /// HTTP 404: missing, or invisible to this token (GitHub answers 404 for private repos it hides).
    #[error("not found on GitHub (404)")]
    NotFound,
    /// HTTP 422: GitHub refused the request (for example a review blocked by a pending review).
    #[error("GitHub refused the request (422): {message}")]
    Unprocessable {
        /// GitHub's `message` field, followed by the `errors[].message` entries if any, joined with `; `.
        message: String,
    },
    /// Any other non-2xx status.
    #[error("GitHub error {status}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// GitHub's `message` field (empty if the body had none).
        message: String,
    },
    /// A 2xx answer that could not be used: invalid JSON, missing field, or a pagination `next` link pointing
    /// outside the [`crate::ApiBase`] origin (refused so the token is never sent elsewhere).
    #[error("unexpected response from GitHub: {message}")]
    UnexpectedResponse {
        /// What was wrong.
        message: String,
    },
}
