//! The user's external editor (#9, ADR 0003 § 5): command resolution, temporary file, launch, bounded read-back.
//!
//! Contract for dev[tui-editor]. Only [`EditRequest::run`] blocks; it runs off the async loop
//! (`spawn_blocking` in `terminal.rs`, called directly by the test harness).
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: EditorVars is read by EditRequest::run (dev[tui-editor])"
)]

use std::time::Duration;

/// Most bytes read back from the edited file: 1 MiB.
pub const MAX_EDITED_BYTES: u64 = 1_048_576;

/// Default threshold of an [`EditRequest`]: below this time between spawn and exit, an unchanged file means
/// [`EditorError::ReturnedAtOnce`].
pub const RETURNED_AT_ONCE: Duration = Duration::from_secs(1);

/// The raw values of `VISUAL` and `EDITOR` captured by [`crate::prepare`] (`Environment::var`); parsed only when
/// an edit runs. `None` when unset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EditorVars {
    /// `VISUAL`, tried first.
    pub(crate) visual: Option<String>,
    /// `EDITOR`, tried when `VISUAL` is unset or blank.
    pub(crate) editor: Option<String>,
}

/// A request to edit a text in the user's editor, taken from [`crate::Ui::take_edit`].
///
/// `Send`; owns its data (the editor variables and the text), so it can move to a blocking thread.
#[derive(Debug)]
pub struct EditRequest {
    vars: EditorVars,
    text: String,
    returned_at_once: Duration,
}

impl EditRequest {
    /// A request for `text` with the editor variables of the session and the default threshold
    /// [`RETURNED_AT_ONCE`]. Built by `Ui::take_edit` only.
    pub(crate) fn new(vars: EditorVars, text: String) -> Self {
        Self {
            vars,
            text,
            returned_at_once: RETURNED_AT_ONCE,
        }
    }

    /// The same request with another [`EditorError::ReturnedAtOnce`] threshold. For tests: a slow CI runner may
    /// take more than 1 s to spawn the fake editor, so the positive test sets a large one (30 s);
    /// `Duration::ZERO` disables the check. Production code never calls it.
    #[must_use]
    pub fn with_returned_at_once(mut self, threshold: Duration) -> Self {
        self.returned_at_once = threshold;
        self
    }

    /// Runs the editor on the text and returns the edited text. Blocking: call with the terminal suspended, off
    /// the async loop.
    ///
    /// - Command: the first non-blank of `VISUAL`, `EDITOR`, else `vi` (`notepad` on Windows, `cfg!(windows)`);
    ///   split on whitespace, double quotes group (no other escape, no shell).
    /// - Writes the text to a new temporary file `ghu-*.md` (`tempfile`, owner-only on Unix, removed after, even
    ///   on error), runs `<program> <args…> <path>` with the terminal's stdin, stdout and stderr, waits without
    ///   timeout, then reads the file again by path (editors that replace the file are fine), bounded:
    ///   `File::open` + `Read::take(MAX_EDITED_BYTES + 1)` + `read_to_end`; more than [`MAX_EDITED_BYTES`] is
    ///   [`EditorError::TooLarge`] (never `fs::read` then a check).
    /// - Program resolution is std's: on Windows only `.exe` is added to a bare name, so a `.cmd` launcher must be
    ///   named with its extension (`code.cmd --wait`).
    /// - [`EditorError::ReturnedAtOnce`] when the editor exits with success less than the threshold
    ///   ([`RETURNED_AT_ONCE`] unless [`EditRequest::with_returned_at_once`] changed it) after the spawn and the
    ///   file holds exactly the bytes written.
    ///
    /// The returned text is the file's content as is (the caller trims trailing newlines and a leading U+FEFF).
    /// Errors: see [`EditorError`]. Never panics.
    pub fn run(self) -> Result<String, EditorError> {
        todo!("ADR 0003, wave 4: dev[tui-editor]")
    }
}

/// Why the editor gave no text. The text being edited is kept in every case.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EditorError {
    /// `VISUAL` or `EDITOR` has an unbalanced quote or no program.
    #[error("cannot parse {var}")]
    Unparsable {
        /// `"VISUAL"` or `"EDITOR"`.
        var: &'static str,
    },
    /// The program was not found (spawn failed with `NotFound`).
    #[error("editor not found: {program}")]
    NotFound {
        /// The program as resolved from the variable (or the default).
        program: String,
    },
    /// Exited with a failure status.
    #[error("editor failed: {code:?}")]
    Failed {
        /// The exit code; `None` when killed by a signal.
        code: Option<i32>,
    },
    /// More than [`MAX_EDITED_BYTES`] written.
    #[error("edited text too large")]
    TooLarge,
    /// Exited with success within the threshold, file unchanged: probably an editor that does not wait.
    #[error("editor returned at once")]
    ReturnedAtOnce,
    /// The edited file is not valid UTF-8.
    #[error("edited text is not UTF-8")]
    NotUtf8,
    /// Temporary file or spawn failure other than `NotFound`; also a failed blocking task (`JoinError`).
    #[error("editor I/O error: {message}")]
    Io {
        /// The underlying cause.
        message: String,
    },
}
