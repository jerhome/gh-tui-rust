//! Private JSON types of GitHub's answers, and their conversion to the public model.

use serde::Deserialize;

use crate::status::truncate;
use crate::{Error, PrNumber, PrState, PullRequest, Repo, RepoName, Result, Timestamp};

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
    let state = match (wire.state.as_str(), wire.merged_at.is_some()) {
        ("open", _) => PrState::Open { draft: wire.draft },
        ("closed", true) => PrState::Merged,
        ("closed", false) => PrState::Closed,
        (other, _) => return Err(unexpected(format!("unknown pull request state `{other}`"))),
    };
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

#[cfg(test)]
mod tests {
    use super::{decode_pulls, decode_repo, decode_repos};
    use crate::{Error, PrState};

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
}
