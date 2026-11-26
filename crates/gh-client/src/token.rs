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
        todo!()
    }

    /// The secret value. Crate-private on purpose: only the HTTP layer may read it.
    #[allow(dead_code)]
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    /// Always `Token(***)`, whatever the value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
