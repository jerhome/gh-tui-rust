//! The application, independent of the real terminal (ADR 0001; screens and navigation: ADR 0002).
//!
//! Layout of `src/ui/` (one module per screen, so screens are built in parallel without touching the same file):
//!
//! - `ui.rs` (this file): [`Ui`], the screen stack, key dispatch, background requests ([`Shared`], [`Outcome`],
//!   [`RequestId`], [`Shared::push`] for a screen opened by an answer), user messages for errors
//!   ([`error_message`]). Changed only by the `architecte` (a new route adds an [`Outcome`] variant; a new screen
//!   adds nothing here).
//! - `keys.rs`: key map types and the global table.
//! - `render.rs`: the frame (header, body of the top screen, footer with loading indicator and notice, help).
//! - `list.rs`: what the list screens share ([`list::Content`], [`list::Selection`]).
//! - `repo_list.rs`: the Repo list screen (#2, #3, #4), root of the stack.
//! - `pulls.rs`: the PR list of one repo (#5).

mod keys;
mod list;
mod pulls;
mod render;
mod repo_list;

use gh_client::{PullRequest, Repo};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::{Context, KeyEvent};

pub(crate) use keys::HelpRow;

/// An input event from the user's terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A key press (key releases and repeats are filtered out before).
    Key(KeyEvent),
    /// Text pasted in one block (bracketed paste). Goes to the top screen while it is editing text; ignored
    /// otherwise.
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
/// working (help, quit, back). Disk access is limited to the Pinned repos file (a few hundred bytes, written on
/// pin and unpin from the calling thread).
///
/// Owned by one task; not `Send` nor `Sync`. Must be created and used inside a tokio runtime (current-thread is
/// enough).
pub struct Ui<B: Backend> {
    terminal: Terminal<B>,
    app: App,
}

/// The state drawn on screen, separate from the terminal so drawing can borrow both.
pub(crate) struct App {
    pub(crate) shared: Shared,
    rx: mpsc::UnboundedReceiver<(RequestId, Outcome)>,
    /// The screens, root first. Never empty: `stack[0]` is the Repo list and is never popped.
    pub(crate) stack: Vec<Box<dyn View>>,
    pub(crate) help: bool,
    pub(crate) finished: bool,
    pub(crate) spinner: usize,
}

/// Identifies one background request, so its answer reaches the screen that asked for it, and only if that screen
/// still waits for this very request (a screen that reloads, or was left with `Esc`, ignores older answers).
/// Unique for the life of the [`Ui`]; given by [`Shared::spawn`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RequestId(u64);

/// The result of a background request. One variant per client route; a new route adds a variant here.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// [`gh_client::Client::user_repos`].
    Repos(gh_client::Result<Vec<Repo>>),
    /// [`gh_client::Client::repo`] (a pinned repo at start-up or refresh, or a repo typed as `owner/name`).
    Repo(gh_client::Result<Repo>),
    /// [`gh_client::Client::pulls`].
    Pulls(gh_client::Result<Vec<PullRequest>>),
    /// The task panicked or was cancelled; show [`TASK_FAILED`].
    TaskFailed,
}

/// Message for [`Outcome::TaskFailed`].
pub(crate) const TASK_FAILED: &str = "Internal error: a background task failed (see the log).";

/// A one-line message in the footer, in place of the key hints, until the next key press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Notice {
    /// Neutral information (`Pinned alice/notes.`).
    Info(String),
    /// Something failed (drawn in red).
    Error(String),
}

/// What every screen may use: the GitHub client, background requests, the notice line. Owned by [`App`], lent
/// to the top screen for each call.
pub(crate) struct Shared {
    client: gh_client::Client,
    tx: mpsc::UnboundedSender<(RequestId, Outcome)>,
    in_flight: usize,
    next_id: u64,
    pub(crate) notice: Option<Notice>,
    /// Screens asked for by [`Shared::push`], put on the stack by [`App`] once the current call returns.
    pushed: Vec<Box<dyn View>>,
}

