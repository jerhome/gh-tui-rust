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
//! - Later tickets add methods (repo by name, pull requests, files, reviews, comments, publishing) on [`Client`]
//!   following the same shape, and variants on the `#[non_exhaustive]` types.

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
pub use model::{Affiliation, Repo, RepoName};
pub use token::Token;

/// Result type of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Base URL of the public GitHub REST API, used when `ghu` is given no other.
pub const DEFAULT_API_BASE: &str = "https://api.github.com";
