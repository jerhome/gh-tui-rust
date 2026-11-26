//! The PR list of one repo (#5). Contract for dev[tui-pulls] (ADR 0002 § PR list):
//!
//! - Opened by `Enter` on the Repo list (or after opening `owner/name`): [`PullsScreen::new`] starts loading the
//!   open PRs at once (`Client::pulls(repo, PullFilter::Open)`). Header title:
//!   `<owner/name> · Pull requests (open)`, or `… (closed / merged)` after the toggle.
//! - **Row**: `#<number>`, the state word `open`, `draft` (open Draft PR), `closed` or `merged`, the title, `@<author>`,
//!   and the update date `YYYY-MM-DD` ([`gh_client::Timestamp::date`], UTC), in GitHub's order (most recently
//!   updated first). The title is cut to fit the width (never wraps onto a second row).
//! - **Empty**: `No open pull requests.` / `No closed or merged pull requests.`
//! - **Toggle** `Tab`: open ↔ closed / merged ([`gh_client::PullFilter::Closed`]); the list is reloaded each time
//!   (no cache), the previous rows stay visible until the answer comes, and only the answer to the latest request
//!   is applied ([`super::RequestId`]).
//! - **Refresh** `r`: reloads the current filter, same rules.
//! - **Selection**: `Up`/`k`, `Down`/`j` (bounded); the list scrolls to keep it visible. `Enter` is reserved
//!   for the PR detail (#6) and does nothing yet (`Flow::Handled`, no notice).
//! - **Back**: `Esc` (global) returns to the Repo list, unchanged (selection, filter). Answers that arrive after
//!   leaving are dropped by the stack.
//! - **Errors**: in red in the body, `Cannot load the pull requests: <reason>` ([`super::error_message`]);
//!   [`super::TASK_FAILED`] for a failed task. `r` retries.
//!
//! Key table (help order): `Tab` Open / closed+merged · `r` Refresh · `Up/k` `Down/j` Move. No reserved key.
//!

use gh_client::{PrState, PullFilter, PullRequest, Repo};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::keys::{self, Binding, HelpRow};
use super::{Flow, Outcome, RequestId, Shared, View, error_message};
use crate::{KeyCode, KeyEvent, KeyModifiers};

/// Actions of this screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Toggle,
    Refresh,
    Up,
    Down,
}

