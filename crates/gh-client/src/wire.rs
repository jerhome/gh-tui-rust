//! Private JSON types of GitHub's answers, and their conversion to the public model.

use serde::Deserialize;

use crate::{Error, Repo, RepoName, Result};

/// Decodes the body of `GET /user/repos` (one page).
///
/// Errors: [`Error::UnexpectedResponse`] for invalid JSON, a missing field or an invalid `full_name`.
pub(crate) fn decode_repos(body: &[u8]) -> Result<Vec<Repo>> {
    let wire: Vec<WireRepo> =
        serde_json::from_slice(body).map_err(|e| Error::UnexpectedResponse {
            message: format!("cannot decode the repo list: {e}"),
        })?;
    wire.into_iter().map(Repo::try_from).collect()
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

impl TryFrom<WireRepo> for Repo {
    type Error = Error;

    fn try_from(wire: WireRepo) -> Result<Self> {
        let name = wire
            .full_name
            .parse::<RepoName>()
            .map_err(|_| Error::UnexpectedResponse {
                message: format!("invalid repo name `{}`", wire.full_name),
            })?;
        Ok(Repo {
            name,
            description: wire.description.filter(|d| !d.is_empty()),
            private: wire.private,
            fork: wire.fork,
            archived: wire.archived,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::decode_repos;
    use crate::Error;

    // Example response of "List repositories for the authenticated user", taken from the GitHub OpenAPI description
    // (rest-api-description, components.examples.repository-items-default-response), the source of
    // https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user
    const DOCS_EXAMPLE: &str = include_str!("../tests/fixtures/user_repos.json");

    #[test]
    fn decodes_the_documented_example() {
        let repos = decode_repos(DOCS_EXAMPLE.as_bytes()).unwrap();
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
        let repos = decode_repos(body.as_bytes()).unwrap();
        assert_eq!(repos.len(), 3);
        assert!(repos[0].private && repos[0].description.is_none());
        assert!(repos[1].fork && repos[1].archived && repos[1].description.is_none());
        assert!(repos[2].description.is_none());
    }

    #[test]
    fn an_empty_page_is_an_empty_list() {
        assert!(decode_repos(b"[]").unwrap().is_empty());
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
                    decode_repos(bad.as_bytes()),
                    Err(Error::UnexpectedResponse { .. })
                ),
                "{bad:?}"
            );
        }
    }
}
