//! `ghu`: terminal client for GitHub (ADR 0001).
//!
//! The binary (`main.rs`) is a thin shell over this library, so tests drive the whole application:
//!
//! 1. [`prepare`] parses the arguments and reads the [`Environment`] (token, API URL) without touching the
//!    terminal. It either ends the program ([`Launch::Exit`]: `--help`, `--version`, usage error, no token…) or
//!    hands over a [`Context`] ([`Launch::Tui`]).
//! 2. [`run`] puts the real terminal in TUI mode and drives a [`Ui`] with keyboard events until the user quits.
//!    Tests instead build a [`Ui`] on ratatui's `TestBackend`, send it [`Input`]s and read the screen, against the
//!    fake GitHub server (`gh-fake`) in real HTTP.
//!
//! Key map (the table is `ui/keys.rs`, shown by the `?` help): `q` quit · `Ctrl-C` quit · `Esc` close the help ·
//! `?` toggle the key help.

mod env;
mod launch;
mod terminal;
mod ui;

pub use env::{API_URL_VAR, Environment, SystemEnvironment};
pub use launch::{Context, Launch, prepare};
pub use terminal::run;
pub use ui::{Input, Ui};

/// Re-exported for tests and callers building [`Input::Key`] (same crossterm version as ratatui).
pub use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
