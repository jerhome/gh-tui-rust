//! What `ghu` reads from its surroundings: environment variables and `gh auth token`.

/// Environment variable overriding the API base URL (default `gh_client::DEFAULT_API_BASE`).
pub const API_URL_VAR: &str = "GHU_API_URL";

/// The program's surroundings, as seen by [`crate::prepare`]. Point of variation: [`SystemEnvironment`] in the
/// binary, a map-backed double in tests (so tests never change the process environment, which is `unsafe` in
/// edition 2024 and racy between tests).
///
/// Called only from [`crate::prepare`], on the calling thread, before the TUI starts: may block (runs `gh`).
/// Later tickets add methods (config directory for pinned repos, editor command).
pub trait Environment {
    /// The value of an environment variable, `None` if unset or not valid Unicode.
    fn var(&self, name: &str) -> Option<String>;

    /// The standard output of `gh auth token`, or `None` if `gh` is not installed, fails, or prints nothing.
    /// Its stderr is discarded (never shown, never logged).
    fn gh_auth_token(&self) -> Option<String>;
}

/// The real environment: `std::env::var` and the `gh` program found on `PATH`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    /// Runs `gh auth token` with stdin closed and waits for it (no timeout: `gh` answers from its local config).
    fn gh_auth_token(&self) -> Option<String> {
        let output = std::process::Command::new("gh")
            .args(["auth", "token"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        (!text.trim().is_empty()).then_some(text)
    }
}
