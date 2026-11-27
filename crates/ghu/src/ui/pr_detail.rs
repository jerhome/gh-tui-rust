//! The detail of one PR (#6). Contract for dev[tui-detail]: ADR 0003 § 4 « Détail (#6) » (texts, keys, errors,
//! reload after `Signal::Published`).
//!
//! Opened by `Enter` on the PR list. Loads a [`PrPage`] (four requests joined with `tokio::try_join!`, the first
//! error wins) as one [`Outcome::PrPage`]. Opens [`super::diff::DiffScreen`] (`d`) and
//! [`super::compose::ComposeScreen`] (`c`, `a`, `x`, only as [`actions`] allows).
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: opened from wave 2 (dev[tui-detail])"
)]

use gh_client::{Comment, PrNumber, PullDetail, RepoName, Review, User, Verdict};
use ratatui::Frame;
use ratatui::layout::Rect;

use super::{Flow, HelpRow, Outcome, RequestId, Shared, Signal, View};
use crate::KeyEvent;

/// Everything the detail shows, loaded together.
#[derive(Clone, Debug)]
pub(crate) struct PrPage {
    /// `Client::pull`.
    pub(crate) pull: PullDetail,
    /// `Client::reviews` (never pending).
    pub(crate) reviews: Vec<Review>,
    /// `Client::comments`.
    pub(crate) comments: Vec<Comment>,
    /// `Client::current_user`, to tell an Own PR.
    pub(crate) viewer: User,
}

/// The PR detail screen. Private fields belong to dev[tui-detail] (content, scroll, pending request…).
pub(crate) struct PrDetailScreen {
    repo: RepoName,
    number: PrNumber,
}

impl PrDetailScreen {
    /// Opened by `Enter` on the PR list (which passes `repo.name.clone()`); starts loading the [`PrPage`] at once.
    pub(crate) fn new(repo: RepoName, number: PrNumber, shared: &mut Shared) -> Self {
        let _ = (repo, number, shared);
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }
}

impl View for PrDetailScreen {
    /// `<owner/name> · #<n>`.
    fn title(&self) -> String {
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }

    fn help(&self) -> Vec<HelpRow> {
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        let _ = (key, shared);
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, shared: &mut Shared) -> Option<Outcome> {
        let _ = (id, outcome, shared);
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }

    /// [`Signal::Published`]: reloads, then scrolls to the end once the answer arrives.
    fn signal(&mut self, signal: Signal, shared: &mut Shared) {
        let _ = (signal, shared);
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let _ = (frame, area);
        todo!("ADR 0003, wave 2: dev[tui-detail]")
    }
}

/// What the user may do on this PR (pure, unit-tested).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Actions {
    /// Open, not archived, not own: Comment and both Verdicts.
    All,
    /// Own PR: Comment only.
    CommentOnly,
    /// Nothing that publishes; the reason is shown.
    None(Refusal),
}

/// Why nothing can be published on a PR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// [`PullDetail::archived`].
    Archived,
    /// Closed without merge.
    Closed,
    /// Merged.
    Merged,
}

/// Archived (`PullDetail::archived`, fresh at each load) beats Closed / Merged, which beat Own PR.
/// Own PR: author == viewer login, ASCII case-insensitive. The Repo loaded at start-up is not used (it may be
/// stale). Pure, never panics.
pub(crate) fn actions(pull: &PullDetail, viewer: &User) -> Actions {
    let _ = (pull, viewer);
    todo!("ADR 0003, wave 2: dev[tui-detail]")
}

/// Current verdict per reviewer: the latest Approved / ChangesRequested / Dismissed review of each author, in the
/// order of those latest reviews; an author whose latest is Dismissed is left out; Commented is not a verdict.
/// Pure, never panics.
pub(crate) fn verdicts(reviews: &[Review]) -> Vec<(String, Verdict)> {
    let _ = reviews;
    todo!("ADR 0003, wave 2: dev[tui-detail]")
}
