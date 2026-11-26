//! Pinned repos: the list kept in `<OS config dir>/ghu/pinned.toml` (ADR 0002 § Pinned repos file).
//!
//! File format (TOML, UTF-8), written by ghu:
//!
//! ```toml
//! # Pinned repos of ghu (owner/name), in pin order. Rewritten by ghu on each pin and unpin.
//! pinned = ["octo-org/hello-world", "bob/tools"]
//! ```
//!
//! Reading: a missing file (or missing directory) is an empty list. A missing `pinned` key is an empty list. Unknown
//! keys are ignored (and dropped at the next write). Each entry must parse as a [`RepoName`]; a second entry naming
//! the same repo ([`RepoName::same_repo`]) is dropped. A file over 1 MiB, not UTF-8, not TOML, with `pinned` not an
//! array of strings, or with an invalid entry is refused as a whole ([`PinsError::Invalid`]): it is never
//! overwritten during the session, and pin/unpin fail with [`PinsError::Disabled`], so the user can fix it by hand.
//!
//! Writing: the whole file is rewritten (header comment above, then `pinned = [...]` with GitHub's canonical names
//! in pin order), atomically: written to `pinned.toml.tmp` in the same directory, then renamed over `pinned.toml`
//! (`std::fs::rename` replaces the target on Windows too). The `ghu` directory is created if missing. Default file
//! permissions (nothing secret is stored). Blocking, but tiny: called on the UI task.

// Skeleton (architecte): the store is wired by dev[pins] (load) and dev[tui-repos] (pin, unpin), who remove this.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use gh_client::RepoName;

/// Directory of ghu inside the OS configuration directory.
const APP_DIR: &str = "ghu";
/// Name of the Pinned repos file.
const FILE_NAME: &str = "pinned.toml";

/// The Pinned repos file for a configuration directory: `<config_dir>/ghu/pinned.toml` (built with
/// [`Path::join`], so portable). Tests use it to pre-write or read the file in their temporary directory.
pub fn pinned_repos_path(config_dir: &Path) -> PathBuf {
    config_dir.join(APP_DIR).join(FILE_NAME)
}

/// Why the Pinned repos could not be read or saved. The `Display` text is shown to the user as is (English, one
/// line, names the file, never contains a token: none is in the file).
#[derive(Debug, thiserror::Error)]
pub(crate) enum PinsError {
    /// The file exists but cannot be read (permissions, I/O).
    #[error("Cannot read the pinned repos file {}: {source}. Pinning is disabled.", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },
    /// The file is not a valid Pinned repos file (see the module documentation).
    #[error("The pinned repos file {} is invalid ({reason}). Pinning is disabled; fix or delete the file.", path.display())]
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong, short (`line 2: expected an array`, `invalid repo name "a/b/c"`, `larger than 1 MiB`).
        reason: String,
    },
    /// Saving failed (directory creation, write or rename). The store is unchanged.
    #[error("Cannot save the pinned repos file {}: {source}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },
    /// Pin or unpin refused because the file could not be read at start-up ([`PinsError::Read`] or
    /// [`PinsError::Invalid`]).
    #[error("Pinning is disabled: the pinned repos file could not be read at start-up.")]
    Disabled,
}

/// The Pinned repos, in pin order, and where they are saved. Owned by the Repo list screen (moved from
/// [`crate::Context`]); `Send`, not shared.
///
/// Invariant: no two entries name the same repo ([`RepoName::same_repo`]); in [`Mode::Saved`] the file holds
/// exactly [`PinStore::pinned`] after every successful [`PinStore::pin`] or [`PinStore::unpin`].
#[derive(Debug)]
pub(crate) struct PinStore {
    pinned: Vec<RepoName>,
    mode: Mode,
}

/// Where changes go.
#[derive(Debug)]
enum Mode {
    /// No configuration directory: changes live for this session only.
    Memory,
    /// Saved to this file after each change.
    Saved(PathBuf),
    /// The file could not be read at start-up: the list is empty and changes are refused.
    Disabled,
}

impl PinStore {
    /// An empty store kept in memory only (no configuration directory known). Never fails.
    pub(crate) fn in_memory() -> Self {
        Self {
            pinned: Vec::new(),
            mode: Mode::Memory,
        }
    }

    /// Loads the store for `config_dir` (the OS configuration directory, see [`crate::Environment::config_dir`]).
    ///
    /// - `None` → [`PinStore::in_memory`], no error.
    /// - File or directory missing → empty store saved to [`pinned_repos_path`], no error (nothing is created
    ///   before the first pin).
    /// - File read → the store, no error.
    /// - Unreadable or invalid file → an empty, disabled store and the error ([`PinsError::Read`] or
    ///   [`PinsError::Invalid`]), which the Repo list shows as a notice. The file is left untouched.
    ///
    /// Blocking (reads one small file); called by [`crate::prepare`].
    pub(crate) fn load(config_dir: Option<&Path>) -> (Self, Option<PinsError>) {
        todo!("dev[pins]: load {config_dir:?}")
    }

    /// The Pinned repos, in pin order.
    pub(crate) fn pinned(&self) -> &[RepoName] {
        &self.pinned
    }

    /// `true` if `name` is pinned ([`RepoName::same_repo`]: case-insensitive).
    pub(crate) fn is_pinned(&self, name: &RepoName) -> bool {
        self.pinned.iter().any(|pinned| pinned.same_repo(name))
    }

    /// Pins `name` at the end of the list and saves. `Ok(false)` (and no write) if it is already pinned; the stored
    /// spelling is then replaced by `name` if they differ in case only (and saved), so the file keeps GitHub's
    /// canonical name.
    ///
    /// Errors: [`PinsError::Disabled`]; [`PinsError::Write`], in which case the store is unchanged.
    pub(crate) fn pin(&mut self, name: RepoName) -> Result<bool, PinsError> {
        todo!("dev[pins]: pin {name}")
    }

    /// Unpins `name` ([`RepoName::same_repo`]) and saves. `Ok(false)` (and no write) if it was not pinned.
    ///
    /// Errors: [`PinsError::Disabled`]; [`PinsError::Write`], in which case the store is unchanged.
    pub(crate) fn unpin(&mut self, name: &RepoName) -> Result<bool, PinsError> {
        todo!("dev[pins]: unpin {name}")
    }

    /// The file changes are saved to; `None` in memory or disabled mode.
    pub(crate) fn path(&self) -> Option<&Path> {
        match &self.mode {
            Mode::Saved(path) => Some(path),
            Mode::Memory | Mode::Disabled => None,
        }
    }
}
