//! The user's external editor (#9, ADR 0003 § 5): command resolution, temporary file, launch, bounded read-back.
//!
//! Contract for dev[tui-editor]. Only [`EditRequest::run`] blocks; it runs off the async loop
//! (`spawn_blocking` in `terminal.rs`, called directly by the test harness).

use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

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
pub struct EditRequest {
    vars: EditorVars,
    text: String,
    returned_at_once: Duration,
}

/// Never prints the text (it may be a private draft): only the variables, its length in chars and the threshold
/// (ADR 0003 § 9).
impl fmt::Debug for EditRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditRequest")
            .field("vars", &self.vars)
            .field("chars", &self.text.chars().count())
            .field("returned_at_once", &self.returned_at_once)
            .finish()
    }
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
        self.run_in(&std::env::temp_dir())
    }

    /// [`EditRequest::run`] with the temporary file created in `dir` (tests look at what is left there).
    fn run_in(self, dir: &Path) -> Result<String, EditorError> {
        let (program, args) = command(&self.vars)?;
        let path =
            temp_file(dir, &self.text).map_err(|error| io_error("temporary file", &error))?;
        let start = Instant::now();
        let status =
            editor_command(&program, &args, &path)
                .status()
                .map_err(|error| match error.kind() {
                    io::ErrorKind::NotFound => EditorError::NotFound {
                        program: program.to_string_lossy().into_owned(),
                    },
                    _ => io_error("cannot start the editor", &error),
                })?;
        let elapsed = start.elapsed();
        if !status.success() {
            return Err(EditorError::Failed {
                code: status.code(),
            });
        }
        let bytes =
            read_bounded(&path).map_err(|error| io_error("cannot read the edited file", &error))?;
        // `path` (a TempPath) removes the file when dropped, on every return above and below.
        if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_EDITED_BYTES) {
            return Err(EditorError::TooLarge);
        }
        if elapsed < self.returned_at_once && bytes == self.text.as_bytes() {
            return Err(EditorError::ReturnedAtOnce);
        }
        String::from_utf8(bytes).map_err(|_| EditorError::NotUtf8)
    }
}

/// The program and its arguments: the first non-blank of `VISUAL`, `EDITOR`, else the default editor.
fn command(vars: &EditorVars) -> Result<(OsString, Vec<String>), EditorError> {
    let chosen = [("VISUAL", &vars.visual), ("EDITOR", &vars.editor)]
        .into_iter()
        .find_map(|(name, value)| {
            value
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .map(|text| (name, text))
        });
    let Some((var, text)) = chosen else {
        return Ok((OsString::from(default_editor()), Vec::new()));
    };
    let mut words = split(text)
        .ok_or(EditorError::Unparsable { var })?
        .into_iter();
    let program = words
        .next()
        .filter(|program| !program.is_empty())
        .ok_or(EditorError::Unparsable { var })?;
    Ok((OsString::from(program), words.collect()))
}

/// The editor process: the terminal's stdio, and no GitHub token in its environment (an editor, or a plugin of
/// it, has no business reading them).
fn editor_command(program: &OsString, args: &[String], path: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(path)
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN");
    command
}

/// `vi`, or `notepad` on Windows.
fn default_editor() -> &'static str {
    if cfg!(windows) { "notepad" } else { "vi" }
}

