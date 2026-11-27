//! Private JSON types of GitHub's answers, and their conversion to the public model.

use serde::Deserialize;

use crate::status::truncate;
use crate::{
    Comment, CommitSha, Error, FileChange, FileStatus, Mergeable, PrNumber, PrState, PullDetail,
    PullRequest, Repo, RepoName, Result, Review, ReviewState, Timestamp, User,
};

/// Decodes the body of `GET /user/repos` (one page).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field or an invalid `full_name`.
pub(crate) fn decode_repos(body: &[u8], secret: &str) -> Result<Vec<Repo>> {
    let wire: Vec<WireRepo> =
        serde_json::from_slice(body).map_err(|e| Error::UnexpectedResponse {
            message: truncate(&format!("cannot decode the repo list: {e}"), secret),
        })?;
    wire.into_iter()
        .map(|repo| repo_from_wire(repo, secret))
        .collect()
}

/// Decodes the body of `GET /repos/{owner}/{repo}`.
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field or an invalid `full_name`.
pub(crate) fn decode_repo(body: &[u8], secret: &str) -> Result<Repo> {
    let wire: WireRepo = serde_json::from_slice(body).map_err(|e| Error::UnexpectedResponse {
        message: truncate(&format!("cannot decode the repo: {e}"), secret),
    })?;
    repo_from_wire(wire, secret)
}

/// Decodes the body of `GET /repos/{owner}/{repo}/pulls` (one page).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field, a number of 0, an invalid
/// `updated_at` or a `state` other than `open` / `closed`.
pub(crate) fn decode_pulls(body: &[u8], secret: &str) -> Result<Vec<PullRequest>> {
    let wire: Vec<WirePull> =
        serde_json::from_slice(body).map_err(|e| Error::UnexpectedResponse {
            message: truncate(&format!("cannot decode the pull request list: {e}"), secret),
        })?;
    wire.into_iter()
        .map(|pull| pull_from_wire(pull, secret))
        .collect()
}

/// A pull request as GitHub's JSON spells it (only the fields we use).
#[derive(Deserialize)]
struct WirePull {
    number: u64,
    title: String,
    user: Option<WireUser>,
    state: String,
    #[serde(default)]
    draft: bool,
    merged_at: Option<String>,
    updated_at: String,
}

#[derive(Deserialize)]
struct WireUser {
    login: String,
}

fn pull_from_wire(wire: WirePull, secret: &str) -> Result<PullRequest> {
    let unexpected = |message: String| Error::UnexpectedResponse {
        message: truncate(&message, secret),
    };
    let number = PrNumber::new(wire.number)
        .ok_or_else(|| unexpected("a pull request has the number 0".to_owned()))?;
    let updated_at = Timestamp::parse(&wire.updated_at)
        .ok_or_else(|| unexpected(format!("invalid updated_at `{}`", wire.updated_at)))?;
    let state = pr_state(&wire.state, wire.draft, wire.merged_at.is_some())
        .ok_or_else(|| unexpected(format!("unknown pull request state `{}`", wire.state)))?;
    Ok(PullRequest {
        number,
        title: wire.title,
        author: wire.user.map_or_else(|| "ghost".to_owned(), |u| u.login),
        state,
        updated_at,
    })
}

/// A repo as GitHub's JSON spells it (only the fields we use).
#[derive(Deserialize)]
struct WireRepo {
    full_name: String,
    description: Option<String>,
    private: bool,
    fork: bool,
    archived: bool,
}

fn repo_from_wire(wire: WireRepo, secret: &str) -> Result<Repo> {
    let name = wire
        .full_name
        .parse::<RepoName>()
        .map_err(|_| Error::UnexpectedResponse {
            message: truncate(&format!("invalid repo name `{}`", wire.full_name), secret),
        })?;
    Ok(Repo {
        name,
        description: wire.description.filter(|d| !d.is_empty()),
        private: wire.private,
        fork: wire.fork,
        archived: wire.archived,
    })
}

/// `GET /user`.
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON or a missing `login`.
pub(crate) fn decode_user(body: &[u8], secret: &str) -> Result<User> {
    let wire: WireUser = parse(body, "the user", secret)?;
    Ok(User { login: wire.login })
}

