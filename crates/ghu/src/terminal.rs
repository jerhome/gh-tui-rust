//! The real terminal: raw mode, alternate screen, keyboard thread, restoration.

use crate::Context;

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
pub async fn run(ctx: Context) -> std::io::Result<()> {
    todo!()
}
