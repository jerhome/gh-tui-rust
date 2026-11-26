//! The HTTP client.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, LINK};
use url::Url;

use crate::link::next_link;
use crate::status::{classify, redact};
use crate::wire::decode_repos;
use crate::{Affiliation, ApiBase, Error, Repo, Result, Token};

/// Largest body read from one answer: a page of 100 repos is far below it; the limit only bounds memory.
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Most pages followed for one list; stops a server that loops through different `next` links.
const MAX_PAGES: usize = 1000;
/// Most bytes read over all the pages of one list (the per-answer limit alone allows 1000 x 32 MiB).
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

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
        use rustls_platform_verifier::BuilderVerifierExt;

        let setup = |message: String| Error::ClientSetup { message };

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_platform_verifier())
            .map_err(|e| setup(format!("TLS: {e}")))?
            .with_no_client_auth();

        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
            .map_err(|_| {
                setup(
                    "the token contains characters that cannot be sent in an HTTP header"
                        .to_owned(),
                )
            })?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );

        let mut builder = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .user_agent(concat!("ghu/", env!("CARGO_PKG_VERSION")))
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none());
        if base.is_loopback() {
            // Tests must reach the local fake server even when the environment sets a proxy.
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(|e| {
            redact(
                Error::ClientSetup {
                    message: error_chain(&e),
                },
                token.expose(),
            )
        })?;
        Ok(Self { http, base, token })
    }

    /// The base URL this client talks to.
    pub fn base(&self) -> &ApiBase {
        &self.base
    }

    /// `GET /user/repos?affiliation=…&per_page=100`, following every `next` page.
    ///
    /// `affiliations` is sent as a comma-separated list, deduplicated; an empty slice sends all three (GitHub's
    /// own default). Order: as GitHub returns it (by `full_name`), pages concatenated. Archived repos and forks
    /// are included; filtering is the caller's job.
    ///
    /// Errors: [`crate::Error::Unauthorized`], [`crate::Error::RateLimited`], [`crate::Error::Forbidden`],
    /// [`crate::Error::Network`], [`crate::Error::UnexpectedResponse`] (also for a `next` link outside the base
    /// origin, or a `next` link equal to the page just read), [`crate::Error::Api`]. The pages together may not exceed 128 MiB (else `UnexpectedResponse`).
    pub async fn user_repos(&self, affiliations: &[Affiliation]) -> Result<Vec<Repo>> {
        let mut url = self.route("/user/repos")?;
        url.query_pairs_mut()
            .append_pair("affiliation", &affiliation_list(affiliations))
            .append_pair("per_page", "100");

        let mut repos = Vec::new();
        let mut seen = HashSet::new();
        let mut total_bytes = 0usize;
        loop {
            if !seen.insert(url.clone()) || seen.len() > MAX_PAGES {
                return Err(
                    self.unexpected("the pagination does not end (repeated or too many pages)")
                );
            }
            let (headers, body) = self.get(url.clone(), body_limit(total_bytes)).await?;
            total_bytes += body.len();
            repos.extend(
                decode_repos(&body, self.token.expose())
                    .map_err(|e| redact(e, self.token.expose()))?,
            );
            let Some(next) = headers
                .get(LINK)
                .and_then(|v| v.to_str().ok())
                .and_then(next_link)
            else {
                return Ok(repos);
            };
            url = self.next_page_url(&url, next)?;
        }
    }

    /// `{base}{path}`.
    fn route(&self, path: &str) -> Result<Url> {
        Url::parse(&format!("{}{path}", self.base.as_str()))
            .map_err(|_| self.unexpected("cannot build the request URL"))
    }

    /// Resolves a `next` link against the page it came from and refuses any other origin than the base's.
    fn next_page_url(&self, current: &Url, next: &str) -> Result<Url> {
        let target = current
            .join(next)
            .map_err(|_| self.unexpected("a pagination link is not a valid URL"))?;
        let base = self.route("")?;
        if target.origin() != base.origin() {
            return Err(self.unexpected("a pagination link points outside the API origin"));
        }
        Ok(target)
    }

    /// One GET; a non-2xx answer becomes the matching [`Error`].
    async fn get(&self, url: Url, limit: usize) -> Result<(HeaderMap, Vec<u8>)> {
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| self.network(&e))?;
        let status = response.status();
        let headers = response.headers().clone();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| self.network(&e))? {
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(self.unexpected("the answer is too large"));
            }
            body.extend_from_slice(&chunk);
        }
        if status.is_success() {
            Ok((headers, body))
        } else {
            Err(redact(
                classify(status, &headers, &body, self.token.expose()),
                self.token.expose(),
            ))
        }
    }

    fn unexpected(&self, message: &str) -> Error {
        Error::UnexpectedResponse {
            message: message.to_owned(),
        }
    }

    fn network(&self, error: &reqwest::Error) -> Error {
        redact(
            Error::Network {
                message: error_chain(error),
            },
            self.token.expose(),
        )
    }
}

/// The comma-separated `affiliation` value: deduplicated, in the caller's order; all three when empty.
fn affiliation_list(affiliations: &[Affiliation]) -> String {
    const ALL: [Affiliation; 3] = [
        Affiliation::Owner,
        Affiliation::Collaborator,
        Affiliation::OrganizationMember,
    ];
    let list = if affiliations.is_empty() {
        &ALL[..]
    } else {
        affiliations
    };
    let mut seen = Vec::new();
    for affiliation in list {
        if !seen.contains(affiliation) {
            seen.push(*affiliation);
        }
    }
    let names: Vec<&str> = seen
        .iter()
        .map(|a| match a {
            Affiliation::Owner => "owner",
            Affiliation::Collaborator => "collaborator",
            Affiliation::OrganizationMember => "organization_member",
        })
        .collect();
    names.join(",")
}

/// Per-answer byte limit given the bytes already read for this list: never above [`MAX_BODY_BYTES`], and the
/// total over all pages never above [`MAX_TOTAL_BYTES`].
fn body_limit(total_read: usize) -> usize {
    MAX_BODY_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total_read))
}

/// A transport error as text, with its causes (reqwest's `Display` may include the request URL; the token is
/// never in it, and callers still pass the result through `redact`).
fn error_chain(error: &reqwest::Error) -> String {
    use std::error::Error as _;

    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{MAX_BODY_BYTES, MAX_TOTAL_BYTES, affiliation_list, body_limit};
    use crate::Affiliation::{Collaborator, Owner};

    #[test]
    fn body_limit_caps_each_answer_and_the_total() {
        assert_eq!(body_limit(0), MAX_BODY_BYTES);
        assert_eq!(body_limit(MAX_TOTAL_BYTES - 10), 10);
        assert_eq!(body_limit(MAX_TOTAL_BYTES), 0);
        assert_eq!(body_limit(usize::MAX), 0);
    }

    #[test]
    fn affiliation_list_deduplicates_and_defaults_to_all() {
        assert_eq!(
            affiliation_list(&[]),
            "owner,collaborator,organization_member"
        );
        assert_eq!(affiliation_list(&[Owner]), "owner");
        assert_eq!(
            affiliation_list(&[Collaborator, Owner, Collaborator]),
            "collaborator,owner"
        );
    }
}
