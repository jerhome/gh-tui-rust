//! The application, independent of the real terminal. Private submodules (state, update, rendering, background
//! tasks) are free for the `dev` lot to organise under `src/ui/` or `src/app/`.

mod keys;
mod render;

use gh_client::{Affiliation, Repo};
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::mpsc;

use crate::{Context, KeyEvent};

/// An input event from the user's terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A key press (key releases and repeats are filtered out before).
    Key(KeyEvent),
    /// Text pasted in one block (bracketed paste).
    Paste(String),
    /// The terminal was resized; the next draw adapts.
    Resize,
}

/// The running application: state, screens and background requests, drawn on a ratatui `Terminal<B>`.
///
/// Event loop contract (ADR 0001): every method returns without waiting on the network. Each GitHub call runs in
/// its own tokio task with a clone of the client; its result comes back through an internal channel and is
/// applied by [`Ui::recv_background`]. A panic in such a task is turned into an error shown on screen, not a crash.
/// While at least one request is in flight, the screen shows a loading indicator (text `Loading`), and keys keep
/// working (help, quit).
///
/// Owned by one task; not `Sync`. Must be created and used inside a tokio runtime (current-thread is enough).
pub struct Ui<B: Backend> {
    terminal: Terminal<B>,
    app: App,
}

/// The state drawn on screen, separate from the terminal so drawing can borrow both.
pub(crate) struct App {
    client: gh_client::Client,
    pub(crate) repos: RepoList,
    pub(crate) help: bool,
    pub(crate) finished: bool,
    pub(crate) spinner: usize,
    in_flight: usize,
    tx: mpsc::UnboundedSender<Outcome>,
    rx: mpsc::UnboundedReceiver<Outcome>,
}

/// What the Repo list shows.
pub(crate) enum RepoList {
    /// The first answer has not come yet.
    Loading,
    /// The repos, in GitHub's order.
    Loaded(Vec<Repo>),
    /// The request failed: a message for the user.
    Failed(String),
}

/// The result of a background request.
enum Outcome {
    Repos(Result<Vec<Repo>, gh_client::Error>),
    /// The task panicked or was cancelled.
    TaskFailed,
}

impl<B: Backend> Ui<B> {
    /// Takes ownership of the terminal and the context, and starts the first screen: the Repo list, whose loading
    /// (the user's own repos, ticket #2) starts at once in the background. Draws nothing yet.
    pub fn new(terminal: Terminal<B>, ctx: Context) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut app = App {
            client: ctx.client,
            repos: RepoList::Loading,
            help: false,
            finished: false,
            spinner: 0,
            in_flight: 0,
            tx,
            rx,
        };
        app.load_repos();
        Self { terminal, app }
    }

    /// Applies one user input (may start background requests). Ignored once [`Ui::is_finished`].
    pub fn input(&mut self, input: Input) {
        if self.app.finished {
            return;
        }
        if let Input::Key(key) = input {
            self.app.key(key);
        }
    }

    /// Waits for the next background result (network answer, task failure) and applies it.
    ///
    /// Cancel-safe (meant for `tokio::select!` against input, or a test timeout). Never resolves while nothing is
    /// in flight.
    pub async fn recv_background(&mut self) {
        if self.app.in_flight == 0 {
            std::future::pending::<()>().await;
        }
        // `recv` is cancel-safe; the sender half lives in `self.app`, so it never returns `None`.
        if let Some(outcome) = self.app.rx.recv().await {
            self.app.apply(outcome);
        }
    }

    /// Advances animations (the loading spinner). The real loop calls it about every 200 ms.
    pub fn tick(&mut self) {
        self.app.spinner = self.app.spinner.wrapping_add(1);
    }

    /// Draws the current screen on the terminal.
    ///
    /// Errors: the backend's error (I/O on a real terminal; never for `TestBackend`).
    pub fn draw(&mut self) -> Result<(), B::Error> {
        let app = &self.app;
        self.terminal.draw(|frame| render::draw(frame, app))?;
        Ok(())
    }

    /// The terminal, for tests reading `terminal().backend().buffer()` after [`Ui::draw`].
    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// `true` once the user asked to quit (`q`, `Ctrl-C`).
    pub fn is_finished(&self) -> bool {
        self.app.finished
    }

    /// `true` while at least one background request is in flight.
    pub fn is_busy(&self) -> bool {
        self.app.in_flight > 0
    }
}

