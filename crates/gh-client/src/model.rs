//! Public model types, decoupled from GitHub's JSON (wire types stay private to the client).

use std::fmt;
use std::str::FromStr;

use crate::Error;

/// A repo identifier, `owner/name` (glossary: Repo).
///
/// Invariants (checked by [`RepoName::new`] and [`FromStr`]): `owner` is 1 to 39 ASCII letters, digits or
/// `-`, not starting or ending with `-`; `name` is 1 to 100 ASCII letters, digits, `.`, `_` or `-`, and is
/// neither `.` nor `..`. Case is kept as given (GitHub compares case-insensitively; equality here is exact).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoName {
    owner: String,
    name: String,
}

impl RepoName {
    /// Builds a repo name from its two parts. Errors: [`Error::InvalidRepoName`].
    pub fn new(owner: &str, name: &str) -> Result<Self, Error> {
        let owner_ok = (1..=39).contains(&owner.len())
            && owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !owner.starts_with('-')
            && !owner.ends_with('-');
        let name_ok = (1..=100).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            && name != "."
            && name != "..";
        if owner_ok && name_ok {
            Ok(Self {
                owner: owner.to_owned(),
                name: name.to_owned(),
            })
        } else {
            Err(Error::InvalidRepoName {
                input: format!("{owner}/{name}"),
            })
        }
    }

    /// The owner (user or organization login).
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The repo name, without its owner.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for RepoName {
    type Err = Error;

    /// Parses `owner/name` (surrounding whitespace ignored, exactly one `/`). Errors: [`Error::InvalidRepoName`]
    /// carrying the input as typed.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || Error::InvalidRepoName {
            input: s.to_owned(),
        };
        let (owner, name) = s.trim().split_once('/').ok_or_else(invalid)?;
        if name.contains('/') {
            return Err(invalid());
        }
        Self::new(owner, name).map_err(|_| invalid())
    }
}

impl fmt::Display for RepoName {
    /// `owner/name`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// How the signed-in user reaches a repo: the `affiliation` filter of `GET /user/repos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Affiliation {
    /// Repos the user owns.
    Owner,
    /// Repos where the user was added as a collaborator.
    Collaborator,
    /// Repos the user reaches through an organization membership.
    OrganizationMember,
}

/// A repo as listed by GitHub (glossary: Repo).
///
/// `#[non_exhaustive]`: later tickets add fields (default branch, push rights…); only this crate builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Repo {
    /// `owner/name`, from GitHub's `full_name`.
    pub name: RepoName,
    /// The repo description, `None` when GitHub sends `null` or an empty string.
    pub description: Option<String>,
    /// Private (or internal) repo.
    pub private: bool,
    /// The repo is a fork (glossary: forks are shown and marked).
    pub fork: bool,
    /// Archived repo: read-only (glossary: Archived repo).
    pub archived: bool,
}

#[cfg(test)]
mod tests {
    use super::RepoName;
    use crate::Error;

    fn invalid(owner: &str, name: &str) {
        assert!(
            matches!(
                RepoName::new(owner, name),
                Err(Error::InvalidRepoName { .. })
            ),
            "{owner:?}/{name:?} should be refused"
        );
    }

    #[test]
    fn parses_owner_and_name() {
        let repo: RepoName = "jerhome/gh-tui-rust".parse().unwrap();
        assert_eq!(repo.owner(), "jerhome");
        assert_eq!(repo.name(), "gh-tui-rust");
        assert_eq!(repo.to_string(), "jerhome/gh-tui-rust");
    }

    #[test]
    fn parse_ignores_surrounding_whitespace_and_keeps_case() {
        let repo: RepoName = "  Octo-Org/Hello.World_1\n".parse().unwrap();
        assert_eq!(repo.to_string(), "Octo-Org/Hello.World_1");
    }

    #[test]
    fn parse_refuses_a_missing_or_extra_slash_and_carries_the_input() {
        for bad in [
            "",
            "alice",
            "/repo",
            "alice/",
            "a/b/c",
            "alice /repo",
            "alice/re po",
        ] {
            assert_eq!(
                bad.parse::<RepoName>(),
                Err(Error::InvalidRepoName {
                    input: bad.to_owned()
                }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn owner_length_bounds_are_1_and_39() {
        assert!(RepoName::new(&"a".repeat(39), "r").is_ok());
        assert!(RepoName::new("a", "r").is_ok());
        invalid(&"a".repeat(40), "r");
        invalid("", "r");
    }

    #[test]
    fn name_length_bounds_are_1_and_100() {
        assert!(RepoName::new("o", &"n".repeat(100)).is_ok());
        assert!(RepoName::new("o", "n").is_ok());
        invalid("o", &"n".repeat(101));
        invalid("o", "");
    }

    #[test]
    fn owner_hyphen_rules() {
        assert!(RepoName::new("a-b", "r").is_ok());
        invalid("-ab", "r");
        invalid("ab-", "r");
        invalid("a_b", "r");
        invalid("a.b", "r");
        invalid("é", "r");
    }

    #[test]
    fn name_allows_dot_underscore_and_hyphen_but_not_dot_or_dotdot() {
        for ok in [".github", "a.b", "a_b", "-a-", "...", "a..", "A1"] {
            assert!(RepoName::new("o", ok).is_ok(), "{ok:?} should be accepted");
        }
        invalid("o", ".");
        invalid("o", "..");
        invalid("o", "a b");
        invalid("o", "a/b");
        invalid("o", "é");
    }
}
