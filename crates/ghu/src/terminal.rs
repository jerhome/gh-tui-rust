//! The real terminal: raw mode, alternate screen, keyboard thread, restoration.

use std::cell::Cell;
use std::io;
use std::panic;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
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

thread_local! {
    /// `true` on the threads whose panic leaves the terminal unusable: the one running [`run`] and the keyboard thread.
    static UI_THREAD: Cell<bool> = const { Cell::new(false) };
}

fn mark_ui_thread() {
    UI_THREAD.set(true);
}

fn is_ui_thread() -> bool {
    UI_THREAD.get()
}

/// Restores the terminal when the panicking thread is an interface thread; leaves it alone otherwise (a panic in a
/// background task is caught and shown on screen, the interface keeps running).
fn restore_if_ui(is_ui: impl Fn() -> bool, restore: impl Fn()) {
    if is_ui() {
        restore();
    }
}

/// Leaves the TUI mode: bracketed paste off, alternate screen left, raw mode off. Best effort, never panics.
fn restore_terminal() {
    // Best effort: nothing more can be done if the terminal refuses.
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();
}

/// Enters the TUI mode and builds the terminal. Undoes what it did on failure.
fn init_terminal() -> io::Result<DefaultTerminal> {
    let result = (|| {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Terminal::new(CrosstermBackend::new(io::stdout()))
    })();
    if result.is_err() {
        restore_terminal();
    }
    result
}

/// Runs the TUI on the process's terminal until the user quits.
///
/// - Raw mode, alternate screen and bracketed paste are set up by hand (not `ratatui::init`, whose panic hook
///   restores the terminal on *every* panic, including those of tokio tasks that the UI survives). Our hook,
///   chained before the previous one, restores the terminal (paste included) only when the panicking thread is an
///   interface thread (this one, or the keyboard thread). `restore_terminal` runs on every exit path.
/// - A dedicated OS thread reads crossterm events (blocking, polled every 200 ms so it can stop) and sends them to
///   the loop through a channel; an empty poll sends a tick.
/// - The loop draws, then waits with `tokio::select!` on the next input or [`crate::Ui::recv_background`]; it
///   never awaits a network call directly.
///
/// Must be called from a tokio runtime (the binary uses the multi-thread one), on a thread that stays the same for
/// the whole call (`block_on`). Errors: terminal I/O errors.
pub async fn run(ctx: Context) -> io::Result<()> {
    mark_ui_thread();
    let previous: Arc<PanicHook> = Arc::from(panic::take_hook());
    let chained = Arc::clone(&previous);
    panic::set_hook(Box::new(move |info| {
        restore_if_ui(is_ui_thread, restore_terminal);
        chained(info);
    }));
    let result = match init_terminal() {
        Ok(terminal) => drive(Ui::new(terminal, ctx)).await,
        Err(error) => Err(error),
    };
    restore_terminal();
    // Put the previous hook back (the one installed above is dropped).
    let _ = panic::take_hook();
    panic::set_hook(Box::new(move |info| previous(info)));
    result
}

type PanicHook = dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send + 'static;

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
    mark_ui_thread();
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// Runs `restore_if_ui` on a fresh thread (optionally marked as an interface thread); returns the restore count.
    fn restores_on_thread(ui: bool) -> usize {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        std::thread::spawn(move || {
            if ui {
                mark_ui_thread();
            }
            restore_if_ui(is_ui_thread, || {
                seen.fetch_add(1, Ordering::SeqCst);
            });
        })
        .join()
        .expect("thread");
        count.load(Ordering::SeqCst)
    }

    #[test]
    fn a_panic_on_an_interface_thread_restores_the_terminal() {
        assert_eq!(restores_on_thread(true), 1);
    }

    #[test]
    fn a_panic_on_any_other_thread_leaves_the_terminal_alone() {
        assert_eq!(restores_on_thread(false), 0);
    }
}
