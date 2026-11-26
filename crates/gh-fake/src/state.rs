//! The in-memory state of the fake server. Plain data with public fields: tests set up and inspect it freely.

use std::time::Duration;

/// Everything the fake server knows. Read and modified through [`crate::FakeGitHub::with_state`].
#[derive(Clone, Debug)]
pub struct State {
    /// The only token accepted (compared exactly, after the `Bearer ` or `token ` prefix).
    pub token: String,
    /// The signed-in user (owner of the token).
    pub user: User,
    /// Repos the user can see, in any order; `GET /user/repos` returns them sorted by `owner/name`
    /// (case-insensitive), filtered by [`Repo::access`] against the `affiliation` query.
    pub repos: Vec<Repo>,
    /// Delay added before handling every request (default zero). Used to test the loading indicator and that the
    /// UI stays responsive.
    pub latency: Duration,
    /// Largest page size served, whatever `per_page` asks (default 100, GitHub's maximum). A test sets it low to
    /// force pagination; `per_page` defaults to 30 as on GitHub.
    pub max_per_page: usize,
    /// Injected failures, checked in order after authentication; the first that matches answers instead of the
    /// route.
    pub failures: Vec<Failure>,
    /// Every request received, oldest first, including refused ones.
    pub requests: Vec<RecordedRequest>,
}

impl State {
    /// An empty state for `user`: token [`crate::DEFAULT_TOKEN`], no repo, no latency, `max_per_page` 100.
    pub fn new(user: User) -> Self {
        todo!()
    }

    /// Demo data (also used by the `gh-fake` binary): user `alice` (id 1) owning at least three repos, one of
    /// them a fork, one archived, one private with a description. Fictional names only.
    pub fn demo() -> Self {
        todo!()
    }
}

/// The signed-in user (`GET /user`, owner of repos).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// Login, for example `alice`.
    pub login: String,
    /// Numeric id.
    pub id: u64,
}

/// A repo of the fake server. JSON field `full_name` is `{owner}/{name}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// Owner login (the user, an organization, or another user).
    pub owner: String,
    /// Repo name.
    pub name: String,
    /// Description (`null` in JSON when `None`).
    pub description: Option<String>,
    /// `private` in JSON.
    pub private: bool,
    /// `fork` in JSON.
    pub fork: bool,
    /// `archived` in JSON.
    pub archived: bool,
    /// How the user reaches it (matched against the `affiliation` query of `GET /user/repos`).
    pub access: Access,
}

impl Repo {
    /// A public, non-fork, non-archived repo without description, owned by the user ([`Access::Owner`]).
    pub fn new(owner: &str, name: &str) -> Self {
        todo!()
    }
}

/// How the user reaches a repo; mirrors GitHub's `affiliation` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// `owner`.
    Owner,
    /// `collaborator`.
    Collaborator,
    /// `organization_member`.
    OrganizationMember,
}

/// HTTP method of a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
}

/// An injected failure: requests matching `method` and `path` get `response` instead of the route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Method to match.
    pub method: Method,
    /// Exact path to match, without query (`/user/repos`).
    pub path: String,
    /// What to answer.
    pub response: FailureResponse,
    /// How many more times it applies (`None` = always). Decremented on each use; removed at zero.
    pub remaining: Option<u32>,
}

/// A GitHub error answer, with GitHub's status, headers and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureResponse {
    /// 401 `Bad credentials`.
    Unauthorized,
    /// 403 with `message`, `x-ratelimit-remaining` > 0.
    Forbidden {
        /// The `message` field.
        message: String,
    },
    /// 403 `API rate limit exceeded…` with `x-ratelimit-remaining: 0` and `x-ratelimit-reset` one hour ahead.
    RateLimited,
    /// 404 `Not Found`.
    NotFound,
    /// 422 with `message` (and an `errors` array holding the same text).
    Unprocessable {
        /// The `message` field.
        message: String,
    },
    /// 500 with an empty JSON object.
    ServerError,
}

/// A request as the fake server received it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedRequest {
    /// Method. Requests with another method get 404 `Not Found` and are not recorded.
    pub method: Method,
    /// Path without query.
    pub path: String,
    /// Raw query string, if any.
    pub query: Option<String>,
    /// The token sent (after `Bearer ` or `token `), if any. Test data only.
    pub token: Option<String>,
    /// JSON body, if any and valid.
    pub body: Option<serde_json::Value>,
}
