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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use gh_client::RepoName;

/// Directory of ghu inside the OS configuration directory.
const APP_DIR: &str = "ghu";
/// Name of the Pinned repos file.
const FILE_NAME: &str = "pinned.toml";
/// Largest file accepted, in bytes (1 MiB).
const MAX_FILE_SIZE: u64 = 1024 * 1024;
/// Comment written at the top of the file.
const HEADER: &str =
    "# Pinned repos of ghu (owner/name), in pin order. Rewritten by ghu on each pin and unpin.\n";

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
    #[error("Cannot read the pinned repos file ({source}). Pinning is disabled. File: {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },
    /// The file is not a valid Pinned repos file (see the module documentation).
    #[error("The pinned repos file is invalid ({reason}). Pinning is disabled; fix or delete the file: {}", path.display())]
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong, short (`line 2: expected an array`, `invalid repo name "a/b/c"`, `larger than 1 MiB`).
        reason: String,
    },
    /// Saving failed (directory creation, write or rename). The store is unchanged.
    #[error("Cannot save the pinned repos file ({source}). File: {}", path.display())]
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

fn invalid(path: &Path, reason: impl Into<String>) -> PinsError {
    PinsError::Invalid {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// Reads the file: `Ok(None)` if it does not exist.
fn read_file(path: &Path) -> Result<Option<Vec<RepoName>>, PinsError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(PinsError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    // Read one byte past the limit to detect an oversized file without loading it.
    let mut bytes = Vec::new();
    file.take(MAX_FILE_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| PinsError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_FILE_SIZE) {
        return Err(invalid(path, "larger than 1 MiB"));
    }
    let text = String::from_utf8(bytes).map_err(|_| invalid(path, "not valid UTF-8"))?;
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| {
        let line = error
            .span()
            .map(|span| format!("line {}: ", text[..span.start].matches('\n').count() + 1))
            .unwrap_or_default();
        invalid(path, format!("{line}{}", error.message()))
    })?;
    let Some(value) = table.get("pinned") else {
        return Ok(Some(Vec::new()));
    };
    let toml::Value::Array(entries) = value else {
        return Err(invalid(path, "`pinned` must be an array of strings"));
    };
    let mut pinned: Vec<RepoName> = Vec::new();
    for entry in entries {
        let toml::Value::String(text) = entry else {
            return Err(invalid(path, "`pinned` must be an array of strings"));
        };
        let name: RepoName = text
            .parse()
            .map_err(|_| invalid(path, format!("invalid repo name {text:?}")))?;
        if !pinned.iter().any(|known| known.same_repo(&name)) {
            pinned.push(name);
        }
    }
    Ok(Some(pinned))
}

/// Rewrites the file atomically and durably (tmp file in the same directory, written and synced, then renamed).
fn write_file(path: &Path, list: &[RepoName]) -> Result<(), PinsError> {
    let wrap = |source| PinsError::Write {
        path: path.to_path_buf(),
        source,
    };
    let entries: Vec<String> = list.iter().map(|name| format!("\"{name}\"")).collect();
    let content = format!("{HEADER}pinned = [{}]\n", entries.join(", "));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(wrap)?;
    }
    let tmp = path.with_file_name(format!("{FILE_NAME}.tmp"));
    let result = write_synced(&tmp, content.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(error) = result {
        // Best effort: the write error is the one worth reporting, a leftover tmp file is harmless.
        let _ = std::fs::remove_file(&tmp);
        return Err(wrap(error));
    }
    Ok(())
}

/// Creates `path` with `bytes`, flushed to disk before the caller renames it over the real file.
fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
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
        let Some(config_dir) = config_dir else {
            return (Self::in_memory(), None);
        };
        let path = pinned_repos_path(config_dir);
        match read_file(&path) {
            Ok(None) => (
                Self {
                    pinned: Vec::new(),
                    mode: Mode::Saved(path),
                },
                None,
            ),
            Ok(Some(pinned)) => (
                Self {
                    pinned,
                    mode: Mode::Saved(path),
                },
                None,
            ),
            Err(error) => (
                Self {
                    pinned: Vec::new(),
                    mode: Mode::Disabled,
                },
                Some(error),
            ),
        }
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
        let mut updated = self.pinned.clone();
        let result = match updated.iter().position(|pinned| pinned.same_repo(&name)) {
            None => {
                updated.push(name);
                true
            }
            Some(index) if updated[index] == name => {
                // Already pinned under the same spelling: nothing to write.
                self.check_enabled()?;
                return Ok(false);
            }
            Some(index) => {
                updated[index] = name;
                false
            }
        };
        self.save(&updated)?;
        self.pinned = updated;
        Ok(result)
    }

    /// Unpins `name` ([`RepoName::same_repo`]) and saves. `Ok(false)` (and no write) if it was not pinned.
    ///
    /// Errors: [`PinsError::Disabled`]; [`PinsError::Write`], in which case the store is unchanged.
    pub(crate) fn unpin(&mut self, name: &RepoName) -> Result<bool, PinsError> {
        let Some(index) = self.pinned.iter().position(|pinned| pinned.same_repo(name)) else {
            self.check_enabled()?;
            return Ok(false);
        };
        let mut updated = self.pinned.clone();
        updated.remove(index);
        self.save(&updated)?;
        self.pinned = updated;
        Ok(true)
    }

    fn check_enabled(&self) -> Result<(), PinsError> {
        match self.mode {
            Mode::Disabled => Err(PinsError::Disabled),
            Mode::Memory | Mode::Saved(_) => Ok(()),
        }
    }

    /// Saves `list` in `Saved` mode (nothing to do in memory, refused when disabled).
    fn save(&self, list: &[RepoName]) -> Result<(), PinsError> {
        match &self.mode {
            Mode::Memory => Ok(()),
            Mode::Disabled => Err(PinsError::Disabled),
            Mode::Saved(path) => write_file(path, list),
        }
    }

    /// The file changes are saved to; `None` in memory or disabled mode. Only the unit tests need it.
    #[cfg(test)]
    pub(crate) fn path(&self) -> Option<&Path> {
        match &self.mode {
            Mode::Saved(path) => Some(path),
            Mode::Memory | Mode::Disabled => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "# Pinned repos of ghu (owner/name), in pin order. Rewritten by ghu on each pin and unpin.\n";

    fn repo(text: &str) -> RepoName {
        text.parse().unwrap()
    }

    fn names(store: &PinStore) -> Vec<String> {
        store.pinned().iter().map(ToString::to_string).collect()
    }

    /// A config dir whose `ghu/pinned.toml` holds `content`.
    fn dir_with(content: impl AsRef<[u8]>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = pinned_repos_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        dir
    }

    fn assert_invalid_then_disabled(content: impl AsRef<[u8]>, expect_reason: &str) {
        let content = content.as_ref().to_vec();
        let dir = dir_with(&content);
        let path = pinned_repos_path(dir.path());
        let (mut store, error) = PinStore::load(Some(dir.path()));
        match error {
            Some(PinsError::Invalid { path: p, reason }) => {
                assert_eq!(p, path);
                assert!(reason.contains(expect_reason), "reason: {reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert!(store.pinned().is_empty());
        assert!(store.path().is_none());
        assert!(matches!(store.pin(repo("a/b")), Err(PinsError::Disabled)));
        assert!(matches!(
            store.unpin(&repo("a/b")),
            Err(PinsError::Disabled)
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            content,
            "file must stay intact"
        );
    }

    #[test]
    fn no_config_dir_is_in_memory() {
        let (mut store, error) = PinStore::load(None);
        assert!(error.is_none());
        assert!(store.path().is_none());
        assert!(store.pin(repo("a/b")).unwrap());
        assert_eq!(names(&store), ["a/b"]);
    }

    #[test]
    fn missing_directory_is_empty_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert!(store.pinned().is_empty());
        assert_eq!(store.path(), Some(pinned_repos_path(dir.path()).as_path()));
        assert!(!dir.path().join(APP_DIR).exists());
    }

    #[test]
    fn missing_file_in_existing_directory_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(APP_DIR)).unwrap();
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert!(store.pinned().is_empty());
    }

    #[test]
    fn missing_pinned_key_and_unknown_keys() {
        let dir = dir_with("other = 1\n");
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert!(store.pinned().is_empty());
    }

    #[test]
    fn unknown_keys_are_ignored_and_dropped_at_next_write() {
        let dir = dir_with("extra = 'x'\npinned = [\"a/b\"]\n");
        let (mut store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert_eq!(names(&store), ["a/b"]);
        assert!(store.pin(repo("c/d")).unwrap());
        let text = std::fs::read_to_string(pinned_repos_path(dir.path())).unwrap();
        assert_eq!(text, format!("{HEADER}pinned = [\"a/b\", \"c/d\"]\n"));
    }

    #[test]
    fn round_trip_with_exact_header_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = pinned_repos_path(dir.path());
        let (mut store, _) = PinStore::load(Some(dir.path()));
        assert!(store.pin(repo("octo-org/hello-world")).unwrap());
        assert!(store.pin(repo("bob/tools")).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{HEADER}pinned = [\"octo-org/hello-world\", \"bob/tools\"]\n")
        );
        let (reloaded, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert_eq!(names(&reloaded), ["octo-org/hello-world", "bob/tools"]);
    }

    #[test]
    fn duplicate_in_file_is_dropped_case_insensitively() {
        let dir = dir_with("pinned = [\"Bob/Tools\", \"a/b\", \"bob/tools\"]\n");
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert_eq!(names(&store), ["Bob/Tools", "a/b"]);
    }

    #[test]
    fn pin_duplicate_is_false_without_write_but_canonical_name_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = pinned_repos_path(dir.path());
        let (mut store, _) = PinStore::load(Some(dir.path()));
        assert!(store.pin(repo("bob/tools")).unwrap());
        // Delete the file: a write would bring it back.
        std::fs::remove_file(&path).unwrap();
        assert!(!store.pin(repo("bob/tools")).unwrap());
        assert!(!path.exists(), "no write expected");

        assert!(!store.pin(repo("Bob/Tools")).unwrap());
        assert_eq!(names(&store), ["Bob/Tools"]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, format!("{HEADER}pinned = [\"Bob/Tools\"]\n"));
        assert!(store.is_pinned(&repo("BOB/TOOLS")));
    }

    #[test]
    fn unpin_removes_saves_and_reports_absence() {
        let dir = tempfile::tempdir().unwrap();
        let path = pinned_repos_path(dir.path());
        let (mut store, _) = PinStore::load(Some(dir.path()));
        store.pin(repo("a/b")).unwrap();
        store.pin(repo("c/d")).unwrap();
        assert!(store.unpin(&repo("A/B")).unwrap());
        assert_eq!(names(&store), ["c/d"]);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{HEADER}pinned = [\"c/d\"]\n")
        );
        let before = std::fs::read(&path).unwrap();
        assert!(!store.unpin(&repo("a/b")).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(store.unpin(&repo("c/d")).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{HEADER}pinned = []\n")
        );
    }

    #[test]
    fn invalid_toml_disables() {
        assert_invalid_then_disabled("pinned = [\n", "line");
    }

    #[test]
    fn invalid_entry_disables() {
        assert_invalid_then_disabled("pinned = [\"a/b/c\"]\n", "a/b/c");
    }

    #[test]
    fn non_string_entry_disables() {
        assert_invalid_then_disabled("pinned = [1]\n", "array of strings");
    }

    #[test]
    fn pinned_not_an_array_disables() {
        assert_invalid_then_disabled("pinned = \"a/b\"\n", "array");
    }

    #[test]
    fn not_utf8_disables() {
        assert_invalid_then_disabled([0xff, 0xfe, 0x00], "UTF-8");
    }

    #[test]
    fn file_over_one_mib_disables() {
        let mut content = b"# ".to_vec();
        content.resize(1024 * 1024 + 1, b'x');
        assert_invalid_then_disabled(content, "1 MiB");
    }

    #[test]
    fn file_of_exactly_one_mib_is_read() {
        let mut content = b"# ".to_vec();
        content.resize(1024 * 1024 - 1, b'x');
        content.push(b'\n');
        let dir = dir_with(&content);
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        assert!(store.path().is_some());
    }

    #[test]
    fn unreadable_file_is_read_error() {
        // A directory where the file should be: reading fails with an I/O error.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(pinned_repos_path(dir.path())).unwrap();
        let (store, error) = PinStore::load(Some(dir.path()));
        assert!(matches!(error, Some(PinsError::Read { .. })), "{error:?}");
        assert!(store.path().is_none());
    }

    #[test]
    fn write_failure_leaves_store_unchanged_and_no_tmp() {
        // A file named `ghu` appears after loading: creating the directory fails.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join(APP_DIR);
        let (mut store, error) = PinStore::load(Some(dir.path()));
        assert!(error.is_none());
        std::fs::write(&fake, "x").unwrap();
        let result = store.pin(repo("a/b"));
        assert!(matches!(result, Err(PinsError::Write { .. })), "{result:?}");
        assert!(store.pinned().is_empty());
        assert_eq!(std::fs::read_to_string(&fake).unwrap(), "x");
    }

    #[test]
    fn rename_failure_keeps_store_and_removes_tmp() {
        // A directory named pinned.toml: the rename over it fails after the tmp file was written.
        let dir = tempfile::tempdir().unwrap();
        let path = pinned_repos_path(dir.path());
        let (mut store, _) = PinStore::load(Some(dir.path()));
        std::fs::create_dir_all(&path).unwrap();
        let result = store.pin(repo("a/b"));
        assert!(matches!(result, Err(PinsError::Write { .. })), "{result:?}");
        assert!(store.pinned().is_empty());
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, [std::ffi::OsString::from(FILE_NAME)]);
    }

    #[test]
    fn no_tmp_left_after_success() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = PinStore::load(Some(dir.path()));
        store.pin(repo("a/b")).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join(APP_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, [std::ffi::OsString::from(FILE_NAME)]);
    }
}