/// `GET /repos/{owner}/{repo}/pulls/{number}`.
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field, a number of 0, an invalid timestamp,
/// an invalid `head.sha`, a `mergeable` that is not a boolean or null, or a `state` other than `open` / `closed`.
pub(crate) fn decode_pull_detail(body: &[u8], secret: &str) -> Result<PullDetail> {
    let wire: WirePullDetail = parse(body, "the pull request", secret)?;
    let unexpected = |message: String| Error::UnexpectedResponse {
        message: truncate(&message, secret),
    };
    let number = PrNumber::new(wire.number)
        .ok_or_else(|| unexpected("a pull request has the number 0".to_owned()))?;
    let state = pr_state(&wire.state, wire.draft, wire.merged_at.is_some())
        .ok_or_else(|| unexpected(format!("unknown pull request state `{}`", wire.state)))?;
    let head_sha = CommitSha::parse(&wire.head.sha)
        .ok_or_else(|| unexpected(format!("invalid head.sha `{}`", wire.head.sha)))?;
    Ok(PullDetail {
        number,
        title: wire.title,
        author: login_or_ghost(wire.user),
        state,
        body: wire.body.filter(|b| !b.is_empty()),
        head_label: wire.head.label,
        base_ref: wire.base.name,
        head_sha,
        archived: wire.base.repo.is_some_and(|repo| repo.archived),
        mergeable: match wire.mergeable {
            Some(true) => Mergeable::Yes,
            Some(false) => Mergeable::Conflicts,
            None => Mergeable::Unknown,
        },
        created_at: timestamp("created_at", &wire.created_at, secret)?,
        updated_at: timestamp("updated_at", &wire.updated_at, secret)?,
        additions: wire.additions,
        deletions: wire.deletions,
        changed_files: wire.changed_files,
    })
}

/// `GET .../pulls/{number}/reviews` (one page): `PENDING` reviews are dropped (they may lack `submitted_at`).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field, an unknown `state` or an invalid
/// `submitted_at`.
pub(crate) fn decode_reviews(body: &[u8], secret: &str) -> Result<Vec<Review>> {
    let wire: Vec<WireReview> = parse(body, "the review list", secret)?;
    let mut reviews = Vec::new();
    for review in wire {
        if let Some(review) = review_from_wire(review, secret)? {
            reviews.push(review);
        }
    }
    Ok(reviews)
}

/// `POST .../pulls/{number}/reviews`: the created review.
///
/// Errors: as [`decode_reviews`]; a `PENDING` review is also an [`Error::UnexpectedResponse`] (a submitted review
/// was asked for).
pub(crate) fn decode_review(body: &[u8], secret: &str) -> Result<Review> {
    let wire: WireReview = parse(body, "the review", secret)?;
    review_from_wire(wire, secret)?.ok_or_else(|| Error::UnexpectedResponse {
        message: "GitHub answered with a pending review".to_owned(),
    })
}

/// `GET /repos/{owner}/{repo}/issues/{number}/comments` (one page).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field or an invalid `created_at`.
pub(crate) fn decode_comments(body: &[u8], secret: &str) -> Result<Vec<Comment>> {
    let wire: Vec<WireComment> = parse(body, "the comment list", secret)?;
    wire.into_iter()
        .map(|comment| comment_from_wire(comment, secret))
        .collect()
}

/// `POST .../issues/{number}/comments`: the created Comment. Errors: as [`decode_comments`].
pub(crate) fn decode_comment(body: &[u8], secret: &str) -> Result<Comment> {
    let wire: WireComment = parse(body, "the comment", secret)?;
    comment_from_wire(wire, secret)
}

/// `GET .../pulls/{number}/files` (one page).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field or an unknown `status`.
pub(crate) fn decode_files(body: &[u8], secret: &str) -> Result<Vec<FileChange>> {
    let wire: Vec<WireFile> = parse(body, "the file list", secret)?;
    wire.into_iter()
        .map(|file| {
            let status = match file.status.as_str() {
                "added" => FileStatus::Added,
                "removed" => FileStatus::Removed,
                "modified" => FileStatus::Modified,
                "renamed" => FileStatus::Renamed,
                "copied" => FileStatus::Copied,
                "changed" => FileStatus::Changed,
                "unchanged" => FileStatus::Unchanged,
                other => {
                    return Err(Error::UnexpectedResponse {
                        message: truncate(&format!("unknown file status `{other}`"), secret),
                    });
                }
            };
            Ok(FileChange {
                path: file.filename,
                previous_path: file.previous_filename,
                status,
                additions: file.additions,
                deletions: file.deletions,
                patch: file.patch.filter(|p| !p.is_empty()),
            })
        })
        .collect()
}

/// JSON `body` into `T`; `what` names it in the error.
fn parse<T: serde::de::DeserializeOwned>(body: &[u8], what: &str, secret: &str) -> Result<T> {
    serde_json::from_slice(body).map_err(|e| Error::UnexpectedResponse {
        message: truncate(&format!("cannot decode {what}: {e}"), secret),
    })
}

fn timestamp(field: &str, text: &str, secret: &str) -> Result<Timestamp> {
    Timestamp::parse(text).ok_or_else(|| Error::UnexpectedResponse {
        message: truncate(&format!("invalid {field} `{text}`"), secret),
    })
}

fn login_or_ghost(user: Option<WireUser>) -> String {
    user.map_or_else(|| "ghost".to_owned(), |u| u.login)
}

/// The state mapping shared by the list and the detail (see [`PrState`]); `None` for an unknown `state`.
fn pr_state(state: &str, draft: bool, merged: bool) -> Option<PrState> {
    match (state, merged) {
        ("open", _) => Some(PrState::Open { draft }),
        ("closed", true) => Some(PrState::Merged),
        ("closed", false) => Some(PrState::Closed),
        _ => None,
    }
}

