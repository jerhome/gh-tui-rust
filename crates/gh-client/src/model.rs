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

    /// `true` if both name the same GitHub repo: owner and name compared ASCII case-insensitively, as GitHub
    /// does (`Alice/Notes` and `alice/notes` are the same repo). Use it to deduplicate; `==` stays exact.
    pub fn same_repo(&self, other: &RepoName) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.name.eq_ignore_ascii_case(&other.name)
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

/// The number of a pull request within its repo (GitHub's `number`, shown as `#12`). Never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrNumber(u64);

impl PrNumber {
    /// `None` for `0` (GitHub numbers start at 1).
    pub const fn new(number: u64) -> Option<Self> {
        if number == 0 {
            None
        } else {
            Some(Self(number))
        }
    }

    /// The number as GitHub sends it.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PrNumber {
    /// `#<number>`, for example `#12`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An instant as GitHub's REST API writes it: `YYYY-MM-DDTHH:MM:SSZ` (UTC, no fraction, no offset).
///
/// Invariant (checked by [`Timestamp::parse`]): exactly 20 ASCII characters in that shape, digits where digits are
/// expected, month 01-12, day 01-31, hour 00-23, minute and second 00-59 (calendar validity of the day, such as
/// February 30, is not checked). Ordering is chronological (lexicographic on that fixed shape). No time zone
/// conversion: ghu shows UTC dates (ADR 0002).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(String);

impl Timestamp {
    /// Parses GitHub's `YYYY-MM-DDTHH:MM:SSZ`; `None` for anything else (offsets, fractions, lowercase `z`,
    /// surrounding spaces). Pure, never panics.
    pub fn parse(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        // Digit positions and separators of `YYYY-MM-DDTHH:MM:SSZ`; checking bytes also rules out non-ASCII text.
        let shape_ok = b.len() == 20
            && b.iter().enumerate().all(|(i, &c)| match i {
                4 | 7 => c == b'-',
                10 => c == b'T',
                13 | 16 => c == b':',
                19 => c == b'Z',
                _ => c.is_ascii_digit(),
            });
        if !shape_ok {
            return None;
        }
        let field = |at: usize| u32::from(b[at] - b'0') * 10 + u32::from(b[at + 1] - b'0');
        let in_range = (1..=12).contains(&field(5))
            && (1..=31).contains(&field(8))
            && field(11) <= 23
            && field(14) <= 59
            && field(17) <= 59;
        in_range.then(|| Self(text.to_owned()))
    }

    /// The text as GitHub sent it, `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The UTC date part, `YYYY-MM-DD` (what the PR list shows).
    pub fn date(&self) -> &str {
        &self.0[..10]
    }
}

impl fmt::Display for Timestamp {
    /// Same as [`Timestamp::as_str`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The PR state (glossary: PR state). A Draft PR is an open PR whose author marked it as not ready.
///
/// Mapping from GitHub's JSON: `state: "open"` → [`PrState::Open`] with `draft` from the `draft` field
/// (`false` when absent); `state: "closed"` with a non-null `merged_at` → [`PrState::Merged`]; `state: "closed"`
/// otherwise → [`PrState::Closed`]. The `draft` flag of a closed or merged PR is dropped. Any other `state` is an
/// [`Error::UnexpectedResponse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrState {
    /// Open; `draft` marks a Draft PR.
    Open {
        /// The author marked it as not ready for review.
        draft: bool,
    },
    /// Closed without being merged.
    Closed,
    /// Merged (GitHub reports it as closed with a merge date).
    Merged,
}

impl PrState {
    /// `true` for [`PrState::Open`], draft or not (only an open PR can be reviewed or commented on).
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open { .. })
    }
}

/// Which PRs [`crate::Client::pulls`] asks for: GitHub's `state` query of `GET /repos/{owner}/{repo}/pulls`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PullFilter {
    /// `state=open`: open PRs, drafts included.
    Open,
    /// `state=closed`: closed and merged PRs (GitHub returns both for `closed`).
    Closed,
}

/// A pull request as listed by GitHub (glossary: Pull request).
///
/// `#[non_exhaustive]`: the detail screen (#6) adds fields (branches, body…); only this crate builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PullRequest {
    /// Number within the repo.
    pub number: PrNumber,
    /// Title, as GitHub sends it (may be empty).
    pub title: String,
    /// Login of the author (`user.login`); `ghost` when GitHub sends `user: null` (deleted account).
    pub author: String,
    /// Open (draft or not), closed or merged.
    pub state: PrState,
    /// Last update (`updated_at`).
    pub updated_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::{RepoName, Timestamp};
    use crate::Error;

    #[test]
    fn timestamp_accepts_the_documented_shape_and_exposes_the_date() {
        let t = Timestamp::parse("2011-01-26T19:01:12Z").expect("valid");
        assert_eq!(t.as_str(), "2011-01-26T19:01:12Z");
        assert_eq!(t.to_string(), "2011-01-26T19:01:12Z");
        assert_eq!(t.date(), "2011-01-26");
    }

    #[test]
    fn timestamp_bounds_of_each_field() {
        for ok in [
            "0000-01-01T00:00:00Z",
            "9999-12-31T23:59:59Z",
            "2026-02-30T00:00:00Z", // calendar validity of the day is not checked
        ] {
            assert!(Timestamp::parse(ok).is_some(), "{ok} should be accepted");
        }
        for bad in [
            "2026-00-01T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-01-00T00:00:00Z",
            "2026-01-32T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:60Z",
        ] {
            assert!(Timestamp::parse(bad).is_none(), "{bad} should be refused");
        }
    }

    #[test]
    fn timestamp_refuses_offsets_fractions_lowercase_z_and_spaces() {
        for bad in [
            "2026-01-01T00:00:00+02:00",
            "2026-01-01T00:00:00.123Z",
            "2026-01-01T00:00:00z",
            "2026-01-01t00:00:00Z",
            "2026-01-01 00:00:00Z",
            " 2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z ",
            "2026-01-01T00:00:00Z\n",
            "2026-01-01T00:00:00",
            "2026-01-01",
            "",
            "2026/01/01T00:00:00Z",
            "2026-01-01T00-00-00Z",
            "+026-01-01T00:00:00Z",
            "2026-0a-01T00:00:00Z",
            "２０２６-01-01T00:00:00Z",
        ] {
            assert!(Timestamp::parse(bad).is_none(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn timestamp_never_panics_and_orders_chronologically() {
        // Multi-byte characters must not cause a slicing panic.
        for text in [
            "é".repeat(20),
            "é".repeat(10),
            "2026-01-01T00:00:0éZ".to_owned(),
        ] {
            assert!(Timestamp::parse(&text).is_none());
        }
        let early = Timestamp::parse("2025-12-31T23:59:59Z").unwrap();
        let late = Timestamp::parse("2026-01-01T00:00:00Z").unwrap();
        assert!(early < late);
    }

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
