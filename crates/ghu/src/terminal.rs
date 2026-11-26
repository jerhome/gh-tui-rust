//! The real terminal: raw mode, alternate screen, keyboard thread, restoration.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use tokio::sync::mpsc;

use crate::{Context, Input, Ui};

/// How long the keyboard thread waits for an event before sending a tick (and checking whether to stop).
const POLL: Duration = Duration::from_millis(200);

/// What the keyboard thread sends to the loop.
enum Message {
    Input(Input),
    Tick,
    Failed(io::Error),
}

/// Runs the TUI on the process's terminal until the user quits.
///
/// - `ratatui::init()` (raw mode, alternate screen, panic hook that restores the terminal before the panic message);
///   bracketed paste enabled; `ratatui::restore()` on every exit path, error included.
/// - A dedicated OS thread reads crossterm events (blocking, polled every 200 ms so it can stop) and sends them to
///   the loop through a channel; an empty poll sends a tick.
/// - The loop draws, then waits with `tokio::select!` on the next input or [`crate::Ui::recv_background`]; it
///   never awaits a network call directly.
///
/// Must be called from a tokio runtime (the binary uses the multi-thread one). Errors: terminal I/O errors.
pub async fn run(ctx: Context) -> io::Result<()> {
    let terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            // `try_init` may fail after raw mode is on: undo what it did.
            ratatui::restore();
            return Err(error);
        }
    };
    let result = match execute!(io::stdout(), EnableBracketedPaste) {
        Ok(()) => drive(Ui::new(terminal, ctx)).await,
        Err(error) => Err(error),
    };
    // Best effort while leaving: the terminal is restored below even if this fails.
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

/// The event loop, between terminal set-up and restoration.
async fn drive<B: ratatui::backend::Backend<Error = io::Error>>(mut ui: Ui<B>) -> io::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let stop = Arc::new(AtomicBool::new(false));
    let keyboard = {
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("ghu-keyboard".into())
            .spawn(move || read_keyboard(&tx, &stop))?
    };

    let result = loop {
        if let Err(error) = ui.draw() {
            break Err(error);
        }
        if ui.is_finished() {
            break Ok(());
        }
        tokio::select! {
            message = rx.recv() => match message {
                Some(Message::Input(input)) => ui.input(input),
                Some(Message::Tick) => ui.tick(),
                Some(Message::Failed(error)) => break Err(error),
                None => break Err(io::Error::other("the keyboard thread stopped")),
            },
            () = ui.recv_background() => {}
        }
    };

    stop.store(true, Ordering::Relaxed);
    drop(rx);
    // Ends within one poll period; a panic there was already reported by the panic hook.
    let _ = keyboard.join();
    result
}

/// Body of the keyboard thread: forwards key presses, pastes and resizes until `stop` or the loop is gone.
fn read_keyboard(tx: &mpsc::UnboundedSender<Message>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let message = match event::poll(POLL) {
            Ok(false) => Message::Tick,
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    Message::Input(Input::Key(key))
                }
                Ok(Event::Paste(text)) => Message::Input(Input::Paste(text)),
                Ok(Event::Resize(..)) => Message::Input(Input::Resize),
                Ok(_) => continue,
                Err(error) => Message::Failed(error),
            },
            Err(error) => Message::Failed(error),
        };
        let failed = matches!(message, Message::Failed(_));
        if tx.send(message).is_err() || failed {
            return;
        }
    }
}
