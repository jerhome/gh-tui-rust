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
//! Screens (ADR 0002): a stack whose root is the Repo list; `Enter` opens a screen, `Esc` goes back. Key maps:
//! global in `ui/keys.rs` (`?` help · `Esc` back / close the help · `q` quit · `Ctrl-C` quit), one per screen in
//! its module (`ui/repo_list.rs`, `ui/pulls.rs`); the `?` help shows the top screen's keys, then the global ones.
//!
//! Files: the Pinned repos are kept in `<OS config dir>/ghu/pinned.toml` ([`pinned_repos_path`]); nothing else is
//! stored.

mod env;
mod launch;
mod pins;
mod terminal;
mod ui;

pub use env::{API_URL_VAR, Environment, SystemEnvironment};
pub use launch::{Context, Launch, prepare};
pub use pins::pinned_repos_path;
pub use terminal::run;
pub use ui::{Input, Ui};

/// Re-exported for tests and callers building [`Input::Key`] (same crossterm version as ratatui).
pub use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
