//! The Repo list screen, root of the stack (#2; #3 complete list and filters; #4 Pinned repos).
//!
//! Contract for dev[tui-repos] (ADR 0002 § Repo list). Behaviour, all observable through the harness:
//!
//! - **Content** (#3): `Client::user_repos(&[])` (owner, collaborator, organization member, every page), plus the
//!   Pinned repos of the [`PinStore`], each loaded with `Client::repo` (one request per pinned repo, in parallel).
//!   Order: Pinned repos first, in pin order, then the others in GitHub's order. A repo is shown once
//!   ([`gh_client::RepoName::same_repo`]): a pinned repo that is also affiliated appears among the pinned.
//! - **Row**: `owner/name`, then the markers `[pinned]`, `[fork]`, `[private]`, `[archived]` (in that order, only
//!   those that apply), then the description. A pinned repo whose loading failed shows `owner/name [pinned]` and,
//!   dimmed, `not found` (404) or `unavailable` (any other error); it can still be unpinned.
//! - **Archived repos** (#3): hidden by default, except Pinned repos (always shown, marked). `a` toggles showing
//!   them; while shown, the header title is `Repo list (archived shown)`.
//! - **Filter by name** (#3): `/` starts typing a filter (editing); characters and paste append, `Backspace`
//!   deletes, `Enter` keeps the filter and stops editing, `Esc` clears it and stops editing. Rows whose
//!   `owner/name` contains the filter, ASCII case-insensitively, are shown. The filter is shown at the top of the
//!   body as `Filter: <text>` (with a cursor while editing). A kept filter is cleared by `Esc` (the screen
//!   handles it, so `Esc` does not reach the global table). No row: `No repo matches "<text>".` (and when the
//!   account has no repo at all: `No repos.`).
//! - **Selection**: `Up`/`k`, `Down`/`j` move a highlighted row (bounded, no wrap), kept on the same repo when
//!   the filter or archived toggle changes if it is still visible, else on the first row. `Enter` opens the
//!   selected repo's PRs ([`super::pulls::PullsScreen::new`]); on a pinned row that failed to load, it shows the
//!   error notice `Cannot open <owner/name>: …` instead.
//! - **Open `owner/name`** (#4): `o` opens a prompt line `Open repo (owner/name): <text>` at the top of the body
//!   (editing). Characters and paste (first line, trimmed) append, `Backspace` deletes, `Esc` cancels. `Enter`:
//!   input that is not a valid [`gh_client::RepoName`] → the prompt stays open with its text and the error notice
//!   `Invalid repo name: type owner/name.`, no request; else `Client::repo` runs (Loading shown, prompt stays
//!   open). On success: the repo (GitHub's canonical name) is pinned ([`PinStore::pin`]), the prompt closes, the
//!   list shows it as `[pinned]` (moved to the pinned group if it was affiliated), and its PR list opens
//!   (`Flow::Push`). If saving the file fails, the repo is still opened and the error notice is the
//!   [`PinsError`]'s text. On failure: prompt stays open with its text, error notice
//!   `Cannot open <owner/name>: <reason>` ([`super::error_message`]), nothing pinned.
//! - **Unpin** (#4): `u` on a pinned row unpins it ([`PinStore::unpin`]) and shows `Unpinned <owner/name>.`; the row
//!   disappears, or stays without `[pinned]` (in GitHub's order) if the repo is affiliated. On another row: notice
//!   `<owner/name> is not pinned.`. A save failure keeps the row pinned and shows the [`PinsError`]'s text.
//! - **Pins problem at start-up**: if [`crate::Context`] carries a [`PinsError`] (unreadable or invalid file), its
//!   text is the first notice (error).
//! - **Refresh**: `r` reloads everything (affiliated repos and each pinned repo); answers of older loads are
//!   ignored ([`super::RequestId`]).
//! - **Errors** of the list: as #2 (`Cannot load the repos: …` in red in the body, via [`super::error_message`]).
//!
//! Key table (help order): `Enter` Open the repo's PRs · `Up/k` `Down/j` Move · `/` Filter by name · `a`
//! Show/hide archived · `o` Open owner/name · `u` Unpin · `r` Refresh. No reserved key (`keys::uses_reserved_key`).
//!

