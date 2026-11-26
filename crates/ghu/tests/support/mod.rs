//! Test harness: the single test point of the spec (#1). Keys in, screen out, against `gh-fake` in real HTTP.
//!
//! Each test file starts with `mod support;`. Each test runs on `#[tokio::test]` (current-thread runtime), starts
//! its own fake server on a free loopback port, and never reaches the real GitHub: [`TestEnv`] always sets
//! `GHU_API_URL` to the fake server unless a test overrides it on purpose.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gh_fake::{FakeGitHub, State};
use ghu::{Environment, KeyCode, KeyModifiers, Launch, Ui};
use ratatui::backend::TestBackend;

/// Screen size of every harness (columns, rows).
pub const SIZE: (u16, u16) = (100, 30);

/// Longest wait of [`Harness::wait_for`] before failing.
pub const WAIT: Duration = Duration::from_secs(5);

/// A map-backed [`Environment`].
#[derive(Clone, Debug, Default)]
pub struct TestEnv {
    /// Environment variables.
    pub vars: HashMap<String, String>,
    /// What `gh auth token` prints (`None`: `gh` missing or not signed in).
    pub gh_auth_token: Option<String>,
}

impl TestEnv {
    /// `GHU_API_URL` = the fake's URL and `GH_TOKEN` = the fake's accepted token.
    pub fn for_fake(fake: &FakeGitHub) -> Self {
        todo!()
    }
}

impl Environment for TestEnv {
    fn var(&self, name: &str) -> Option<String> {
        todo!()
    }

    fn gh_auth_token(&self) -> Option<String> {
        todo!()
    }
}

/// Runs `ghu::prepare` with `args` (program name not included) and returns its result with what it wrote on stdout
/// and stderr.
pub fn launch(args: &[&str], env: &TestEnv) -> (Launch, String, String) {
    todo!()
}

/// Logs captured during a test (all `tracing` events at TRACE level, from every crate, on the current thread).
pub struct LogCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

impl LogCapture {
    /// Installs the capturing subscriber for the current thread until the value is dropped.
    pub fn start() -> Self {
        todo!()
    }

    /// Everything logged so far.
    pub fn text(&self) -> String {
        todo!()
    }
}

/// The application on a `TestBackend`, with its fake server.
pub struct Harness {
    /// The fake GitHub; tests change its state or read its recorded requests.
    pub fake: FakeGitHub,
    ui: Ui<TestBackend>,
}

impl Harness {
    /// Starts the fake with `state`, then the application with [`TestEnv::for_fake`] and no arguments.
    pub async fn start(state: State) -> Self {
        todo!()
    }

    /// Like [`Harness::start`], letting `setup` change the environment first. Panics if `prepare` does not
    /// return `Launch::Tui`.
    pub async fn start_with(state: State, setup: impl FnOnce(&mut TestEnv, &FakeGitHub)) -> Self {
        todo!()
    }

    /// Sends a key press without modifiers.
    pub fn press(&mut self, code: KeyCode) {
        todo!()
    }

    /// Sends a key press with modifiers (`Ctrl-C`…).
    pub fn press_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        todo!()
    }

    /// Sends each character of `text` as a key press.
    pub fn type_text(&mut self, text: &str) {
        todo!()
    }

    /// Draws, then returns the screen: one line per row, trailing spaces trimmed, joined with `\n`.
    pub fn screen(&mut self) -> String {
        todo!()
    }

    /// Applies background results until the screen contains `needle`; panics after [`WAIT`] with the screen.
    pub async fn wait_for(&mut self, needle: &str) {
        todo!()
    }

    /// Applies background results until `done(screen)` is true; panics after [`WAIT`] with `what` and the screen.
    pub async fn wait_until(&mut self, what: &str, done: impl Fn(&str) -> bool) {
        todo!()
    }

    /// Applies background results until no request is in flight; panics after [`WAIT`].
    pub async fn settle(&mut self) {
        todo!()
    }

    /// `true` once the application quit.
    pub fn is_finished(&self) -> bool {
        todo!()
    }
}