impl Shared {
    /// The GitHub client, to clone into a request's future.
    pub(crate) fn client(&self) -> &gh_client::Client {
        &self.client
    }

    /// Runs `work` in its own tokio task and returns the id its outcome will carry. The outcome (or
    /// [`Outcome::TaskFailed`] if the task panics) is later given to [`View::apply`] of the screens, top first,
    /// until one claims it. Counts as in flight (loading indicator) until applied, even if no screen claims it.
    pub(crate) fn spawn(
        &mut self,
        work: impl Future<Output = Outcome> + Send + 'static,
    ) -> RequestId {
        let id = RequestId(self.next_id);
        self.next_id += 1;
        self.in_flight += 1;
        let tx = self.tx.clone();
        let inner = tokio::spawn(work);
        tokio::spawn(async move {
            let outcome = inner.await.unwrap_or_else(|error| {
                tracing::error!(%error, "background task failed");
                Outcome::TaskFailed
            });
            // The receiver lives as long as the `Ui`; once it is dropped nobody needs the result.
            let _ = tx.send((id, outcome));
        });
        id
    }

    /// Shows `notice` in the footer (replacing any previous one) until the next key press.
    pub(crate) fn notify(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    /// Asks to open `view` on top of the stack. For [`View::apply`], which has no [`Flow`] to return (a key
    /// returns [`Flow::Push`] instead). Screens pushed in one call go on the stack in order, after the call.
    pub(crate) fn push(&mut self, view: Box<dyn View>) {
        self.pushed.push(view);
    }

    /// `true` while at least one background request is in flight.
    pub(crate) fn is_busy(&self) -> bool {
        self.in_flight > 0
    }
}

/// What a screen asks the application to do after a key.
pub(crate) enum Flow {
    /// The screen does not use this key: the global table is tried (`?`, `Esc`, `q`).
    Ignored,
    /// The screen used the key.
    Handled,
    /// Open a new screen on top (for example the PR list after `Enter` on a repo).
    Push(Box<dyn View>),
}

/// A screen of the stack (ADR 0002). Point of variation: one implementation per screen module.
///
/// All methods are called on the UI task, never block on the network (requests go through [`Shared::spawn`]), and
/// must not panic on any input or outcome.
pub(crate) trait View {
    /// Header text after `ghu · `, for example `Repo list` or `alice/notes · Pull requests (open)`.
    fn title(&self) -> String;

    /// This screen's rows of the `?` help (shown before the global ones). Usually `keys::help_rows(&TABLE)`.
    fn help(&self) -> Vec<HelpRow>;

    /// `true` while a text field of this screen has the focus (filter, `owner/name` prompt). Then every key but
    /// `Ctrl-C` goes to [`View::key`] first and the global table is not tried (`q` and `?` are text), and pastes go
    /// to [`View::paste`].
    fn editing(&self) -> bool {
        false
    }

    /// Handles a key press. Not called for `Ctrl-C`, nor while the help is shown.
    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow;

    /// Inserts pasted text in the focused field; called only while [`View::editing`].
    fn paste(&mut self, text: &str, shared: &mut Shared) {
        let _ = (text, shared);
    }

    /// Offers a background result. Returns `None` if this screen was waiting for `id` and took it, else gives the
    /// outcome back (`Some`) so the screen below is offered it. A result nobody claims is dropped (stale). To open
    /// a screen in answer, call [`Shared::push`]; it is stacked once the result is dealt with.
    fn apply(&mut self, id: RequestId, outcome: Outcome, shared: &mut Shared) -> Option<Outcome>;

