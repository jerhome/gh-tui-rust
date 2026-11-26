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
    /// Pull requests of every repo, in any order. `GET /repos/{owner}/{repo}/pulls` serves those whose
    /// [`Pull::owner`]/[`Pull::repo`] match the path (case-insensitive), filtered by `state` and sorted by the
    /// `sort`/`direction` query (#5).
    pub pulls: Vec<Pull>,
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
        Self {
            token: crate::DEFAULT_TOKEN.to_owned(),
            user,
            repos: Vec::new(),
            pulls: Vec::new(),
            latency: Duration::ZERO,
            max_per_page: 100,
            failures: Vec::new(),
            requests: Vec::new(),
        }
    }

    /// Demo data (also used by the `gh-fake` binary). Fictional names only. Must keep what #2's tests rely on:
    /// user `alice` (id 1) owning `alice/notes` (description `Plain-text notes and snippets`),
    /// `alice/secret-garden` (private, with a description), `alice/dotfiles` (fork) and `alice/old-experiments`
    /// (archived), all [`Access::Owner`].
    ///
    /// Added for #3-#5 (dev[fake]): at least one [`Access::Collaborator`] repo of another user, two
    /// [`Access::OrganizationMember`] repos of an organization, one archived; one [`Access::NoAffiliation`] public
    /// repo of another user (to open by `owner/name`); and PRs on `alice/notes`: at least two open (one draft),
    /// one closed, one merged, with distinct `updated_at` and at least two authors.
    pub fn demo() -> Self {
        let mut state = Self::new(User {
            login: "alice".into(),
            id: 1,
        });
        let mut notes = Repo::new("alice", "notes");
        notes.description = Some("Plain-text notes and snippets".into());
        let mut garden = Repo::new("alice", "secret-garden");
        garden.description = Some("Private plans for the garden".into());
        garden.private = true;
        let mut dotfiles = Repo::new("alice", "dotfiles");
        dotfiles.fork = true;
        let mut old = Repo::new("alice", "old-experiments");
        old.archived = true;
        state.repos = vec![notes, garden, dotfiles, old];
        state
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
        Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
            description: None,
            private: false,
            fork: false,
            archived: false,
            access: Access::Owner,
        }
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
    /// No affiliation: never listed by `GET /user/repos`, but served by `GET /repos/{owner}/{repo}` (a public repo
    /// of someone else, opened by typing `owner/name`, #4).
    NoAffiliation,
}

/// A pull request of the fake server (JSON of "List pull requests", <https://docs.github.com/en/rest/pulls/pulls>).
///
/// JSON fields served: `id` (stable fake id), `number`, `state` (`"open"` or `"closed"`), `title`, `user`
/// (`{"login", "id", "type": "User"}`, or `null` when [`Pull::author`] is `None`), `body`, `draft`,
/// `created_at`, `updated_at`, `closed_at` (`null` when open, else `updated_at`), `merged_at` (`updated_at` when
/// [`PullState::Merged`], else `null`), `head` and `base` (`{"ref", "label": "<owner>:<ref>"}`), `html_url`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    /// Owner of the repo the PR belongs to.
    pub owner: String,
    /// Name of the repo the PR belongs to.
    pub repo: String,
    /// Number within the repo (unique per repo; not checked).
    pub number: u64,
    /// Title.
    pub title: String,
    /// Author login; `None` serves `"user": null` (deleted account).
    pub author: Option<String>,
    /// Open, closed or merged.
    pub state: PullState,
    /// `draft` in JSON (served as is, whatever the state).
    pub draft: bool,
    /// Description (`body`, `null` when `None`).
    pub body: Option<String>,
    /// `created_at`, `YYYY-MM-DDTHH:MM:SSZ` (served as is, not checked).
    pub created_at: String,
    /// `updated_at`, same format (served as is, not checked).
    pub updated_at: String,
    /// Source branch (`head.ref`).
    pub head: String,
    /// Target branch (`base.ref`).
    pub base: String,
}

impl Pull {
    /// An open, non-draft PR of `owner/repo` by `author`, without body, created and updated at
    /// `2026-01-01T00:00:00Z`, from `feature` into `main`.
    pub fn new(owner: &str, repo: &str, number: u64, title: &str, author: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number,
            title: title.to_owned(),
            author: Some(author.to_owned()),
            state: PullState::Open,
            draft: false,
            body: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            head: "feature".to_owned(),
            base: "main".to_owned(),
        }
    }
}

/// State of a fake PR. Served as `state` `"open"` or `"closed"`, plus `merged_at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullState {
    /// `state: "open"`.
    Open,
    /// `state: "closed"`, `merged_at: null`.
    Closed,
    /// `state: "closed"`, `merged_at` set.
    Merged,
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
