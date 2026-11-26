//! The HTTP client.

use crate::{Affiliation, ApiBase, Repo, Result, Token};

/// GitHub REST v3 client.
///
/// Ownership and threads: owns its base URL and token; `Clone` is cheap (shared connection pool) and the type is
/// `Send + Sync`, so `ghu` clones it into each background task. Methods are `async`, need a running tokio
/// runtime, never block the calling thread, and may run concurrently.
///
/// Every request sends `Authorization: Bearer <token>`, `Accept: application/vnd.github+json`,
/// `X-GitHub-Api-Version: 2022-11-28` and `User-Agent: ghu/<crate version>`. Redirects are not followed;
/// proxies from the environment are ignored for loopback bases (tests). Timeouts: connect 10 s, request 30 s.
/// TLS: rustls with the ring provider and the OS trust store (`rustls-platform-verifier`).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Client {
    http: reqwest::Client,
    base: ApiBase,
    token: Token,
}

impl Client {
    /// Builds the client. No request is sent.
    ///
    /// Errors: [`crate::Error::ClientSetup`] if TLS or the HTTP stack cannot be initialised.
    pub fn new(base: ApiBase, token: Token) -> Result<Self> {
        todo!()
    }

    /// The base URL this client talks to.
    pub fn base(&self) -> &ApiBase {
        todo!()
    }

    /// `GET /user/repos?affiliation=…&per_page=100`, following every `next` page.
    ///
    /// `affiliations` is sent as a comma-separated list, deduplicated; an empty slice sends all three (GitHub's
    /// own default). Order: as GitHub returns it (by `full_name`), pages concatenated. Archived repos and forks
    /// are included; filtering is the caller's job.
    ///
    /// Errors: [`crate::Error::Unauthorized`], [`crate::Error::RateLimited`], [`crate::Error::Forbidden`],
    /// [`crate::Error::Network`], [`crate::Error::UnexpectedResponse`] (also for a `next` link outside the base
    /// origin, or a `next` link equal to the page just read), [`crate::Error::Api`].
    pub async fn user_repos(&self, affiliations: &[Affiliation]) -> Result<Vec<Repo>> {
        todo!()
    }
}