/// A pull request with its detail fields, as GitHub's JSON spells it (only the fields we use).
#[derive(Deserialize)]
struct WirePullDetail {
    number: u64,
    title: String,
    user: Option<WireUser>,
    state: String,
    #[serde(default)]
    draft: bool,
    merged_at: Option<String>,
    body: Option<String>,
    head: WireHead,
    base: WireBase,
    // Absent is read as null: GitHub is still computing it.
    #[serde(default)]
    mergeable: Option<bool>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changed_files: u64,
}

#[derive(Deserialize)]
struct WireHead {
    label: String,
    sha: String,
}

#[derive(Deserialize)]
struct WireBase {
    #[serde(rename = "ref")]
    name: String,
    // null for a deleted repo; absent is read the same.
    #[serde(default)]
    repo: Option<WireBaseRepo>,
}

#[derive(Deserialize)]
struct WireBaseRepo {
    #[serde(default)]
    archived: bool,
}

#[derive(Deserialize)]
struct WireReview {
    id: u64,
    user: Option<WireUser>,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    submitted_at: Option<String>,
}

/// `None` for a `PENDING` review.
fn review_from_wire(wire: WireReview, secret: &str) -> Result<Option<Review>> {
    let state = match wire.state.as_str() {
        "PENDING" => return Ok(None),
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "COMMENTED" => ReviewState::Commented,
        "DISMISSED" => ReviewState::Dismissed,
        other => {
            return Err(Error::UnexpectedResponse {
                message: truncate(&format!("unknown review state `{other}`"), secret),
            });
        }
    };
    let submitted_at = wire
        .submitted_at
        .map(|text| timestamp("submitted_at", &text, secret))
        .transpose()?;
    Ok(Some(Review {
        id: wire.id,
        author: login_or_ghost(wire.user),
        state,
        body: wire.body.filter(|b| !b.is_empty()),
        submitted_at,
    }))
}

#[derive(Deserialize)]
struct WireComment {
    id: u64,
    user: Option<WireUser>,
    #[serde(default)]
    body: Option<String>,
    created_at: String,
}

fn comment_from_wire(wire: WireComment, secret: &str) -> Result<Comment> {
    Ok(Comment {
        id: wire.id,
        author: login_or_ghost(wire.user),
        body: wire.body.unwrap_or_default(),
        created_at: timestamp("created_at", &wire.created_at, secret)?,
    })
}