use gh_client::{Repo, RepoName};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::keys::{self, Binding, HelpRow};
use super::list::{Content, Selection};
use super::pulls::PullsScreen;
use super::{Flow, Notice, Outcome, RequestId, Shared, View, error_message};
use crate::pins::{PinStore, PinsError};
use crate::{KeyCode, KeyEvent, KeyModifiers};

/// Actions of this screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Open,
    Up,
    Down,
    Filter,
    ToggleArchived,
    OpenByName,
    Unpin,
    Refresh,
}

/// Key table of this screen (help order).
const TABLE: [Binding<Action>; 8] = [
    Binding {
        label: "Enter",
        what: "Open the repo's PRs",
        action: Action::Open,
        keys: &[(KeyCode::Enter, KeyModifiers::NONE)],
    },
    Binding {
        label: "Up/k",
        what: "Previous repo",
        action: Action::Up,
        keys: &[
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Char('k'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Down/j",
        what: "Next repo",
        action: Action::Down,
        keys: &[
            (KeyCode::Down, KeyModifiers::NONE),
            (KeyCode::Char('j'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "/",
        what: "Filter by name",
        action: Action::Filter,
        // Some keyboard layouts need Shift for `/`.
        keys: &[
            (KeyCode::Char('/'), KeyModifiers::NONE),
            (KeyCode::Char('/'), KeyModifiers::SHIFT),
        ],
    },
    Binding {
        label: "a",
        what: "Show/hide archived",
        action: Action::ToggleArchived,
        keys: &[(KeyCode::Char('a'), KeyModifiers::NONE)],
    },
    Binding {
        label: "o",
        what: "Open owner/name",
        action: Action::OpenByName,
        keys: &[(KeyCode::Char('o'), KeyModifiers::NONE)],
    },
    Binding {
        label: "u",
        what: "Unpin",
        action: Action::Unpin,
        keys: &[(KeyCode::Char('u'), KeyModifiers::NONE)],
    },
    Binding {
        label: "r",
        what: "Refresh",
        action: Action::Refresh,
        keys: &[(KeyCode::Char('r'), KeyModifiers::NONE)],
    },
];

/// What is known of one Pinned repo.
enum PinLoad {
    /// Its `Client::repo` answer has not come yet.
    Loading,
    Loaded(Repo),
    /// Loading failed: `not_found` for a 404; `message` is the `Cannot open …` notice for `Enter`.
    Failed {
        not_found: bool,
        message: String,
    },
}

/// A Pinned repo of the store with its loading state. `name` is the store's spelling.
struct PinEntry {
    name: RepoName,
    load: PinLoad,
    /// The request whose answer is awaited, if any.
    pending: Option<RequestId>,
}

/// The text field that has the focus, if any.
enum Focus {
    None,
    Filter,
    Prompt(Prompt),
}

/// The `owner/name` prompt: its text, and the request in flight with the name that was sent (what the user
/// typed since does not change what a failure reports).
struct Prompt {
    text: String,
    pending: Option<(RequestId, RepoName)>,
}

/// One visible row.
struct Row<'a> {
    name: &'a RepoName,
    repo: Option<&'a Repo>,
    pin: Option<&'a PinEntry>,
}

/// The Repo list screen.
pub(crate) struct RepoListScreen {
    /// The affiliated repos (`Client::user_repos`), in GitHub's order.
    content: Content<Repo>,
    pending: Option<RequestId>,
    pins: PinStore,
    /// One entry per Pinned repo of `pins`, same order.
    pin_entries: Vec<PinEntry>,
    show_archived: bool,
    filter: String,
    focus: Focus,
    /// Index into the visible rows.
    selected: Selection,
}

impl RepoListScreen {
    /// Builds the screen and starts loading at once (affiliated repos and each pinned repo).
    /// `pins` is the store loaded by [`crate::prepare`]; `pins_problem` its load error, shown as the first notice.
    pub(crate) fn new(
        shared: &mut Shared,
        pins: PinStore,
        pins_problem: Option<PinsError>,
    ) -> Self {
        if let Some(problem) = pins_problem {
            shared.notify(Notice::Error(problem.to_string()));
        }
        let pin_entries = pins
            .pinned()
            .iter()
            .map(|name| PinEntry {
                name: name.clone(),
                load: PinLoad::Loading,
                pending: None,
            })
            .collect();
        let mut screen = Self {
            content: Content::Loading,
            pending: None,
            pins,
            pin_entries,
            show_archived: false,
            filter: String::new(),
            focus: Focus::None,
            selected: Selection::default(),
        };
        screen.load(shared);
        screen
    }

    /// (Re)loads the affiliated repos and every pinned repo; older answers become stale.
    fn load(&mut self, shared: &mut Shared) {
        let client = shared.client().clone();
        self.pending =
            Some(shared.spawn(async move { Outcome::Repos(client.user_repos(&[]).await) }));
        for entry in &mut self.pin_entries {
            let client = shared.client().clone();
            let name = entry.name.clone();
            entry.pending =
                Some(shared.spawn(async move { Outcome::Repo(client.repo(&name).await) }));
        }
    }

    /// The visible rows: Pinned repos in pin order, then the affiliated ones not pinned, in GitHub's order;
    /// archived ones hidden unless pinned or shown; filtered by name.
    fn rows(&self) -> Vec<Row<'_>> {
        let affiliated = self.content.items();
        let pinned = self.pin_entries.iter().map(|entry| match &entry.load {
            PinLoad::Loaded(repo) => Row {
                name: &repo.name,
                repo: Some(repo),
                pin: Some(entry),
            },
            PinLoad::Loading | PinLoad::Failed { .. } => Row {
                name: &entry.name,
                repo: None,
                pin: Some(entry),
            },
        });
        let others = affiliated
            .iter()
            .filter(|repo| !self.pins.is_pinned(&repo.name))
            .filter(|repo| self.show_archived || !repo.archived)
            .map(|repo| Row {
                name: &repo.name,
                repo: Some(repo),
                pin: None,
            });
        let needle = self.filter.to_ascii_lowercase();
        pinned
            .chain(others)
            .filter(|row| {
                needle.is_empty() || row.name.to_string().to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    fn selected_name(&self) -> Option<RepoName> {
        self.rows()
            .get(self.selected.index())
            .map(|row| row.name.clone())
    }

    /// After the rows changed: keeps the selection on `previous` if still visible, else the first row.
    fn reselect(&mut self, previous: Option<RepoName>) {
        let rows = self.rows();
        self.selected.set(
            previous
                .and_then(|name| rows.iter().position(|row| row.name.same_repo(&name)))
                .unwrap_or(0),
        );
    }

    /// Applies `change` to the screen, keeping the selection on the same repo if it stays visible.
    fn keeping_selection(&mut self, change: impl FnOnce(&mut Self)) {
        let previous = self.selected_name();
        change(self);
        self.reselect(previous);
    }

    /// `Enter` on the Repo list: the selected repo's PRs, or the error of a pinned repo that failed to load.
    fn open_selected(&self, shared: &mut Shared) -> Flow {
        let rows = self.rows();
        let Some(row) = rows.get(self.selected.index()) else {
            return Flow::Handled;
        };
        if let Some(repo) = row.repo {
            return Flow::Push(Box::new(super::pulls::PullsScreen::new(
                repo.clone(),
                shared,
            )));
        }
        if let Some(PinEntry {
            load: PinLoad::Failed { message, .. },
            ..
        }) = row.pin
        {
            shared.notify(Notice::Error(message.clone()));
        }
        Flow::Handled
    }

    fn unpin_selected(&mut self, shared: &mut Shared) {
        let rows = self.rows();
        let Some(row) = rows.get(self.selected.index()) else {
            return;
        };
        let name = row.name.clone();
        if row.pin.is_none() {
            shared.notify(Notice::Info(format!("{name} is not pinned.")));
            return;
        }
        match self.pins.unpin(&name) {
            Ok(_) => {
                self.keeping_selection(|screen| {
                    screen
                        .pin_entries
                        .retain(|entry| !entry.name.same_repo(&name));
                });
                shared.notify(Notice::Info(format!("Unpinned {name}.")));
            }
            Err(error) => shared.notify(Notice::Error(error.to_string())),
        }
    }

    /// `Enter` in the prompt: validates the name, then asks GitHub for it.
    fn submit_prompt(&mut self, shared: &mut Shared) {
        let Focus::Prompt(prompt) = &mut self.focus else {
            return;
        };
        let Ok(name) = prompt.text.parse::<RepoName>() else {
            shared.notify(Notice::Error(
                "Invalid repo name: type owner/name.".to_owned(),
            ));
            return;
        };
        let client = shared.client().clone();
        let sent = name.clone();
        let id = shared.spawn(async move { Outcome::Repo(client.repo(&name).await) });
        prompt.pending = Some((id, sent));
    }

    /// A repo opened by name was found: pin it under GitHub's name and show it among the pinned.
    fn opened(&mut self, repo: Repo, shared: &mut Shared) {
        self.focus = Focus::None;
        let name = repo.name.clone();
        let opened_repo = repo.clone();
        match self.pins.pin(name.clone()) {
            Ok(_) => {
                self.keeping_selection(|screen| {
                    match screen
                        .pin_entries
                        .iter_mut()
                        .find(|entry| entry.name.same_repo(&name))
                    {
                        Some(entry) => {
                            entry.name = name.clone();
                            entry.load = PinLoad::Loaded(repo);
                            entry.pending = None;
                        }
                        None => screen.pin_entries.push(PinEntry {
                            name: name.clone(),
                            load: PinLoad::Loaded(repo),
                            pending: None,
                        }),
                    }
                });
                shared.notify(Notice::Info(format!("Pinned {name}.")));
            }
            Err(error) => shared.notify(Notice::Error(error.to_string())),
        }
        let pulls = PullsScreen::new(opened_repo, shared);
        shared.push(Box::new(pulls));
    }

    /// Keys while a text field has the focus.
    fn edit_key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match (&mut self.focus, key.code) {
            (Focus::Filter, KeyCode::Enter) => self.focus = Focus::None,
            (Focus::Filter, KeyCode::Esc) => {
                self.focus = Focus::None;
                self.keeping_selection(|screen| screen.filter.clear());
            }
            (Focus::Filter, KeyCode::Backspace) => {
                self.keeping_selection(|screen| {
                    screen.filter.pop();
                });
            }
            (Focus::Filter, KeyCode::Char(c)) if is_text(key) => {
                self.keeping_selection(|screen| screen.filter.push(c));
            }
            (Focus::Prompt(_), KeyCode::Enter) => self.submit_prompt(shared),
            // Closing the prompt drops the request in flight: its answer is stale.
            (Focus::Prompt(_), KeyCode::Esc) => self.focus = Focus::None,
            (Focus::Prompt(prompt), KeyCode::Backspace) => {
                prompt.text.pop();
            }
            (Focus::Prompt(prompt), KeyCode::Char(c)) if is_text(key) => prompt.text.push(c),
            _ => {}
        }
        Flow::Handled
    }

    /// Lines above the rows: the prompt, then the filter.
    fn field_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();
        if let Focus::Prompt(prompt) = &self.focus {
            lines.push(Line::from(format!(
                " Open repo (owner/name): {}",
                prompt.text
            )));
        }
        if matches!(self.focus, Focus::Filter) || !self.filter.is_empty() {
            lines.push(Line::from(format!(" Filter: {}", self.filter)));
        }
        lines
    }
}

/// A key that types its character: no modifier but Shift.
fn is_text(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT
}

impl View for RepoListScreen {
    fn title(&self) -> String {
        if self.show_archived {
            "Repo list (archived shown)".to_owned()
        } else {
            "Repo list".to_owned()
        }
    }

    fn help(&self) -> Vec<HelpRow> {
        keys::help_rows(&TABLE)
    }

    fn editing(&self) -> bool {
        !matches!(self.focus, Focus::None)
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        if self.editing() {
            return self.edit_key(key, shared);
        }
        let count = self.rows().len();
        match keys::lookup(&TABLE, key) {
            Some(Action::Up) => self.selected.up(),
            Some(Action::Down) => self.selected.down(count),
            Some(Action::Open) => return self.open_selected(shared),
            Some(Action::Filter) => self.focus = Focus::Filter,
            Some(Action::ToggleArchived) => {
                self.keeping_selection(|screen| screen.show_archived = !screen.show_archived);
            }
            Some(Action::OpenByName) => {
                self.focus = Focus::Prompt(Prompt {
                    text: String::new(),
                    pending: None,
                });
            }
            Some(Action::Unpin) => self.unpin_selected(shared),
            Some(Action::Refresh) => self.load(shared),
            None if key.code == KeyCode::Esc && !self.filter.is_empty() => {
                self.keeping_selection(|screen| screen.filter.clear());
            }
            None => return Flow::Ignored,
        }
        Flow::Handled
    }

    fn paste(&mut self, text: &str, _shared: &mut Shared) {
        let first = text.lines().next().unwrap_or_default();
        match &mut self.focus {
            Focus::Filter => self.keeping_selection(|screen| screen.filter.push_str(first)),
            Focus::Prompt(prompt) => prompt.text.push_str(first.trim()),
            Focus::None => {}
        }
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, shared: &mut Shared) -> Option<Outcome> {
        if self.pending == Some(id) {
            let content = match outcome {
                Outcome::Repos(Ok(repos)) => {
                    tracing::debug!(count = repos.len(), "repos loaded");
                    Content::Loaded(repos)
                }
                Outcome::Repos(Err(error)) => {
                    tracing::warn!(%error, "loading repos failed");
                    Content::Failed(error_message("load the repos", &error))
                }
                Outcome::TaskFailed => Content::Failed(super::TASK_FAILED.to_owned()),
                other => return Some(other),
            };
            self.pending = None;
            self.keeping_selection(|screen| screen.content = content);
            return None;
        }
        let sent = match &mut self.focus {
            Focus::Prompt(prompt)
                if prompt
                    .pending
                    .as_ref()
                    .is_some_and(|(request, _)| *request == id) =>
            {
                prompt.pending.take().map(|(_, name)| name)
            }
            _ => None,
        };
        if let Some(sent) = sent {
            match outcome {
                Outcome::Repo(Ok(repo)) => self.opened(repo, shared),
                Outcome::Repo(Err(error)) => {
                    tracing::warn!(%error, "opening a repo failed");
                    shared.notify(Notice::Error(error_message(
                        &format!("open {sent}"),
                        &error,
                    )));
                }
                Outcome::TaskFailed => shared.notify(Notice::Error(super::TASK_FAILED.to_owned())),
                other => return Some(other),
            }
            return None;
        }
        let Some(index) = self
            .pin_entries
            .iter()
            .position(|entry| entry.pending == Some(id))
        else {
            return Some(outcome);
        };
        let name = self.pin_entries[index].name.clone();
        let load = match outcome {
            Outcome::Repo(Ok(repo)) => PinLoad::Loaded(repo),
            Outcome::Repo(Err(error)) => {
                tracing::warn!(%error, "loading a pinned repo failed");
                PinLoad::Failed {
                    not_found: matches!(error, gh_client::Error::NotFound),
                    message: error_message(&format!("open {name}"), &error),
                }
            }
            Outcome::TaskFailed => PinLoad::Failed {
                not_found: false,
                message: super::TASK_FAILED.to_owned(),
            },
            other => return Some(other),
        };
        // A repo loaded under another case than the file's: write GitHub's canonical name back.
        let mut canonical = None;
        if let PinLoad::Loaded(repo) = &load
            && repo.name != name
        {
            match self.pins.pin(repo.name.clone()) {
                Ok(_) => canonical = Some(repo.name.clone()),
                Err(error) => shared.notify(Notice::Error(error.to_string())),
            }
        }
        self.keeping_selection(|screen| {
            let entry = &mut screen.pin_entries[index];
            entry.pending = None;
            entry.load = load;
            if let Some(canonical) = canonical {
                entry.name = canonical;
            }
        });
        None
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered();
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines = self.field_lines();
        // The focused field: the prompt is the first line, the filter the last.
        let cursor_row = match self.focus {
            Focus::Prompt(_) => Some(0),
            Focus::Filter => lines.len().checked_sub(1),
            Focus::None => None,
        };
        let rows = self.rows();
        let mut wrap = false;
        match &self.content {
            Content::Failed(message) => {
                lines.push(Line::from(format!(" {message}")).red());
                wrap = true;
            }
            Content::Loaded(repos) if repos.is_empty() && self.pin_entries.is_empty() => {
                lines.push(Line::from(" No repos."));
            }
            Content::Loaded(_) if rows.is_empty() && !self.filter.is_empty() => {
                lines.push(Line::from(format!(" No repo matches \"{}\".", self.filter)));
            }
            Content::Loading | Content::Loaded(_) => {}
        }
        // Scroll so the selected row stays visible (rows never wrap).
        let height = usize::from(inner.height).saturating_sub(lines.len()).max(1);
        let offset = self.selected.first_visible(height);
        lines.extend(
            rows.iter()
                .enumerate()
                .skip(offset)
                .take(height)
                .map(|(i, row)| row_line(row, i == self.selected.index())),
        );

        let mut paragraph = Paragraph::new(lines);
        if wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        frame.render_widget(paragraph, inner);

        if let Some(row) = cursor_row {
            let text = self.field_lines().swap_remove(row);
            let x = u16::try_from(text.width()).unwrap_or(u16::MAX);
            let y = u16::try_from(row).unwrap_or(u16::MAX);
            frame.set_cursor_position(Position::new(
                inner
                    .x
                    .saturating_add(x)
                    .min(inner.right().saturating_sub(1)),
                inner.y.saturating_add(y),
            ));
        }
    }
}

/// One row of the Repo list: selection marker, name, markers, description (or the loading problem of a pinned
/// repo).
fn row_line<'a>(row: &Row<'a>, selected: bool) -> Line<'a> {
    let mut spans = vec![Span::styled(
        format!("{}{}", if selected { ">" } else { " " }, row.name),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    let flags = row
        .repo
        .map_or([false; 3], |repo| [repo.fork, repo.private, repo.archived]);
    for (flag, label) in [
        (row.pin.is_some(), "pinned"),
        (flags[0], "fork"),
        (flags[1], "private"),
        (flags[2], "archived"),
    ] {
        if flag {
            spans.push(Span::raw(format!(" [{label}]")).dim());
        }
    }
    if let Some(PinEntry {
        load: PinLoad::Failed { not_found, .. },
        ..
    }) = row.pin
    {
        let what = if *not_found {
            "not found"
        } else {
            "unavailable"
        };
        spans.push(Span::raw(format!("  {what}")).dim());
    } else if let Some(description) = row.repo.and_then(|repo| repo.description.as_deref()) {
        spans.push(Span::raw(format!("  {description}")));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_binds_no_reserved_key() {
        assert!(!keys::uses_reserved_key(&TABLE));
    }
}
