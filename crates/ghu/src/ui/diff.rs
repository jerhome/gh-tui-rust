//! The Diff of one PR (#7). Contract for dev[tui-diff]: ADR 0003 § 4 « Diff (#7) ».
//!
//! Wave 0 gives a provisional body that never panics (title, `Not available yet.`, `Esc` goes back): no
//! `todo!()` may be reachable from the keyboard in a commit. dev[tui-diff] replaces it (loading with
//! `Client::pull_files` → [`super::Outcome::Files`]).
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: opened from wave 2 (dev[tui-detail])"
)]

use gh_client::{PrNumber, RepoName};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph};

use super::{Flow, HelpRow, Outcome, RequestId, Shared, View};
use crate::KeyEvent;

/// The Diff screen. Private fields belong to dev[tui-diff] (files, selected file, scroll…).
pub(crate) struct DiffScreen {
    repo: RepoName,
    number: PrNumber,
}

impl DiffScreen {
    /// Opened by `d` on the detail; starts loading the files at once (provisional: loads nothing).
    pub(crate) fn new(repo: RepoName, number: PrNumber, shared: &mut Shared) -> Self {
        let _ = shared;
        Self { repo, number }
    }
}

impl View for DiffScreen {
    /// `<owner/name> · #<n> · Diff`.
    fn title(&self) -> String {
        format!("{} · {} · Diff", self.repo, self.number)
    }

    fn help(&self) -> Vec<HelpRow> {
        Vec::new()
    }

    /// Provisional: every key goes to the global table (`Esc` back).
    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        let _ = (key, shared);
        Flow::Ignored
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, shared: &mut Shared) -> Option<Outcome> {
        let _ = (id, shared);
        Some(outcome)
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Paragraph::new(" Not available yet.").block(Block::bordered()),
            area,
        );
    }
}
