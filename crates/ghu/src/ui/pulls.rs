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
//! What is here now (architecte): a stub with the final title and nothing else, so dev[tui-repos] can test that
//! `Enter` and `owner/name` open this screen. Private fields and helpers are free for dev[tui-pulls].

use gh_client::Repo;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::keys::HelpRow;
use super::{Flow, Outcome, RequestId, Shared, View};
use crate::KeyEvent;

/// The PR list screen of one repo.
pub(crate) struct PullsScreen {
    repo: Repo,
}

impl PullsScreen {
    /// Builds the screen for `repo` and starts loading its open PRs (stub: loads nothing yet).
    pub(crate) fn new(repo: Repo, shared: &mut Shared) -> Self {
        let _ = shared;
        Self { repo }
    }
}

impl View for PullsScreen {
    /// `<owner/name> · Pull requests (open)` (final wording; `(closed / merged)` after `Tab`).
    fn title(&self) -> String {
        format!("{} · Pull requests (open)", self.repo.name)
    }

    fn help(&self) -> Vec<HelpRow> {
        Vec::new()
    }

    fn key(&mut self, _key: KeyEvent, _shared: &mut Shared) -> Flow {
        Flow::Ignored
    }

    fn apply(&mut self, _id: RequestId, outcome: Outcome, _shared: &mut Shared) -> Option<Outcome> {
        Some(outcome)
    }

    fn render(&self, _frame: &mut Frame<'_>, _area: Rect) {}
}
