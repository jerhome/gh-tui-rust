//! Start-up: arguments, token, API URL. No terminal, no network.

use std::ffi::OsString;
use std::io::Write;

use crate::Environment;

/// What to do after [`prepare`].
#[derive(Debug)]
pub enum Launch {
    /// End the program with this exit code; the text is already written. Codes: `0` for `--help` and
    /// `--version`, `1` for a start-up error (no token, invalid `GHU_API_URL`, HTTP client setup), `2` for a
    /// usage error (unknown option…).
    Exit(u8),
    /// Start the TUI with this context.
    Tui(Context),
}

/// Everything the TUI needs, resolved at start-up. Opaque: only [`prepare`] builds it.
///
/// Owns the GitHub client (base URL + token). `Send`, moved into [`crate::run`] or [`crate::Ui::new`]. Its
/// `Debug` never shows the token. Later tickets add fields (pinned-repo file, editor command).
#[derive(Debug)]
pub struct Context {
    #[allow(dead_code)]
    pub(crate) client: gh_client::Client,
}

/// Parses `args` (program name first, as `std::env::args_os`) and resolves the start-up settings.
///
/// - `--help` / `--version` (clap, binary name `ghu`): text on `out`, [`Launch::Exit(0)`]. Usage errors: clap's
///   text on `err`, [`Launch::Exit(2)`].
/// - Token: the first non-empty of `GH_TOKEN`, `GITHUB_TOKEN`, [`Environment::gh_auth_token`]
///   (see [`gh_client::Token::new`]); `gh` is not run when a variable is set. None: a message on `err` that
///   names `GH_TOKEN`, `GITHUB_TOKEN` and `gh auth login`, then [`Launch::Exit(1)`].
/// - API URL: [`crate::API_URL_VAR`] if set and non-empty, else the default; invalid: a message on `err` naming the
///   variable and the reason, then [`Launch::Exit(1)`].
/// - The token never appears in `out`, `err` or logs. No network access here (a refused token shows in the TUI).
///
/// Write errors on `out`/`err` are ignored. Blocking (may run `gh`); call before starting the TUI.
pub fn prepare<I, T>(
    args: I,
    env: &dyn Environment,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Launch
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    todo!()
}