    /// Draws the screen's body in `area` (between header and footer). May place the cursor for a text field.
    fn render(&self, frame: &mut Frame<'_>, area: Rect);
}

impl<B: Backend> Ui<B> {
    /// Takes ownership of the terminal and the context, and opens the first screen, the Repo list, whose loading
    /// starts at once in the background (see `repo_list.rs`). Draws nothing yet.
    pub fn new(terminal: Terminal<B>, ctx: Context) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut shared = Shared {
            client: ctx.client,
            tx,
            in_flight: 0,
            next_id: 0,
            notice: None,
            pushed: Vec::new(),
        };
        let root = repo_list::RepoListScreen::new(&mut shared, ctx.pins, ctx.pins_problem);
        let app = App {
            shared,
            rx,
            stack: vec![Box::new(root)],
            help: false,
            finished: false,
            spinner: 0,
        };
        Self { terminal, app }
    }

    /// Applies one user input (may start background requests). Ignored once [`Ui::is_finished`].
    pub fn input(&mut self, input: Input) {
        if self.app.finished {
            return;
        }
        match input {
            Input::Key(key) => self.app.key(key),
            Input::Paste(text) => self.app.paste(&text),
            Input::Resize => {}
        }
    }

    /// Waits for the next background result (network answer, task failure) and applies it.
    ///
    /// Cancel-safe (meant for `tokio::select!` against input, or a test timeout). Never resolves while nothing is
    /// in flight.
    pub async fn recv_background(&mut self) {
        if !self.app.shared.is_busy() {
            std::future::pending::<()>().await;
        }
        // `recv` is cancel-safe; the sender half lives in `self.app`, so it never returns `None`.
        if let Some((id, outcome)) = self.app.rx.recv().await {
            self.app.apply(id, outcome);
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
        self.app.shared.is_busy()
    }
}

impl App {
    /// The screen on top of the stack.
    pub(crate) fn top(&self) -> &dyn View {
        self.stack
            .last()
            .expect("the stack is never empty")
            .as_ref()
    }

    /// Key dispatch (ADR 0002): `Ctrl-C` quits from anywhere; any key clears the notice; while the help is shown
    /// only `?`, `Esc` (close) and `q` (quit) act; while the top screen is editing, it gets every key; otherwise
    /// the top screen first, then the global table if it returned [`Flow::Ignored`].
    fn key(&mut self, key: KeyEvent) {
        if keys::is_ctrl_c(key) {
            self.finished = true;
            return;
        }
        self.shared.notice = None;
        if self.help {
            match keys::lookup(&keys::GLOBAL, key) {
                Some(keys::Global::Quit) => self.finished = true,
                Some(keys::Global::ToggleHelp | keys::Global::Back) => self.help = false,
                None => {}
            }
            return;
        }
        let top = self.stack.last_mut().expect("the stack is never empty");
        let editing = top.editing();
        let flow = top.key(key, &mut self.shared);
        match flow {
            Flow::Handled => {}
            Flow::Push(view) => self.stack.push(view),
            Flow::Ignored if editing => {}
            Flow::Ignored => match keys::lookup(&keys::GLOBAL, key) {
                Some(keys::Global::Quit) => self.finished = true,
                Some(keys::Global::ToggleHelp) => self.help = true,
                Some(keys::Global::Back) => self.pop(),
                None => {}
            },
        }
        // After the flow, so a screen pushed by `Shared::push` goes on top of one returned as `Flow::Push`.
        self.stack.append(&mut self.shared.pushed);
    }

    /// Goes back one screen; does nothing on the root.
    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn paste(&mut self, text: &str) {
        if self.help {
            return;
        }
        let top = self.stack.last_mut().expect("the stack is never empty");
        if top.editing() {
            top.paste(text, &mut self.shared);
        }
    }