/// Splits on whitespace; double quotes group (and are removed), nothing else is special. `None` for an unbalanced
/// quote. An empty pair of quotes is an empty word.
fn split(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// A new `ghu-*.md` file in `dir` holding `text`, closed (an editor may replace it), owner-only on Unix
/// (tempfile's default), removed when the returned path is dropped.
fn temp_file(dir: &Path, text: &str) -> io::Result<tempfile::TempPath> {
    let mut file = tempfile::Builder::new()
        .prefix("ghu-")
        .suffix(".md")
        .tempfile_in(dir)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    Ok(file.into_temp_path())
}

/// The file's bytes, at most `MAX_EDITED_BYTES + 1` (one more tells that it is too large).
fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_EDITED_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn io_error(what: &str, error: &io::Error) -> EditorError {
    EditorError::Io {
        message: format!("{what}: {error}"),
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

#[cfg(test)]
mod tests {
    #[test]
    fn debug_of_a_request_never_shows_the_text() {
        let request = EditRequest::new(EditorVars::default(), "my secret!".to_owned() + "ab");
        let shown = format!("{request:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("chars: 12"), "{shown}");
    }

    use super::*;

    fn vars(visual: Option<&str>, editor: Option<&str>) -> EditorVars {
        EditorVars {
            visual: visual.map(str::to_owned),
            editor: editor.map(str::to_owned),
        }
    }

    fn program(vars: &EditorVars) -> String {
        command(vars)
            .expect("command")
            .0
            .into_string()
            .expect("UTF-8")
    }

    #[test]
    fn visual_beats_editor_which_beats_the_default() {
        assert_eq!(program(&vars(Some("nano"), Some("vim"))), "nano");
        assert_eq!(program(&vars(None, Some("vim"))), "vim");
        assert_eq!(program(&vars(Some("  "), Some("vim"))), "vim");
        assert_eq!(program(&vars(Some(""), Some("\t"))), default_editor());
        assert_eq!(program(&vars(None, None)), default_editor());
    }

    #[test]
    fn the_default_editor_depends_on_the_os() {
        #[cfg(windows)]
        assert_eq!(default_editor(), "notepad");
        #[cfg(not(windows))]
        assert_eq!(default_editor(), "vi");
    }

    #[test]
    fn words_split_on_whitespace_and_double_quotes_group() {
        let words = |text: &str| split(text).map(|w| w.join("|"));
        assert_eq!(words("code --wait").as_deref(), Some("code|--wait"));
        assert_eq!(
            words(r#""C:\Program Files\Notepad++\notepad++.exe" -multiInst"#).as_deref(),
            Some(r"C:\Program Files\Notepad++\notepad++.exe|-multiInst")
        );
        assert_eq!(words("  vim   -u  NONE ").as_deref(), Some("vim|-u|NONE"));
        assert_eq!(words(r#"a"b c"d"#).as_deref(), Some("ab cd"));
        assert_eq!(words(r#"ed """#).as_deref(), Some("ed|"));
        assert_eq!(words(r"back\slash").as_deref(), Some(r"back\slash"));
        assert_eq!(words(r#"vim "unbalanced"#), None);
    }

    #[test]
    fn an_unbalanced_quote_or_no_program_is_unparsable_with_the_variable_named() {
        let error = command(&vars(Some(r#""vim"#), Some("nano"))).expect_err("unbalanced");
        assert!(
            matches!(error, EditorError::Unparsable { var: "VISUAL" }),
            "{error:?}"
        );
        let error = command(&vars(None, Some(r#""" -x"#))).expect_err("empty program");
        assert!(
            matches!(error, EditorError::Unparsable { var: "EDITOR" }),
            "{error:?}"
        );
    }

    #[test]
    fn the_editor_does_not_inherit_the_github_tokens() {
        let command = editor_command(
            &OsString::from("ed"),
            &["-x".to_owned()],
            Path::new("file.md"),
        );
        let envs: Vec<_> = command.get_envs().collect();
        for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
            assert!(
                envs.contains(&(std::ffi::OsStr::new(name), None)),
                "{name} must be removed: {envs:?}"
            );
        }
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-x", "file.md"]);
    }

    /// Unix: an editor written in `sh` records the mode of the file it received (`ls -l`) next to it; the
    /// temporary file itself is gone after the run.
    #[cfg(unix)]
    #[test]
    fn the_temporary_file_is_owner_only_and_removed_after_use() {
        let dir = tempfile::tempdir().expect("dir");
        let request = EditRequest::new(
            vars(
                Some(r#"sh -c "ls -l $0 > $0.mode; echo edited > $0""#),
                None,
            ),
            "text".to_owned(),
        );
        assert_eq!(request.run_in(dir.path()).expect("run"), "edited\n");
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .into_string()
                    .expect("UTF-8")
            })
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(
            left[0].starts_with("ghu-") && left[0].ends_with(".md.mode"),
            "{left:?}"
        );
        let mode = std::fs::read_to_string(dir.path().join(&left[0])).expect("mode");
        // Only the 10 mode characters: a suffix follows on SELinux ('.') or macOS ('@').
        assert_eq!(
            mode.chars().take(10).collect::<String>(),
            "-rw-------",
            "{mode}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_temporary_file_is_removed_after_a_failure_too() {
        let dir = tempfile::tempdir().expect("dir");
        let request = EditRequest::new(vars(Some(r#"sh -c "exit 3""#), None), "text".to_owned());
        let error = request.run_in(dir.path()).expect_err("fails");
        assert!(
            matches!(error, EditorError::Failed { code: Some(3) }),
            "{error:?}"
        );
        assert_eq!(std::fs::read_dir(dir.path()).expect("read dir").count(), 0);
    }
}
