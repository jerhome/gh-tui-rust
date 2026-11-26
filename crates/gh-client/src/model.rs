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
        todo!()
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
        todo!()
    }
}

impl fmt::Display for RepoName {
    /// `owner/name`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
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