#[derive(Deserialize)]
struct WireFile {
    filename: String,
    #[serde(default)]
    previous_filename: Option<String>,
    status: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    patch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        decode_comment, decode_comments, decode_files, decode_pull_detail, decode_pulls,
        decode_repo, decode_repos, decode_review, decode_reviews, decode_user,
    };
    use crate::{Error, FileStatus, Mergeable, PrState, ReviewState, Timestamp};

    // Example responses taken from the GitHub OpenAPI description (rest-api-description): "Get a repository"
    // (repository, minimal) and "List pull requests" (pull-request-simple items).
    const REPO_EXAMPLE: &str = include_str!("../tests/fixtures/repo.json");
    const PULLS_EXAMPLE: &str = include_str!("../tests/fixtures/pulls.json");

    fn pull(state_fields: &str) -> String {
        format!(
            r#"[{{"number":7,"title":"t","user":{{"login":"bob"}},"updated_at":"2026-03-04T05:06:07Z",{state_fields}}}]"#
        )
    }

    fn state_of(state_fields: &str) -> PrState {
        decode_pulls(pull(state_fields).as_bytes(), "").unwrap()[0].state
    }

    #[test]
    fn decodes_the_documented_repo_example() {
        let repo = decode_repo(REPO_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(repo.name.to_string(), "octocat/Hello-World");
        assert_eq!(repo.description.as_deref(), Some("This your first repo!"));
        assert!(!repo.private && !repo.fork && !repo.archived);
    }

    #[test]
    fn a_repo_body_that_is_a_list_or_garbage_is_unexpected() {
        for bad in [
            "",
            "[]",
            "not json",
            "{}",
            r#"{"full_name":"no-slash","private":false,"fork":false,"archived":false}"#,
        ] {
            assert!(
                matches!(
                    decode_repo(bad.as_bytes(), ""),
                    Err(Error::UnexpectedResponse { .. })
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn decodes_the_documented_pulls_example() {
        let pulls = decode_pulls(PULLS_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].number.get(), 1347);
        assert_eq!(pulls[0].title, "Amazing new feature");
        assert_eq!(pulls[0].author, "octocat");
        assert_eq!(pulls[0].state, PrState::Open { draft: false });
        assert_eq!(pulls[0].updated_at.as_str(), "2011-01-26T19:01:12Z");
    }

    #[test]
    fn pull_state_mapping() {
        assert_eq!(
            state_of(r#""state":"open","draft":false"#),
            PrState::Open { draft: false }
        );
        assert_eq!(
            state_of(r#""state":"open","draft":true"#),
            PrState::Open { draft: true }
        );
        assert_eq!(
            state_of(r#""state":"open""#),
            PrState::Open { draft: false }
        );
        assert_eq!(
            state_of(r#""state":"closed","merged_at":null"#),
            PrState::Closed
        );
        assert_eq!(state_of(r#""state":"closed""#), PrState::Closed);
        assert_eq!(
            state_of(r#""state":"closed","merged_at":"2026-03-04T05:06:07Z""#),
            PrState::Merged
        );
        // The draft flag of a closed or merged PR is dropped.
        assert_eq!(
            state_of(r#""state":"closed","draft":true,"merged_at":null"#),
            PrState::Closed
        );
        assert_eq!(
            state_of(r#""state":"closed","draft":true,"merged_at":"2026-03-04T05:06:07Z""#),
            PrState::Merged
        );
    }

    #[test]
    fn a_null_user_is_the_ghost() {
        let body = r#"[{"number":1,"title":"t","user":null,"state":"open","updated_at":"2026-03-04T05:06:07Z"}]"#;
        assert_eq!(
            decode_pulls(body.as_bytes(), "").unwrap()[0].author,
            "ghost"
        );
    }

    #[test]
    fn bad_pull_bodies_are_unexpected_responses() {
        let ok_tail = r#""title":"t","user":{"login":"b"},"state":"open","updated_at":"2026-03-04T05:06:07Z""#;
        for bad in [
            String::new(),
            "{}".into(),
            "not json".into(),
            format!(
                r#"[{{"number":1,{}}}]"#,
                ok_tail.replace(r#""state":"open""#, r#""state":"weird""#)
            ),
            format!(
                r#"[{{"number":1,{}}}]"#,
                ok_tail.replace(r#""state":"open""#, r#""state":"merged""#)
            ),
            format!(r#"[{{"number":0,{ok_tail}}}]"#),
            format!(r#"[{{{ok_tail}}}]"#),
            format!(
                r#"[{{"number":1,{}}}]"#,
                ok_tail.replace("2026-03-04T05:06:07Z", "yesterday")
            ),
            format!(
                r#"[{{"number":1,{}}}]"#,
                ok_tail.replace("2026-03-04T05:06:07Z", "2026-03-04T05:06:07.5Z")
            ),
            format!(
                r#"[{{"number":1,{}}}]"#,
                ok_tail.replace(r#""title":"t","#, "")
            ),
        ] {
            assert!(
                matches!(
                    decode_pulls(bad.as_bytes(), ""),
                    Err(Error::UnexpectedResponse { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(decode_pulls(b"[]", "").unwrap().is_empty());
    }

    #[test]
    fn pull_errors_are_masked_then_truncated() {
        let secret = "ghp_secret";
        let long = format!("{}{secret}{}", "a".repeat(10), "b".repeat(5000));
        let bodies = [
            format!(
                r#"[{{"number":1,"title":"t","user":null,"state":"{long}","updated_at":"2026-03-04T05:06:07Z"}}]"#
            ),
            format!(
                r#"[{{"number":1,"title":"t","user":null,"state":"open","updated_at":"{long}"}}]"#
            ),
            format!(
                r#"[{{"number":1,"title":"t","user":null,"state":"open","updated_at":"2026-03-04T05:06:07Z","draft":"{long}"}}]"#
            ),
        ];
        for body in bodies {
            match decode_pulls(body.as_bytes(), secret) {
                Err(Error::UnexpectedResponse { message }) => {
                    assert!(!message.contains("ghp_"), "{message}");
                    assert!(
                        message.chars().count() <= 300,
                        "{}",
                        message.chars().count()
                    );
                }
                other => panic!("unexpected {:?}", other.map(|r| r.len())),
            }
        }
    }

    // Example response of "List repositories for the authenticated user", taken from the GitHub OpenAPI description
    // (rest-api-description, components.examples.repository-items-default-response), the source of
    // https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user
    const DOCS_EXAMPLE: &str = include_str!("../tests/fixtures/user_repos.json");

    #[test]
    fn decodes_the_documented_example() {
        let repos = decode_repos(DOCS_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name.to_string(), "octocat/Hello-World");
        assert_eq!(
            repos[0].description.as_deref(),
            Some("This your first repo!")
        );
        assert!(!repos[0].private && !repos[0].fork && !repos[0].archived);
    }

    #[test]
    fn flags_are_carried_and_empty_or_null_description_is_none() {
        let body = r#"[
            {"full_name":"a/x","description":null,"private":true,"fork":false,"archived":false},
            {"full_name":"a/y","description":"","private":false,"fork":true,"archived":true},
            {"full_name":"a/z","private":false,"fork":false,"archived":false}
        ]"#;
        let repos = decode_repos(body.as_bytes(), "").unwrap();
        assert_eq!(repos.len(), 3);
        assert!(repos[0].private && repos[0].description.is_none());
        assert!(repos[1].fork && repos[1].archived && repos[1].description.is_none());
        assert!(repos[2].description.is_none());
    }

    #[test]
    fn network_texts_in_errors_are_truncated() {
        let long = "é".repeat(10_000);
        let bodies = [
            format!(
                r#"[{{"full_name":"a/x","description":null,"private":"{long}","fork":false,"archived":false}}]"#
            ),
            format!(r#"[{{"full_name":"{long}","private":false,"fork":false,"archived":false}}]"#),
        ];
        for body in bodies {
            match decode_repos(body.as_bytes(), "") {
                Err(Error::UnexpectedResponse { message }) => {
                    assert!(
                        message.chars().count() <= 300,
                        "{}",
                        message.chars().count()
                    );
                }
                other => panic!("unexpected {:?}", other.map(|r| r.len())),
            }
        }
    }

    #[test]
    fn the_secret_is_masked_before_the_cut() {
        let secret = "ghp_secret";
        // The message is "invalid repo name `<name>`": 19 characters before the name, so the secret starts 5 characters
        // before the 300-character limit; cutting first would keep "ghp_s".
        let name = format!("{}{secret}{}", "a".repeat(276), "b".repeat(50));
        let body =
            format!(r#"[{{"full_name":"{name}","private":false,"fork":false,"archived":false}}]"#);
        match decode_repos(body.as_bytes(), secret) {
            Err(Error::UnexpectedResponse { message }) => {
                assert!(!message.contains("ghp"), "{message}");
                assert_eq!(message.chars().count(), 300);
            }
            other => panic!("unexpected {:?}", other.map(|r| r.len())),
        }
    }

    #[test]
    fn an_empty_page_is_an_empty_list() {
        assert!(decode_repos(b"[]", "").unwrap().is_empty());
    }

    #[test]
    fn bad_bodies_are_unexpected_responses() {
        for bad in [
            "",
            "not json",
            "{}",
            r#"[{"description":"no name","private":false,"fork":false,"archived":false}]"#,
            r#"[{"full_name":"a/x","fork":false,"archived":false}]"#,
            r#"[{"full_name":"no-slash","private":false,"fork":false,"archived":false}]"#,
            r#"[{"full_name":"a/b/c","private":false,"fork":false,"archived":false}]"#,
        ] {
            assert!(
                matches!(
                    decode_repos(bad.as_bytes(), ""),
                    Err(Error::UnexpectedResponse { .. })
                ),
                "{bad:?}"
            );
        }
    }

    // ---- ADR 0003: detail, reviews, comments, files, user ----
    // Examples taken from the GitHub OpenAPI description (rest-api-description): "Get a pull request",
    // "List reviews for a pull request", "Create a review for a pull request", "List issue comments",
    // "Create an issue comment", "List pull requests files", "Get the authenticated user". Fixtures are copies of
    // components.examples: pull-request, pull-request-review-items, pull-request-review, issue-comment-items,
    // issue-comment, diff-entry-items, private-user-response-with-public-and-private-profile-information.
    const PULL_DETAIL_EXAMPLE: &str = include_str!("../tests/fixtures/pull.json");
    const REVIEWS_EXAMPLE: &str = include_str!("../tests/fixtures/reviews.json");
    const REVIEW_CREATED_EXAMPLE: &str = include_str!("../tests/fixtures/review_created.json");
    const COMMENTS_EXAMPLE: &str = include_str!("../tests/fixtures/issue_comments.json");
    const COMMENT_CREATED_EXAMPLE: &str = include_str!("../tests/fixtures/comment_created.json");
    const FILES_EXAMPLE: &str = include_str!("../tests/fixtures/files.json");
    const USER_EXAMPLE: &str = include_str!("../tests/fixtures/user.json");

    /// The documented object, edited by `edit`, as bytes.
    fn edited(example: &str, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_str(example).unwrap();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    fn detail(edit: impl FnOnce(&mut serde_json::Value)) -> crate::Result<crate::PullDetail> {
        decode_pull_detail(&edited(PULL_DETAIL_EXAMPLE, edit), "")
    }

    fn is_unexpected<T: std::fmt::Debug>(result: &crate::Result<T>) -> bool {
        matches!(result, Err(Error::UnexpectedResponse { .. }))
    }

    #[test]
    fn decodes_the_documented_user_example() {
        assert_eq!(
            decode_user(USER_EXAMPLE.as_bytes(), "").unwrap().login,
            "octocat"
        );
        for bad in ["", "[]", "{}", r#"{"login":null}"#, r#"{"login":3}"#] {
            assert!(is_unexpected(&decode_user(bad.as_bytes(), "")), "{bad:?}");
        }
    }

    #[test]
    fn decodes_the_documented_pull_example() {
        let pull = decode_pull_detail(PULL_DETAIL_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(pull.number.get(), 1347);
        assert_eq!(pull.title, "Amazing new feature");
        assert_eq!(pull.author, "octocat");
        // merged_at is set in the example but the state is open: open wins.
        assert_eq!(pull.state, PrState::Open { draft: false });
        assert_eq!(
            pull.body.as_deref(),
            Some("Please pull these awesome changes in!")
        );
        assert_eq!(pull.head_label, "octocat:new-topic");
        assert_eq!(pull.base_ref, "master");
        assert_eq!(
            pull.head_sha.as_str(),
            "6dcb09b5b57875f334f61aebed695e2e4193db5e"
        );
        assert!(!pull.archived);
        assert_eq!(pull.mergeable, Mergeable::Yes);
        assert_eq!(pull.created_at.as_str(), "2011-01-26T19:01:12Z");
        assert_eq!(pull.updated_at.as_str(), "2011-01-26T19:01:12Z");
        assert_eq!(
            (pull.additions, pull.deletions, pull.changed_files),
            (100, 3, 5)
        );
    }

    #[test]
    fn archived_is_read_from_base_repo() {
        let archived = |value: serde_json::Value| detail(|v| v["base"]["repo"]["archived"] = value);
        assert!(archived(true.into()).unwrap().archived);
        assert!(!archived(false.into()).unwrap().archived);
        // base.repo null, base.repo absent, archived absent: false (GitHub then answers 403 on publishing).
        assert!(
            !detail(|v| v["base"]["repo"] = serde_json::Value::Null)
                .unwrap()
                .archived
        );
        assert!(
            !detail(|v| {
                v["base"].as_object_mut().unwrap().remove("repo");
            })
            .unwrap()
            .archived
        );
        assert!(
            !detail(|v| {
                v["base"]["repo"]
                    .as_object_mut()
                    .unwrap()
                    .remove("archived");
            })
            .unwrap()
            .archived
        );
        // head.repo is not the base: its archived flag is ignored.
        assert!(
            !detail(|v| v["head"]["repo"]["archived"] = true.into())
                .unwrap()
                .archived
        );
    }

    #[test]
    fn mergeable_has_three_cases() {
        let mergeable =
            |value: serde_json::Value| detail(|v| v["mergeable"] = value).unwrap().mergeable;
        assert_eq!(mergeable(true.into()), Mergeable::Yes);
        assert_eq!(mergeable(false.into()), Mergeable::Conflicts);
        assert_eq!(mergeable(serde_json::Value::Null), Mergeable::Unknown);
        let absent = detail(|v| {
            v.as_object_mut().unwrap().remove("mergeable");
        })
        .unwrap();
        assert_eq!(absent.mergeable, Mergeable::Unknown);
        assert!(is_unexpected(&detail(|v| v["mergeable"] = "yes".into())));
    }

    #[test]
    fn pull_detail_optional_fields_and_state_mapping() {
        assert_eq!(
            detail(|v| v["user"] = serde_json::Value::Null)
                .unwrap()
                .author,
            "ghost"
        );
        assert_eq!(
            detail(|v| v["body"] = serde_json::Value::Null)
                .unwrap()
                .body,
            None
        );
        assert_eq!(detail(|v| v["body"] = "".into()).unwrap().body, None);
        assert_eq!(
            detail(|v| {
                v.as_object_mut().unwrap().remove("body");
            })
            .unwrap()
            .body,
            None
        );
        let counts = detail(|v| {
            for key in ["additions", "deletions", "changed_files"] {
                v.as_object_mut().unwrap().remove(key);
            }
        })
        .unwrap();
        assert_eq!(
            (counts.additions, counts.deletions, counts.changed_files),
            (0, 0, 0)
        );
        assert_eq!(
            detail(|v| v["draft"] = true.into()).unwrap().state,
            PrState::Open { draft: true }
        );
        let closed = |merged_at: serde_json::Value| {
            detail(|v| {
                v["state"] = "closed".into();
                v["draft"] = true.into();
                v["merged_at"] = merged_at;
            })
            .unwrap()
            .state
        };
        assert_eq!(closed(serde_json::Value::Null), PrState::Closed);
        assert_eq!(closed("2026-03-04T05:06:07Z".into()), PrState::Merged);
    }

    #[test]
    fn bad_pull_details_are_unexpected_responses() {
        type Edit = Box<dyn FnOnce(&mut serde_json::Value)>;
        let bad: Vec<(&str, Edit)> = vec![
            ("unknown state", Box::new(|v| v["state"] = "merged".into())),
            ("number 0", Box::new(|v| v["number"] = 0.into())),
            (
                "bad created_at",
                Box::new(|v| v["created_at"] = "yesterday".into()),
            ),
            (
                "bad updated_at",
                Box::new(|v| v["updated_at"] = "2026-03-04T05:06:07.5Z".into()),
            ),
            (
                "uppercase sha",
                Box::new(|v| v["head"]["sha"] = "6DCB09B5B57875F334F61AEBED695E2E4193DB5E".into()),
            ),
            (
                "short sha",
                Box::new(|v| v["head"]["sha"] = "6dcb09b".into()),
            ),
            (
                "no sha",
                Box::new(|v| {
                    v["head"].as_object_mut().unwrap().remove("sha");
                }),
            ),
            (
                "no head",
                Box::new(|v| {
                    v.as_object_mut().unwrap().remove("head");
                }),
            ),
            (
                "no base ref",
                Box::new(|v| {
                    v["base"].as_object_mut().unwrap().remove("ref");
                }),
            ),
            (
                "no title",
                Box::new(|v| {
                    v.as_object_mut().unwrap().remove("title");
                }),
            ),
            (
                "negative additions",
                Box::new(|v| v["additions"] = (-1).into()),
            ),
        ];
        for (what, edit) in bad {
            assert!(is_unexpected(&detail(edit)), "{what}");
        }
        for body in ["", "[]", "not json", "{}"] {
            assert!(
                is_unexpected(&decode_pull_detail(body.as_bytes(), "")),
                "{body:?}"
            );
        }
    }

    fn review(state: &str) -> serde_json::Value {
        serde_json::json!({"id": 9, "user": {"login": "carol"}, "body": "ok", "state": state,
            "submitted_at": "2026-03-04T05:06:07Z"})
    }

    #[test]
    fn decodes_the_documented_review_examples() {
        let reviews = decode_reviews(REVIEWS_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].id, 80);
        assert_eq!(reviews[0].author, "octocat");
        assert_eq!(reviews[0].state, ReviewState::Approved);
        assert_eq!(
            reviews[0].body.as_deref(),
            Some("Here is the body for the review.")
        );
        assert_eq!(
            reviews[0].submitted_at.as_ref().map(Timestamp::as_str),
            Some("2019-11-17T17:43:43Z")
        );

        let created = decode_review(REVIEW_CREATED_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(created.id, 80);
        assert_eq!(created.state, ReviewState::ChangesRequested);
        assert_eq!(
            created.body.as_deref(),
            Some("This is close to perfect! Please address the suggested inline change.")
        );
    }

    #[test]
    fn review_states_map_and_pending_is_dropped() {
        let list = serde_json::Value::Array(
            [
                "APPROVED",
                "PENDING",
                "CHANGES_REQUESTED",
                "COMMENTED",
                "DISMISSED",
            ]
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let mut r = review(state);
                r["id"] = (i as u64 + 1).into();
                r
            })
            .collect(),
        );
        let reviews = decode_reviews(&serde_json::to_vec(&list).unwrap(), "").unwrap();
        let got: Vec<(u64, ReviewState)> = reviews.iter().map(|r| (r.id, r.state)).collect();
        assert_eq!(
            got,
            [
                (1, ReviewState::Approved),
                (3, ReviewState::ChangesRequested),
                (4, ReviewState::Commented),
                (5, ReviewState::Dismissed),
            ]
        );
        // A Pending review has no submitted_at and may have a null user: still dropped, not an error.
        let pending = br#"[{"id":1,"user":null,"body":"","state":"PENDING"}]"#;
        assert!(decode_reviews(pending, "").unwrap().is_empty());
    }

    #[test]
    fn review_optional_fields_and_errors() {
        let one = |edit: fn(&mut serde_json::Value)| {
            let mut r = review("COMMENTED");
            edit(&mut r);
            decode_reviews(&serde_json::to_vec(&vec![r]).unwrap(), "")
        };
        assert_eq!(
            one(|r| r["user"] = serde_json::Value::Null).unwrap()[0].author,
            "ghost"
        );
        assert_eq!(one(|r| r["body"] = "".into()).unwrap()[0].body, None);
        assert_eq!(
            one(|r| r["body"] = serde_json::Value::Null).unwrap()[0].body,
            None
        );
        assert_eq!(
            one(|r| {
                r.as_object_mut().unwrap().remove("submitted_at");
            })
            .unwrap()[0]
                .submitted_at,
            None
        );
        assert!(is_unexpected(&one(|r| r["state"] = "WEIRD".into())));
        assert!(is_unexpected(&one(|r| r["state"] = "approved".into())));
        assert!(is_unexpected(&one(|r| r["submitted_at"] = "soon".into())));
        assert!(is_unexpected(&one(|r| {
            r.as_object_mut().unwrap().remove("id");
        })));
        assert!(is_unexpected(&decode_reviews(b"{}", "")));
        assert!(decode_reviews(b"[]", "").unwrap().is_empty());
    }

    #[test]
    fn a_created_review_that_is_pending_is_unexpected() {
        let pending = serde_json::to_vec(&review("PENDING")).unwrap();
        assert!(is_unexpected(&decode_review(&pending, "")));
        let approved = serde_json::to_vec(&review("APPROVED")).unwrap();
        assert_eq!(
            decode_review(&approved, "").unwrap().state,
            ReviewState::Approved
        );
        for bad in ["", "[]", "{}"] {
            assert!(is_unexpected(&decode_review(bad.as_bytes(), "")), "{bad:?}");
        }
    }

    #[test]
    fn decodes_the_documented_comment_examples() {
        let comments = decode_comments(COMMENTS_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, 1);
        assert_eq!(comments[0].author, "octocat");
        assert_eq!(comments[0].body, "Me too");
        assert_eq!(comments[0].created_at.as_str(), "2011-04-14T16:00:49Z");
        let created = decode_comment(COMMENT_CREATED_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!((created.id, created.body.as_str()), (1, "Me too"));
    }

    #[test]
    fn comment_optional_fields_and_errors() {
        let one = |edit: fn(&mut serde_json::Value)| {
            decode_comment(&edited(COMMENT_CREATED_EXAMPLE, edit), "")
        };
        assert_eq!(
            one(|c| c["user"] = serde_json::Value::Null).unwrap().author,
            "ghost"
        );
        assert_eq!(
            one(|c| c["body"] = serde_json::Value::Null).unwrap().body,
            ""
        );
        assert_eq!(
            one(|c| {
                c.as_object_mut().unwrap().remove("body");
            })
            .unwrap()
            .body,
            ""
        );
        assert!(is_unexpected(&one(|c| c["created_at"] = "x".into())));
        assert!(is_unexpected(&one(|c| {
            c.as_object_mut().unwrap().remove("id");
        })));
        assert!(is_unexpected(&decode_comments(b"{}", "")));
        assert!(decode_comments(b"[]", "").unwrap().is_empty());
    }

    #[test]
    fn decodes_the_documented_files_example() {
        let files = decode_files(FILES_EXAMPLE.as_bytes(), "").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "file1.txt");
        assert_eq!(files[0].previous_path, None);
        assert_eq!(files[0].status, FileStatus::Added);
        assert_eq!((files[0].additions, files[0].deletions), (103, 21));
        assert_eq!(
            files[0].patch.as_deref(),
            Some("@@ -132,7 +132,7 @@ module Test @@ -1000,7 +1000,7 @@ module Test")
        );
    }

    #[test]
    fn file_statuses_patch_and_rename() {
        let all = [
            ("added", FileStatus::Added),
            ("removed", FileStatus::Removed),
            ("modified", FileStatus::Modified),
            ("renamed", FileStatus::Renamed),
            ("copied", FileStatus::Copied),
            ("changed", FileStatus::Changed),
            ("unchanged", FileStatus::Unchanged),
        ];
        for (text, expected) in all {
            let body = edited(FILES_EXAMPLE, |v| v[0]["status"] = text.into());
            assert_eq!(
                decode_files(&body, "").unwrap()[0].status,
                expected,
                "{text}"
            );
        }
        let one = |edit: fn(&mut serde_json::Value)| decode_files(&edited(FILES_EXAMPLE, edit), "");
        assert_eq!(
            one(|v| {
                v[0].as_object_mut().unwrap().remove("patch");
            })
            .unwrap()[0]
                .patch,
            None
        );
        assert_eq!(one(|v| v[0]["patch"] = "".into()).unwrap()[0].patch, None);
        assert_eq!(
            one(|v| v[0]["patch"] = serde_json::Value::Null).unwrap()[0].patch,
            None
        );
        // Line ends are kept as sent.
        assert_eq!(
            one(|v| v[0]["patch"] = "@@ -1 +1 @@\r\n-a\r\n+b".into()).unwrap()[0]
                .patch
                .as_deref(),
            Some("@@ -1 +1 @@\r\n-a\r\n+b")
        );
        let renamed = one(|v| {
            v[0]["status"] = "renamed".into();
            v[0]["previous_filename"] = "old.txt".into();
        })
        .unwrap();
        assert_eq!(renamed[0].previous_path.as_deref(), Some("old.txt"));
        assert!(is_unexpected(&one(|v| v[0]["status"] = "weird".into())));
        assert!(is_unexpected(&one(|v| v[0]["status"] = "Added".into())));
        assert!(is_unexpected(&one(|v| {
            v[0].as_object_mut().unwrap().remove("filename");
        })));
        assert!(is_unexpected(&one(|v| {
            v[0].as_object_mut().unwrap().remove("status");
        })));
        assert!(is_unexpected(&decode_files(b"{}", "")));
        assert!(decode_files(b"[]", "").unwrap().is_empty());
    }

    #[test]
    fn new_decoders_mask_the_secret_then_cut() {
        let secret = "ghp_secret";
        let long = format!("{}{secret}{}", "a".repeat(10), "b".repeat(5000));
        let results = [
            decode_review(&serde_json::to_vec(&review(&long)).unwrap(), secret).map(|_| ()),
            decode_files(
                &edited(FILES_EXAMPLE, |v| v[0]["status"] = long.clone().into()),
                secret,
            )
            .map(|_| ()),
            decode_pull_detail(
                &edited(PULL_DETAIL_EXAMPLE, |v| v["state"] = long.clone().into()),
                secret,
            )
            .map(|_| ()),
            decode_comment(
                &edited(COMMENT_CREATED_EXAMPLE, |v| {
                    v["created_at"] = long.clone().into()
                }),
                secret,
            )
            .map(|_| ()),
        ];
        for result in results {
            match result {
                Err(Error::UnexpectedResponse { message }) => {
                    assert!(!message.contains("ghp_"), "{message}");
                    assert!(
                        message.chars().count() <= 300,
                        "{}",
                        message.chars().count()
                    );
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}
