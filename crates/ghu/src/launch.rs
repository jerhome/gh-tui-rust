//! Start-up: arguments, token, API URL. No terminal, no network.

use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use gh_client::{ApiBase, Token};

use crate::editor::EditorVars;
use crate::pins::{PinStore, PinsError};
use crate::{API_URL_VAR, Environment};

/// Token variables, in priority order.
const TOKEN_VARS: [&str; 2] = ["GH_TOKEN", "GITHUB_TOKEN"];

/// Command line of `ghu`. No option of its own yet: `--help` and `--version` come from clap.
#[derive(Debug, Parser)]
#[command(
    name = "ghu",
    version,
    about = "Terminal client for GitHub: browse repos and pull requests, and review them."
)]
struct Cli {}

/// What to do after [`prepare`].
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Built once at start-up and moved at once: boxing would only add noise.
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
/// Owns the GitHub client (base URL + token) and the Pinned repos store. `Send`, moved into [`crate::run`] or
/// [`crate::Ui::new`]. Its `Debug` never shows the token (nor the editor variables).
pub struct Context {
    pub(crate) client: gh_client::Client,
    /// Loaded by [`prepare`] from [`Environment::config_dir`] (ADR 0002).
    pub(crate) pins: PinStore,
    /// Why the Pinned repos file could not be read, shown by the Repo list as its first notice.
    pub(crate) pins_problem: Option<PinsError>,
    /// `VISUAL` and `EDITOR` as read by [`prepare`] (raw; unset or absent: `None`), for the external editor (#9).
    pub(crate) editor: EditorVars,
}

impl std::fmt::Debug for Context {
    /// `Context { api: "<base URL>", .. }`: the token is never shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("api", &self.client.base().as_str())
            .finish_non_exhaustive()
    }
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
/// - Pinned repos: loaded with `PinStore::load(env.config_dir())` (never ends the program: a bad file is reported in
///   the TUI and pinning is disabled for the session). Only once the token and URL are valid.
/// - Editor (#9): `VISUAL` and `EDITOR` read with [`Environment::var`] and kept raw (parsed only when an edit runs,
///   so a bad value never ends the program).
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
    // Write errors on `out`/`err` are ignored on purpose (documented): there is nowhere else to report them.
    if let Err(error) = Cli::try_parse_from(args) {
        let code = error.exit_code();
        let text = error.render().to_string();
        let stream: &mut dyn Write = if error.use_stderr() { err } else { out };
        let _ = stream.write_all(text.as_bytes());
        return Launch::Exit(u8::try_from(code).unwrap_or(2));
    }

    let Some(token) = find_token(env) else {
        let _ = writeln!(
            err,
            "ghu: no GitHub token found.\n\
             Set GH_TOKEN or GITHUB_TOKEN, or sign in with the GitHub CLI: gh auth login"
        );
        return Launch::Exit(1);
    };

    let base = match env
        .var(API_URL_VAR)
        .filter(|value| !value.trim().is_empty())
    {
        None => ApiBase::default(),
        Some(value) => match ApiBase::parse(&value) {
            Ok(base) => base,
            Err(error) => {
                let reason = match error {
                    gh_client::Error::InvalidBaseUrl { reason } => reason,
                    other => other.to_string(),
                };
                let _ = writeln!(err, "ghu: invalid {API_URL_VAR}: {reason}");
                return Launch::Exit(1);
            }
        },
    };
    tracing::debug!(api = base.as_str(), "start-up settings resolved");

    match gh_client::Client::new(base, token) {
        Ok(client) => {
            let (pins, pins_problem) = PinStore::load(env.config_dir().as_deref());
            Launch::Tui(Context {
                client,
                pins,
                pins_problem,
                editor: EditorVars {
                    visual: env.var("VISUAL"),
                    editor: env.var("EDITOR"),
                },
            })
        }
        Err(error) => {
            let _ = writeln!(err, "ghu: {error}");
            Launch::Exit(1)
        }
    }
}

/// The first non-empty of `GH_TOKEN`, `GITHUB_TOKEN`, `gh auth token`; `gh` runs only if both variables are
/// unset or empty.
fn find_token(env: &dyn Environment) -> Option<Token> {
    for name in TOKEN_VARS {
        if let Some(token) = env.var(name).and_then(Token::new) {
            tracing::debug!(source = name, "token found");
            return Some(token);
        }
    }
    let token = env.gh_auth_token().and_then(Token::new);
    if token.is_some() {
        tracing::debug!(source = "gh auth token", "token found");
    }
    token
}
