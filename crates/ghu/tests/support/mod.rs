//! Test harness: the single test point of the spec (#1). Keys in, screen out, against `gh-fake` in real HTTP.
//!
//! Each test file starts with `mod support;`. Each test runs on `#[tokio::test]` (current-thread runtime), starts
//! its own fake server on a free loopback port, and never reaches the real GitHub: [`TestEnv`] always sets
//! `GHU_API_URL` to the fake server unless a test overrides it on purpose.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gh_fake::{FakeGitHub, State};
use ghu::{Environment, Input, KeyCode, KeyEvent, KeyModifiers, Launch, Ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Screen size of every harness (columns, rows).
pub const SIZE: (u16, u16) = (100, 30);

/// Longest wait of [`Harness::wait_for`] before failing.
pub const WAIT: Duration = Duration::from_secs(5);

/// `EDITOR` of every [`TestEnv::for_fake`]: a program that does not exist.
pub const NO_EDITOR: &str = "/nonexistent/ghu-editor";

/// A map-backed [`Environment`].
#[derive(Clone, Debug, Default)]
pub struct TestEnv {
    /// Environment variables.
    pub vars: HashMap<String, String>,
    /// What `gh auth token` prints (`None`: `gh` missing or not signed in).
    pub gh_auth_token: Option<String>,
    /// How many times `gh auth token` was run (shared between clones).
    pub gh_runs: Arc<AtomicUsize>,
    /// What [`Environment::config_dir`] returns. `None` by default (pins in memory only), so a test never touches
    /// the real configuration directory; [`Harness`] sets a temporary directory.
    pub config_dir: Option<PathBuf>,
}

impl TestEnv {
    /// `GHU_API_URL` = the fake's URL, `GH_TOKEN` = the fake's accepted token, `EDITOR` = [`NO_EDITOR`] (a test
    /// that presses `Ctrl-E` without choosing an editor never starts the real `vi` or `notepad`).
    pub fn for_fake(fake: &FakeGitHub) -> Self {
        let token = fake.with_state(|s| s.token.clone());
        let mut env = Self::default();
        env.set(ghu::API_URL_VAR, &fake.base_url());
        env.set("GH_TOKEN", &token);
        env.set("EDITOR", NO_EDITOR);
        env
    }

    /// Sets a variable.
    pub fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(name.to_owned(), value.to_owned());
    }

    /// Removes a variable.
    pub fn unset(&mut self, name: &str) {
        self.vars.remove(name);
    }
}

impl Environment for TestEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn gh_auth_token(&self) -> Option<String> {
        self.gh_runs.fetch_add(1, Ordering::SeqCst);
        self.gh_auth_token.clone()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.config_dir.clone()
    }
}

/// Runs `ghu::prepare` with `args` (program name not included) and returns its result with what it wrote on stdout
/// and stderr.
pub fn launch(args: &[&str], env: &TestEnv) -> (Launch, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let argv = std::iter::once("ghu").chain(args.iter().copied());
    let launch = ghu::prepare(argv, env, &mut out, &mut err);
    (
        launch,
        String::from_utf8(out).expect("stdout is UTF-8"),
        String::from_utf8(err).expect("stderr is UTF-8"),
    )
}

/// Logs captured during a test (all `tracing` events at TRACE level, from every crate, on the current thread).
pub struct LogCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

