//! The GitHub token, as a secret value.

use std::fmt;

/// A GitHub token (personal access token, or the one `gh auth token` prints).
///
/// Invariants: never empty, no leading or trailing whitespace. Its value can only be read inside this crate, to
/// build the `Authorization: Bearer …` header. `Debug` prints `Token(***)`; there is no `Display`, no
/// `Serialize` and no accessor outside the crate, so the value cannot leak into the UI, logs or errors.
/// `Send + Sync`, cheap to clone.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    /// Builds a token from raw text (an environment variable, the output of `gh auth token`).
    ///
    /// Trims surrounding whitespace (including the trailing newline `gh` prints). Returns `None` if nothing is
    /// left, so an empty `GH_TOKEN=` counts as unset.
    pub fn new(raw: impl Into<String>) -> Option<Self> {
        let raw: String = raw.into();
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| Self(trimmed.to_owned()))
    }

    /// The secret value. Crate-private on purpose: only the HTTP layer may read it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    /// Always `Token(***)`, whatever the value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::Token;

    #[test]
    fn new_trims_surrounding_whitespace_and_newline() {
        let token = Token::new("  ghp_abc123\n").expect("non-empty token");
        assert_eq!(token.expose(), "ghp_abc123");
    }

    #[test]
    fn new_keeps_a_plain_value_unchanged() {
        assert_eq!(
            Token::new("ghp_abc123").expect("token").expose(),
            "ghp_abc123"
        );
    }

    #[test]
    fn new_refuses_empty_and_blank_input() {
        assert!(Token::new("").is_none());
        assert!(Token::new("   \t\r\n").is_none());
    }

    #[test]
    fn debug_never_shows_the_value() {
        let token = Token::new("ghp_supersecret").expect("token");
        assert_eq!(format!("{token:?}"), "Token(***)");
        assert_eq!(format!("{token:#?}"), "Token(***)");
    }
}