    /// Offers the outcome to the screens, top first, until one claims it; then stacks the screens they pushed.
    fn apply(&mut self, id: RequestId, outcome: Outcome) {
        self.shared.in_flight = self.shared.in_flight.saturating_sub(1);
        let mut outcome = Some(outcome);
        for view in self.stack.iter_mut().rev() {
            match view.apply(
                id,
                outcome.take().expect("set until claimed"),
                &mut self.shared,
            ) {
                None => break,
                Some(unclaimed) => outcome = Some(unclaimed),
            }
        }
        if outcome.is_some() {
            tracing::debug!(?id, "stale background result dropped");
        }
        self.stack.append(&mut self.shared.pushed);
    }
}

/// The message shown for a failed request: `Cannot <action>: <reason>`, one reason per error variant, written
/// here (the `Display` of the error is for logs, see gh-client's `Error`). Never built from GitHub's free text or
/// from the error's fields, except the HTTP status of [`gh_client::Error::Api`].
///
/// `action` is a lowercase verb phrase: `load the repos`, `open bob/tools`, `load the pull requests`. A 401
/// gives a full sentence of its own that names `GH_TOKEN`, `GITHUB_TOKEN` and `gh auth login`, whatever the action.
pub(crate) fn error_message(action: &str, error: &gh_client::Error) -> String {
    use gh_client::Error;
    let reason = match error {
        Error::Unauthorized => {
            return "GitHub rejected the token (401): it is invalid, expired or revoked. \
                    Set GH_TOKEN or GITHUB_TOKEN to a valid token, or sign in again with: gh auth login"
                .to_owned();
        }
        Error::Network { .. } => {
            "no answer from GitHub. Check your connection, the proxy and GHU_API_URL.".to_owned()
        }
        Error::RateLimited { .. } => {
            "GitHub rate limit exceeded. Wait a few minutes and refresh.".to_owned()
        }
        Error::Forbidden { .. } => {
            "GitHub denied access (403). The token may lack the required scope.".to_owned()
        }
        Error::NotFound => {
            "not found on GitHub (404): it does not exist or this token cannot see it.".to_owned()
        }
        Error::Unprocessable { .. } => "GitHub refused the request (422).".to_owned(),
        Error::Api { status, .. } => format!("GitHub answered with an error (HTTP {status})."),
        Error::UnexpectedResponse { .. } => {
            "GitHub sent an answer that ghu cannot read.".to_owned()
        }
        Error::InvalidBaseUrl { .. } => {
            "the GitHub API URL is invalid. Check GHU_API_URL.".to_owned()
        }
        Error::InvalidRepoName { .. } => "invalid repo name: type owner/name.".to_owned(),
        Error::ClientSetup { .. } => "the HTTP client could not be set up.".to_owned(),
        // `Error` is non-exhaustive: a future variant gets a generic message until it has its own.
        _ => "an unexpected error occurred (see the log).".to_owned(),
    };
    format!("Cannot {action}: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyCode;
    use ratatui::crossterm::event::KeyModifiers;

    /// A screen that asks for another one with [`Shared::push`] when a key is pressed.
    struct Pusher;

    impl View for Pusher {
        fn title(&self) -> String {
            "pusher".to_owned()
        }
        fn help(&self) -> Vec<HelpRow> {
            Vec::new()
        }
        fn key(&mut self, _key: KeyEvent, shared: &mut Shared) -> Flow {
            shared.push(Box::new(Pusher));
            Flow::Handled
        }
        fn apply(&mut self, _: RequestId, outcome: Outcome, _: &mut Shared) -> Option<Outcome> {
            Some(outcome)
        }
        fn render(&self, _frame: &mut Frame<'_>, _area: Rect) {}
    }

    #[tokio::test]
    async fn a_screen_pushed_during_a_key_is_stacked_after_the_key() {
        let (tx, rx) = mpsc::unbounded_channel();
        let client = gh_client::Client::new(
            gh_client::ApiBase::default(),
            gh_client::Token::new("unused").expect("token"),
        )
        .expect("client");
        let mut app = App {
            shared: Shared {
                client,
                tx,
                in_flight: 0,
                next_id: 0,
                notice: None,
                pushed: Vec::new(),
            },
            rx,
            stack: vec![Box::new(Pusher)],
            help: false,
            finished: false,
            spinner: 0,
        };
        app.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.stack.len(), 2);
        assert!(app.shared.pushed.is_empty());
    }
}