/// Key table of this screen (help order).
const TABLE: [Binding<Action>; 4] = [
    Binding {
        label: "Tab",
        what: "Open / closed + merged",
        action: Action::Toggle,
        keys: &[(KeyCode::Tab, KeyModifiers::NONE)],
    },
    Binding {
        label: "r",
        what: "Refresh",
        action: Action::Refresh,
        keys: &[(KeyCode::Char('r'), KeyModifiers::NONE)],
    },
    Binding {
        label: "Up/k",
        what: "Previous PR",
        action: Action::Up,
        keys: &[
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Char('k'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Down/j",
        what: "Next PR",
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
    /// The PRs of the current filter, in GitHub's order.
    Loaded(Vec<PullRequest>),
    /// The request failed: a message for the user.
    Failed(String),
}

/// The PR list screen of one repo.
pub(crate) struct PullsScreen {
    repo: Repo,
    filter: PullFilter,
    content: Content,
    selected: usize,
    /// The latest request; only its answer is applied.
    pending: Option<RequestId>,
}

impl PullsScreen {
    /// Builds the screen for `repo` and starts loading its open PRs.
    pub(crate) fn new(repo: Repo, shared: &mut Shared) -> Self {
        let mut screen = Self {
            repo,
            filter: PullFilter::Open,
            content: Content::Loading,
            selected: 0,
            pending: None,
        };
        screen.load(shared);
        screen
    }

    /// Requests the PRs of the current filter; the rows already shown stay until the answer comes.
    fn load(&mut self, shared: &mut Shared) {
        let client = shared.client().clone();
        let name = self.repo.name.clone();
        let filter = self.filter;
        self.pending =
            Some(shared.spawn(async move { Outcome::Pulls(client.pulls(&name, filter).await) }));
    }

    fn count(&self) -> usize {
        match &self.content {
            Content::Loaded(pulls) => pulls.len(),
            _ => 0,
        }
    }
}

impl View for PullsScreen {
    /// `<owner/name> · Pull requests (open)`, or `(closed / merged)` after `Tab`.
    fn title(&self) -> String {
        let which = match self.filter {
            PullFilter::Open => "open",
            PullFilter::Closed => "closed / merged",
        };
        format!("{} · Pull requests ({which})", self.repo.name)
    }

    fn help(&self) -> Vec<HelpRow> {
        keys::help_rows(&TABLE)
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE {
            // Reserved for the PR detail (#6): nothing yet.
            return Flow::Handled;
        }
        match keys::lookup(&TABLE, key) {
            Some(Action::Toggle) => {
                self.filter = match self.filter {
                    PullFilter::Open => PullFilter::Closed,
                    PullFilter::Closed => PullFilter::Open,
                };
                self.selected = 0;
                self.load(shared);
                Flow::Handled
            }
            Some(Action::Refresh) => {
                self.load(shared);
                Flow::Handled
            }
            Some(Action::Up) => {
                self.selected = self.selected.saturating_sub(1);
                Flow::Handled
            }
            Some(Action::Down) => {
                if self.selected + 1 < self.count() {
                    self.selected += 1;
                }
                Flow::Handled
            }
            None => Flow::Ignored,
        }
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, _shared: &mut Shared) -> Option<Outcome> {
        if self.pending != Some(id) {
            return Some(outcome);
        }
        self.pending = None;
        self.content = match outcome {
            Outcome::Pulls(Ok(pulls)) => {
                tracing::debug!(count = pulls.len(), "pull requests loaded");
                Content::Loaded(pulls)
            }
            Outcome::Pulls(Err(error)) => {
                tracing::warn!(%error, "loading pull requests failed");
                Content::Failed(error_message("load the pull requests", &error))
            }
            Outcome::TaskFailed => Content::Failed(super::TASK_FAILED.to_owned()),
            other => return Some(other),
        };
        self.selected = self.selected.min(self.count().saturating_sub(1));
        None
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width.saturating_sub(2));
        let height = usize::from(area.height.saturating_sub(2)).max(1);
        let lines: Vec<Line<'_>> = match &self.content {
            Content::Loading => Vec::new(),
            Content::Loaded(pulls) if pulls.is_empty() => vec![Line::from(match self.filter {
                PullFilter::Open => " No open pull requests.",
                PullFilter::Closed => " No closed or merged pull requests.",
            })],
            Content::Loaded(pulls) => pulls
                .iter()
                .enumerate()
                .map(|(i, pull)| pull_line(pull, i == self.selected, width))
                .collect(),
            Content::Failed(message) => vec![Line::from(format!(" {message}")).red()],
        };
        // Keep the selected row visible; the rows never wrap, so one PR is one row.
        let first = (self.selected + 1).saturating_sub(height);
        let scroll = u16::try_from(first).unwrap_or(u16::MAX);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered())
                .scroll((scroll, 0)),
            area,
        );
    }
}

/// One row: selection marker, `#N`, state word, title, `@author`, date. The title is cut (with `…`) so the whole
/// row fits in `width` columns.
fn pull_line(pull: &PullRequest, selected: bool, width: usize) -> Line<'_> {
    let state = match pull.state {
        PrState::Open { draft: true } => "draft",
        PrState::Open { draft: false } => "open",
        PrState::Closed => "closed",
        PrState::Merged => "merged",
    };
    let prefix = format!(
        "{}{} {state:<6} ",
        if selected { ">" } else { " " },
        pull.number
    );
    let suffix = format!("  @{}  {}", pull.author, pull.updated_at.date());
    let room = width.saturating_sub(prefix.chars().count() + suffix.chars().count());
    let title = cut(&pull.title, room);
    Line::from(vec![
        Span::styled(prefix, Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(title),
        Span::raw(suffix).dim(),
    ])
}

/// `text` cut to at most `room` characters, ending with `…` when something was cut. Control characters become
/// spaces so a title cannot break the row.
fn cut(text: &str, room: usize) -> String {
    let clean: Vec<char> = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if clean.len() <= room {
        return clean.into_iter().collect();
    }
    let keep = room.saturating_sub(1);
    let mut out: String = clean.into_iter().take(keep).collect();
    if room > 0 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_binds_no_reserved_key() {
        assert!(!keys::uses_reserved_key(&TABLE));
    }

    #[test]
    fn cut_keeps_short_text_and_marks_a_cut() {
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("abcd", 3), "ab…");
        assert_eq!(cut("abcd", 0), "");
        assert_eq!(cut("a\nb", 5), "a b");
    }
}
