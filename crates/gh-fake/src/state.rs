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
    /// Instant written in objects created by `POST` routes (`created_at` of a Comment, `submitted_at` of a
    /// Review, `updated_at` of the PR), `YYYY-MM-DDTHH:MM:SSZ` (served as is, not checked). Default
    /// `2026-04-01T12:00:00Z`; tests set it to tell old and new entries apart (ADR 0003).
    pub now: String,
    /// Next id given to a created Comment or Review, then incremented. Default `1_000_000`.
    pub next_id: u64,
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
            now: "2026-04-01T12:00:00Z".to_owned(),
            next_id: 1_000_000,
        }
    }

    /// The PR `number` of `owner/repo` (owner and repo compared case-insensitively), for tests to edit its
    /// reviews, comments and files. `None` if there is none. Never panics.
    pub fn pull_mut(&mut self, owner: &str, repo: &str, number: u64) -> Option<&mut Pull> {
        self.pulls.iter_mut().find(|p| {
            p.number == number
                && p.owner.eq_ignore_ascii_case(owner)
                && p.repo.eq_ignore_ascii_case(repo)
        })
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
        let mut shared = Repo::new("bob", "shared-tools");
        shared.description = Some("Scripts shared with Alice".into());
        shared.access = Access::Collaborator;
        let mut widgets = Repo::new("acme", "widgets");
        widgets.description = Some("Widget toolkit".into());
        widgets.private = true;
        widgets.access = Access::OrganizationMember;
        let mut legacy = Repo::new("acme", "legacy-site");
        legacy.archived = true;
        legacy.access = Access::OrganizationMember;
        let mut public_lib = Repo::new("carol", "public-lib");
        public_lib.description = Some("A public library of someone else".into());
        public_lib.access = Access::NoAffiliation;
        state.repos = vec![
            notes, garden, dotfiles, old, shared, widgets, legacy, public_lib,
        ];

        let pr = |number: u64, title: &str, author: &str, pull_state: PullState, day: u32| {
            let mut p = Pull::new("alice", "notes", number, title, author);
            p.state = pull_state;
            p.created_at = format!("2026-02-{day:02}T09:00:00Z");
            p.updated_at = format!("2026-03-{day:02}T12:30:00Z");
            p
        };
        let mut first = pr(1, "Fix typo in the README", "bob", PullState::Open, 3);
        first.body = Some(DEMO_BODY.to_owned());
        first.files = vec![
            PullFile::modified(
                "README.md",
                "@@ -1,3 +1,3 @@\n # Notes\n-Plain-text notess and snippets\n+Plain-text notes and snippets\n See the guide.\n",
            ),
            PullFile {
                filename: "docs/guide.md".into(),
                previous_filename: None,
                status: FileStatus::Added,
                additions: 2,
                deletions: 0,
                patch: Some("@@ -0,0 +1,2 @@\n+# Guide\n+Start here.\n".into()),
            },
            PullFile {
                filename: "scripts/export.sh".into(),
                previous_filename: None,
                status: FileStatus::Removed,
                additions: 0,
                deletions: 2,
                patch: Some("@@ -1,2 +0,0 @@\n-#!/bin/sh\n-echo export\n".into()),
            },
            PullFile {
                filename: "docs/tags.md".into(),
                previous_filename: Some("docs/labels.md".into()),
                status: FileStatus::Renamed,
                additions: 0,
                deletions: 0,
                patch: None,
            },
            PullFile {
                filename: "assets/logo.png".into(),
                previous_filename: None,
                status: FileStatus::Added,
                additions: 0,
                deletions: 0,
                patch: None,
            },
        ];
        let demo_review =
            |id: u64, author: &str, review_state: ReviewState, body: &str| PullReview {
                id,
                author: Some(author.to_owned()),
                state: review_state,
                body: body.to_owned(),
                submitted_at: (review_state != ReviewState::Pending)
                    .then(|| format!("2026-03-0{}T10:00:00Z", id - 100)),
                commit_id: first.head_sha.clone(),
            };
        first.reviews = vec![
            demo_review(101, "carol", ReviewState::Approved, ""),
            demo_review(
                102,
                "dave",
                ReviewState::ChangesRequested,
                "Please also fix the title.",
            ),
            demo_review(
                103,
                "erin",
                ReviewState::Commented,
                "Looks fine, one question about the guide.",
            ),
            demo_review(104, "alice", ReviewState::Pending, "Draft of my review"),
        ];
        first.comments = vec![
            IssueComment {
                id: 201,
                author: Some("carol".into()),
                body: "Thanks, good catch.".into(),
                created_at: "2026-03-02T09:00:00Z".into(),
            },
            IssueComment {
                id: 202,
                author: Some("bob".into()),
                body: "Fixed in the last commit.".into(),
                created_at: "2026-03-03T09:30:00Z".into(),
            },
        ];
        let mut draft = pr(2, "WIP: full-text search", "carol", PullState::Open, 5);
        draft.draft = true;
        draft.head = "search".into();
        state.pulls = vec![
            first,
            draft,
            pr(
                3,
                "Drop the unused export script",
                "bob",
                PullState::Closed,
                8,
            ),
            pr(4, "Add a tag index", "carol", PullState::Merged, 10),
        ];
        // Own PR of alice in a repo of someone else, a PR of an archived repo, a PR with conflicts.
        let mut own = Pull::new("bob", "shared-tools", 1, "Add a lint script", "alice");
        own.head = "lint".into();
        let mut frozen = Pull::new("acme", "legacy-site", 1, "Update the footer", "bob");
        frozen.head = "footer".into();
        let mut conflicting = Pull::new("acme", "widgets", 1, "Rename the Button widget", "carol");
        conflicting.head = "rename-button".into();
        conflicting.mergeable = Some(false);
        state.pulls.extend([own, frozen, conflicting]);
        state
    }
}