impl App {
    /// `true` while at least one background request is in flight.
    pub(crate) fn is_busy(&self) -> bool {
        self.in_flight > 0
    }

    fn key(&mut self, key: KeyEvent) {
        match keys::action(key) {
            Some(keys::Action::Quit) => self.finished = true,
            Some(keys::Action::ToggleHelp) => self.help = !self.help,
            Some(keys::Action::Back) => self.help = false,
            None => {}
        }
    }

    /// Starts loading the user's own repos in the background.
    fn load_repos(&mut self) {
        let client = self.client.clone();
        self.spawn(async move { Outcome::Repos(client.user_repos(&[Affiliation::Owner]).await) });
    }

    /// Runs `work` in its own task; its outcome (or [`Outcome::TaskFailed`] if it panics) comes back on the channel.
    fn spawn(&mut self, work: impl Future<Output = Outcome> + Send + 'static) {
        self.in_flight += 1;
        let tx = self.tx.clone();
        let inner = tokio::spawn(work);
        tokio::spawn(async move {
            let outcome = inner.await.unwrap_or_else(|error| {
                tracing::error!(%error, "background task failed");
                Outcome::TaskFailed
            });
            // The receiver lives as long as the `Ui`; once it is dropped nobody needs the result.
            let _ = tx.send(outcome);
        });
    }

    fn apply(&mut self, outcome: Outcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.repos = match outcome {
            Outcome::Repos(Ok(repos)) => {
                tracing::debug!(count = repos.len(), "repos loaded");
                RepoList::Loaded(repos)
            }
            Outcome::Repos(Err(error)) => {
                tracing::warn!(%error, "loading repos failed");
                RepoList::Failed(error_message(&error))
            }
            Outcome::TaskFailed => RepoList::Failed(
                "Internal error: a background task failed (see the log).".to_owned(),
            ),
        };
    }
}

/// The message shown for a failed request: one per error variant, written here (the `Display` of the error is
/// for logs, see gh-client's `Error`). Never built from GitHub's free text or from the error's fields.
fn error_message(error: &gh_client::Error) -> String {
    use gh_client::Error;
    let reason = match error {
        Error::Unauthorized => {
            return "GitHub rejected the token (401): it is invalid, expired or revoked. \
                    Set GH_TOKEN or GITHUB_TOKEN to a valid token, or sign in again with: gh auth login"
                .to_owned();
        }
        Error::Network { .. } => {
            "no answer from GitHub. Check your connection, the proxy and GHU_API_URL."
        }
        Error::RateLimited { .. } => "GitHub rate limit exceeded. Wait a few minutes and refresh.",
        Error::Forbidden { .. } => {
            "GitHub denied access (403). The token may lack the required scope."
        }
        Error::NotFound => "not found on GitHub (404).",
        Error::Unprocessable { .. } => "GitHub refused the request (422).",
        Error::Api { status, .. } => {
            return format!(
                "Cannot load the repos: GitHub answered with an error (HTTP {status})."
            );
        }
        Error::UnexpectedResponse { .. } => "GitHub sent an answer that ghu cannot read.",
        Error::InvalidBaseUrl { .. } => "the GitHub API URL is invalid. Check GHU_API_URL.",
        Error::InvalidRepoName { .. } => "invalid repo name.",
        Error::ClientSetup { .. } => "the HTTP client could not be set up.",
        // `Error` is non-exhaustive: a future variant gets a generic message until it has its own.
        _ => "an unexpected error occurred (see the log).",
    };
    format!("Cannot load the repos: {reason}")
}
