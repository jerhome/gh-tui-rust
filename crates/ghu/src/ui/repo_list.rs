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
//! What is here now (architecte): #2's behaviour (own repos only, no selection marker beyond a `>` prefix) and
//! `Enter` → PR list, so dev[tui-pulls] can test #5. Private fields and helpers are free for dev[tui-repos].

use gh_client::{Affiliation, Repo};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::keys::{self, Binding, HelpRow};
use super::{Flow, Outcome, RequestId, Shared, View, error_message};
use crate::pins::{PinStore, PinsError};
use crate::{KeyCode, KeyEvent, KeyModifiers};

/// Actions of this screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Open,
    Up,
    Down,
}

/// Key table of this screen (help order).
const TABLE: [Binding<Action>; 3] = [
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
];

/// What the list shows.
enum Content {
    /// The first answer has not come yet.
    Loading,
    /// The repos, in GitHub's order.
    Loaded(Vec<Repo>),
    /// The request failed: a message for the user.
    Failed(String),
}

/// The Repo list screen.
pub(crate) struct RepoListScreen {
    content: Content,
    selected: usize,
    pending: Option<RequestId>,
    #[allow(dead_code)] // used by #4 (dev[tui-repos])
    pins: PinStore,
}

impl RepoListScreen {
    /// Builds the screen and starts loading at once (affiliated repos and, from #4, each pinned repo).
    /// `pins` is the store loaded by [`crate::prepare`]; `pins_problem` its load error, shown as the first notice.
    pub(crate) fn new(
        shared: &mut Shared,
        pins: PinStore,
        pins_problem: Option<PinsError>,
    ) -> Self {
        if let Some(problem) = pins_problem {
            shared.notify(super::Notice::Error(problem.to_string()));
        }
        let mut screen = Self {
            content: Content::Loading,
            selected: 0,
            pending: None,
            pins,
        };
        screen.load(shared);
        screen
    }

    fn load(&mut self, shared: &mut Shared) {
        let client = shared.client().clone();
        self.pending =
            Some(shared.spawn(async move {
                Outcome::Repos(client.user_repos(&[Affiliation::Owner]).await)
            }));
    }

    fn visible(&self) -> &[Repo] {
        match &self.content {
            Content::Loaded(repos) => repos,
            Content::Loading | Content::Failed(_) => &[],
        }
    }
}

impl View for RepoListScreen {
    fn title(&self) -> String {
        "Repo list".to_owned()
    }

    fn help(&self) -> Vec<HelpRow> {
        keys::help_rows(&TABLE)
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        let count = self.visible().len();
        match keys::lookup(&TABLE, key) {
            Some(Action::Up) => {
                self.selected = self.selected.saturating_sub(1);
                Flow::Handled
            }
            Some(Action::Down) => {
                if self.selected + 1 < count {
                    self.selected += 1;
                }
                Flow::Handled
            }
            Some(Action::Open) => match self.visible().get(self.selected) {
                Some(repo) => Flow::Push(Box::new(super::pulls::PullsScreen::new(
                    repo.clone(),
                    shared,
                ))),
                None => Flow::Handled,
            },
            None => Flow::Ignored,
        }
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, _shared: &mut Shared) -> Option<Outcome> {
        if self.pending != Some(id) {
            return Some(outcome);
        }
        self.pending = None;
        self.content = match outcome {
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
        self.selected = 0;
        None
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let lines: Vec<Line<'_>> = match &self.content {
            Content::Loading => Vec::new(),
            Content::Loaded(repos) if repos.is_empty() => vec![Line::from(" No repos.")],
            Content::Loaded(repos) => repos
                .iter()
                .enumerate()
                .map(|(i, repo)| repo_line(repo, i == self.selected))
                .collect(),
            Content::Failed(message) => vec![Line::from(format!(" {message}")).red()],
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered())
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}

/// One row of the Repo list: selection marker, name, markers, description.
fn repo_line(repo: &Repo, selected: bool) -> Line<'_> {
    let mut spans = vec![Span::styled(
        format!("{}{}", if selected { ">" } else { " " }, repo.name),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    for (flag, label) in [
        (repo.fork, "fork"),
        (repo.private, "private"),
        (repo.archived, "archived"),
    ] {
        if flag {
            spans.push(Span::raw(format!(" [{label}]")).dim());
        }
    }
    if let Some(description) = &repo.description {
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
