//! GitHub REST v3 client used by `ghu` (ADR 0001).
//!
//! Contract:
//! - The client talks to one [`ApiBase`] (default [`DEFAULT_API_BASE`]) with one [`Token`]; both are given by
//!   the caller. This crate never reads the environment and never runs `gh`: token lookup belongs to `ghu`.
//! - One `async` method per GitHub route, all returning [`Result`]. Methods follow `Link: rel="next"` pagination
//!   themselves and return the whole list (scale limits are out of scope for 0.1).
//! - The token is only ever written into the `Authorization` header. It never appears in `Debug` output,
//!   error messages or logs ([`Token`] has a masked `Debug` and no `Display`).
//! - No GraphQL in 0.1. Wire (JSON) types are private; the public model types in this crate are what callers see.
//! - Routes: `GET /user/repos` (#2, #3), `GET /repos/{owner}/{repo}` (#4), `GET /repos/{owner}/{repo}/pulls` (#5);
//!   ADR 0003 (#6-#10): `GET /user`, one PR, its files, reviews and conversation comments, and the two publishing
//!   routes (`POST` of a Comment, of a Review). Publishing routes are never retried.
//! - Texts to publish are [`MessageBody`] values, checked before any request ([`Error::InvalidBody`]).

mod base;
mod client;
mod error;
mod link;
mod model;
mod status;
mod token;
mod wire;

pub use base::ApiBase;
pub use client::Client;
pub use error::Error;
pub use model::{
    Affiliation, BodyProblem, Comment, CommitSha, FileChange, FileStatus, MAX_BODY_CHARS,
    Mergeable, MessageBody, PrNumber, PrState, PullDetail, PullFilter, PullRequest, Repo, RepoName,
    Review, ReviewRequest, ReviewState, Timestamp, User, Verdict,
};
pub use token::Token;

/// Result type of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Base URL of the public GitHub REST API, used when `ghu` is given no other.
pub const DEFAULT_API_BASE: &str = "https://api.github.com";