const DEMO_BODY: &str = "# Fix typo\n\nThe README said **notess**. Changes:\n\n- fix the typo\n- add a guide\n- drop the export script\n\n```sh\ngrep -n notess README.md\n```\n\n![screenshot](https://example.com/shot.png)\n\n<b>Raw HTML</b> stays as text.";

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
    /// `head.sha` (ADR 0003). [`Pull::new`]: the number as 40 lowercase hex digits (`format!("{number:040x}")`).
    pub head_sha: String,
    /// `mergeable`; `None` serves `null` (still computed). [`Pull::new`]: `Some(true)`.
    pub mergeable: Option<bool>,
    /// Changed files, in the order served by `GET …/pulls/{n}/files`. [`Pull::new`]: empty. `additions`,
    /// `deletions` and `changed_files` of the PR are computed from them.
    pub files: Vec<PullFile>,
    /// Reviews, chronological (served in this order, `PENDING` included, as GitHub does for the token's user).
    /// [`Pull::new`]: empty.
    pub reviews: Vec<PullReview>,
    /// Conversation comments, served sorted by id. [`Pull::new`]: empty.
    pub comments: Vec<IssueComment>,
}

impl Pull {
    /// An open, non-draft PR of `owner/repo` by `author`, without body, created and updated at
    /// `2026-01-01T00:00:00Z`, from `feature` into `main`; head sha from the number, mergeable, no file, review
    /// nor comment.
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
            head_sha: format!("{number:040x}"),
            mergeable: Some(true),
            files: Vec::new(),
            reviews: Vec::new(),
            comments: Vec::new(),
        }
    }
}

/// A changed file of a PR (JSON of "List pull requests files": `filename`, `previous_filename`, `status`,
/// `additions`, `deletions`, `changes`, `patch`, plus `sha`, `blob_url`, `raw_url`, `contents_url`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullFile {
    /// `filename`: path in the head commit.
    pub filename: String,
    /// Served as `previous_filename` when `Some` (absent otherwise).
    pub previous_filename: Option<String>,
    /// `status`.
    pub status: FileStatus,
    /// `additions`.
    pub additions: u64,
    /// `deletions`.
    pub deletions: u64,
    /// `None`: the `patch` key is absent (binary, too large, rename without change).
    pub patch: Option<String>,
}

impl PullFile {
    /// A [`FileStatus::Modified`] file without previous name, whose `additions` and `deletions` count the lines
    /// of `patch` starting with `+` / `-` (lines split as `str::lines`).
    pub fn modified(filename: &str, patch: &str) -> Self {
        let count = |sign: char| {
            u64::try_from(patch.lines().filter(|l| l.starts_with(sign)).count())
                .expect("a line count fits in u64")
        };
        Self {
            filename: filename.to_owned(),
            previous_filename: None,
            status: FileStatus::Modified,
            additions: count('+'),
            deletions: count('-'),
            patch: Some(patch.to_owned()),
        }
    }
}

/// `status` of a changed file, served in lowercase (`added`, `removed`…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// `added`.
    Added,
    /// `removed`.
    Removed,
    /// `modified`.
    Modified,
    /// `renamed`.
    Renamed,
    /// `copied`.
    Copied,
    /// `changed`.
    Changed,
    /// `unchanged`.
    Unchanged,
}

/// A review (JSON of "List reviews for a pull request": `id`, `user`, `body`, `state`, `commit_id`,
/// `submitted_at` (absent when `None`), `html_url`, `author_association`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullReview {
    /// `id`.
    pub id: u64,
    /// Author login; `None` serves `"user": null`.
    pub author: Option<String>,
    /// `state`.
    pub state: ReviewState,
    /// `body` (GitHub sends `""` for none).
    pub body: String,
    /// `submitted_at`; `None`: the key is absent (Pending).
    pub submitted_at: Option<String>,
    /// `commit_id`.
    pub commit_id: String,
}

/// `state` of a review, served as GitHub writes it (`APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`,
/// `PENDING`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    /// `APPROVED`.
    Approved,
    /// `CHANGES_REQUESTED`.
    ChangesRequested,
    /// `COMMENTED`.
    Commented,
    /// `DISMISSED`.
    Dismissed,
    /// `PENDING` (started on the website, not submitted).
    Pending,
}

/// A conversation comment of a PR (JSON of "List issue comments": `id`, `user`, `body`, `created_at`,
/// `updated_at` (same as `created_at`), `html_url`, `author_association`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueComment {
    /// `id`.
    pub id: u64,
    /// Author login; `None` serves `"user": null`.
    pub author: Option<String>,
    /// `body`.
    pub body: String,
    /// `created_at`, served as is.
    pub created_at: String,
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
