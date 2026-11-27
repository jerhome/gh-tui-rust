//! Writing, confirming and publishing a Comment (#8) or a Review (#10). Contract: ADR 0003 § 4 « Saisie et
//! confirmation » and § 6.
//!
//! Wave 0 gives a provisional body that never panics (title, `Not available yet.`, `Esc` goes back).
//! dev[tui-compose] writes the Comment path (Writing → Confirm → Publishing, [`super::Outcome::CommentPosted`])
//! and leaves the Review branches on `Not available yet.`; dev[tui-review] writes them
//! ([`super::Outcome::ReviewPosted`]). Success: `Shared::pop(Some(Signal::Published))`.
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: opened from wave 2 (dev[tui-detail])"
)]

use gh_client::{CommitSha, PrNumber, RepoName, Verdict};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph};

use super::text_area::TextArea;
use super::{Flow, HelpRow, Outcome, RequestId, Shared, View};
use crate::{EditorError, KeyEvent};

/// The PR a text is published on.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    /// The repo of the PR.
    pub(crate) repo: RepoName,
    /// The PR.
    pub(crate) number: PrNumber,
    /// `head.sha` seen on the detail (Review `commit_id`).
    pub(crate) head: CommitSha,
}

/// What is published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A conversation Comment.
    Comment,
    /// A Review with this Verdict.
    Review(Verdict),
}

/// The compose screen. Private fields belong to dev[tui-compose] (phase Writing | Confirm | Publishing, error,
/// pending request…).
pub(crate) struct ComposeScreen {
    target: Target,
    kind: Kind,
    text: TextArea,
}

impl ComposeScreen {
    /// Opened by `c`, `a` or `x` on the detail, only when `pr_detail::actions` allows it. No request at opening.
    pub(crate) fn new(target: Target, kind: Kind) -> Self {
        Self {
            target,
            kind,
            text: TextArea::new(),
        }
    }
}

impl View for ComposeScreen {
    /// `<owner/name> · #<n> · Comment`, `… · Review: Approve` or `… · Review: Request changes`.
    fn title(&self) -> String {
        let what = match self.kind {
            Kind::Comment => "Comment",
            Kind::Review(Verdict::Approve) => "Review: Approve",
            Kind::Review(Verdict::RequestChanges) => "Review: Request changes",
        };
        format!("{} · {} · {what}", self.target.repo, self.target.number)
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

    /// Forwards the editor's answer to the text field.
    fn edited(&mut self, result: Result<String, EditorError>, shared: &mut Shared) {
        self.text.edited(result, shared);
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Paragraph::new(" Not available yet.").block(Block::bordered()),
            area,
        );
    }
}