/// A writer appending to the shared buffer of a [`LogCapture`].
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogCapture {
    /// Installs the capturing subscriber for the current thread until the value is dropped.
    pub fn start() -> Self {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buffer);
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || SharedWriter(Arc::clone(&writer)))
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        Self {
            buffer,
            _guard: guard,
        }
    }

    /// Everything logged so far.
    pub fn text(&self) -> String {
        let bytes = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// The application on a `TestBackend`, with its fake server and its own temporary configuration directory.
pub struct Harness {
    /// The fake GitHub; tests change its state or read its recorded requests.
    pub fake: FakeGitHub,
    ui: Ui<TestBackend>,
    env: TestEnv,
    /// Deleted when the harness is dropped.
    config: tempfile::TempDir,
    /// `ReturnedAtOnce` threshold given to each editor request (`EditRequest::with_returned_at_once`).
    edit_threshold: Duration,
}

impl Harness {
    /// Starts the fake with `state`, then the application with [`TestEnv::for_fake`], a fresh temporary
    /// configuration directory and no arguments.
    pub async fn start(state: State) -> Self {
        Self::start_with(state, |_, _| {}).await
    }

    /// Like [`Harness::start`], letting `setup` change the environment first (`env.config_dir` is already the
    /// temporary directory; a test may pre-write its Pinned repos file with [`ghu::pinned_repos_path`], or set
    /// `None`). Panics if `prepare` does not return `Launch::Tui`.
    pub async fn start_with(state: State, setup: impl FnOnce(&mut TestEnv, &FakeGitHub)) -> Self {
        let fake = FakeGitHub::start(state)
            .await
            .expect("start the fake server");
        let config = tempfile::tempdir().expect("temporary config directory");
        let mut env = TestEnv::for_fake(&fake);
        env.config_dir = Some(config.path().to_path_buf());
        setup(&mut env, &fake);
        let ui = new_ui(&env);
        Self {
            fake,
            ui,
            env,
            config,
            edit_threshold: ghu::RETURNED_AT_ONCE,
        }
    }

    /// Quits and starts the application again with the same environment, configuration directory and fake server
    /// (whose state and recorded requests are kept): a new session, for persistence tests.
    pub fn restart(&mut self) {
        self.ui = new_ui(&self.env);
    }

    /// The temporary configuration directory (what `Environment::config_dir` returns unless `setup` changed it).
    pub fn config_dir(&self) -> &Path {
        self.config.path()
    }

    /// The Pinned repos file of this harness: `ghu::pinned_repos_path(self.config_dir())`.
    pub fn pinned_file(&self) -> PathBuf {
        ghu::pinned_repos_path(self.config_dir())
    }

    /// Sends a pasted block of text (bracketed paste).
    pub fn paste(&mut self, text: &str) {
        self.ui.input(Input::Paste(text.to_owned()));
    }

    /// Sends a key press without modifiers.
    pub fn press(&mut self, code: KeyCode) {
        self.press_with(code, KeyModifiers::NONE);
    }

    /// Sends a key press with modifiers (`Ctrl-C`…). If the key asked for the editor (`Ctrl-E`), runs it at once
    /// (blocking, no terminal to suspend) and gives its result back, as the real loop does.
    pub fn press_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.ui.input(Input::Key(KeyEvent::new(code, modifiers)));
        if let Some(request) = self.ui.take_edit() {
            let result = request.with_returned_at_once(self.edit_threshold).run();
            self.ui.finish_edit(result);
        }
    }

    /// Sets the `ReturnedAtOnce` threshold of the next editor runs (default `ghu::RETURNED_AT_ONCE`;
    /// `Duration::ZERO` disables the check).
    pub fn set_edit_threshold(&mut self, threshold: Duration) {
        self.edit_threshold = threshold;
    }

    /// Sends each character of `text` as a key press.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    /// Draws, then returns the screen: one line per row, trailing spaces trimmed, joined with `\n`.
    pub fn screen(&mut self) -> String {
        self.ui.draw().expect("TestBackend never fails");
        let buffer = self.ui.terminal().backend().buffer();
        let area = buffer.area;
        let mut lines = Vec::with_capacity(usize::from(area.height));
        for y in area.top()..area.bottom() {
            let mut line = String::new();
            for x in area.left()..area.right() {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    /// Applies background results until the screen contains `needle`; panics after [`WAIT`] with the screen.
    pub async fn wait_for(&mut self, needle: &str) {
        self.wait_until(&format!("{needle:?} on screen"), |screen| {
            screen.contains(needle)
        })
        .await;
    }

    /// Applies background results until `done(screen)` is true; panics after [`WAIT`] with `what` and the screen.
    pub async fn wait_until(&mut self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let screen = self.screen();
            if done(&screen) {
                return;
            }
            if tokio::time::timeout_at(deadline, self.ui.recv_background())
                .await
                .is_err()
            {
                let screen = self.screen();
                panic!("timed out after {WAIT:?} waiting for {what}; screen:\n{screen}");
            }
        }
    }

    /// Applies background results until no request is in flight; panics after [`WAIT`].
    pub async fn settle(&mut self) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while self.ui.is_busy() {
            if tokio::time::timeout_at(deadline, self.ui.recv_background())
                .await
                .is_err()
            {
                let screen = self.screen();
                panic!("still busy after {WAIT:?}; screen:\n{screen}");
            }
        }
    }

    /// `true` once the application quit.
    pub fn is_finished(&self) -> bool {
        self.ui.is_finished()
    }
}

/// Runs `prepare` with `env` and no arguments, and builds the application on a [`SIZE`] test terminal.
fn new_ui(env: &TestEnv) -> Ui<TestBackend> {
    let (launch, _out, err) = launch(&[], env);
    let ctx = match launch {
        Launch::Tui(ctx) => ctx,
        Launch::Exit(code) => panic!("prepare exited with {code}: {err}"),
    };
    let terminal = Terminal::new(TestBackend::new(SIZE.0, SIZE.1)).expect("test terminal");
    Ui::new(terminal, ctx)
}
