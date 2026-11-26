//! The application, independent of the real terminal. Private submodules (state, update, rendering, background
//! tasks) are free for the `dev` lot to organise under `src/ui/` or `src/app/`.

use ratatui::Terminal;
use ratatui::backend::Backend;

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
    #[allow(dead_code)]
    terminal: Terminal<B>,
}

impl<B: Backend> Ui<B> {
    /// Takes ownership of the terminal and the context, and starts the first screen: the Repo list, whose loading
    /// (the user's own repos, ticket #2) starts at once in the background. Draws nothing yet.
    pub fn new(terminal: Terminal<B>, ctx: Context) -> Self {
        todo!()
    }

    /// Applies one user input (may start background requests). Ignored once [`Ui::is_finished`].
    pub fn input(&mut self, input: Input) {
        todo!()
    }

    /// Waits for the next background result (network answer, task failure) and applies it.
    ///
    /// Cancel-safe (meant for `tokio::select!` against input, or a test timeout). Never resolves while nothing is
    /// in flight.
    pub async fn recv_background(&mut self) {
        todo!()
    }

    /// Advances animations (the loading spinner). The real loop calls it about every 200 ms.
    pub fn tick(&mut self) {
        todo!()
    }

    /// Draws the current screen on the terminal.
    ///
    /// Errors: the backend's error (I/O on a real terminal; never for `TestBackend`).
    pub fn draw(&mut self) -> Result<(), B::Error> {
        todo!()
    }

    /// The terminal, for tests reading `terminal().backend().buffer()` after [`Ui::draw`].
    pub fn terminal(&self) -> &Terminal<B> {
        todo!()
    }

    /// `true` once the user asked to quit (`q`, `Ctrl-C`).
    pub fn is_finished(&self) -> bool {
        todo!()
    }

    /// `true` while at least one background request is in flight.
    pub fn is_busy(&self) -> bool {
        todo!()
    }
}
