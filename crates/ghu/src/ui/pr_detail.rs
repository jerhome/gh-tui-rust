//! The detail of one PR (#6). ADR 0003 § 4 « Détail (#6) » (texts, keys, errors, reload after `Signal::Published`).
//!
//! Opened by `Enter` on the PR list. Loads a [`PrPage`] (four requests joined with `tokio::try_join!`, the first
//! error wins) as one [`Outcome::PrPage`]. Opens [`super::diff::DiffScreen`] (`d`) and
//! [`super::compose::ComposeScreen`] (`c`, `a`, `x`, only as [`actions`] allows).

use std::cell::Cell;

use gh_client::{
    Comment, Mergeable, PrNumber, PrState, PullDetail, RepoName, Review, ReviewState, Timestamp,
    User, Verdict,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::compose::{ComposeScreen, Kind, Target};
use super::diff::DiffScreen;
use super::keys::{self, Binding, HelpRow};
use super::text::{clean, markdown};
use super::{Flow, Outcome, RequestId, Shared, Signal, View, error_message};
use crate::{KeyCode, KeyEvent, KeyModifiers};

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

/// Actions of this screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Up,
    Down,
    PageUp,
    PageDown,
    Refresh,
    Diff,
    Comment,
    Approve,
    RequestChanges,
}

/// Key table of this screen (help order). The publishing keys are offered only as [`actions`] allows.
const TABLE: [Binding<Action>; 9] = [
    Binding {
        label: "Up/k",
        what: "Scroll up",
        action: Action::Up,
        keys: &[
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Char('k'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Down/j",
        what: "Scroll down",
        action: Action::Down,
        keys: &[
            (KeyCode::Down, KeyModifiers::NONE),
            (KeyCode::Char('j'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "PgUp",
        what: "Page up",
        action: Action::PageUp,
        keys: &[(KeyCode::PageUp, KeyModifiers::NONE)],
    },
    Binding {
        label: "PgDn",
        what: "Page down",
        action: Action::PageDown,
        keys: &[(KeyCode::PageDown, KeyModifiers::NONE)],
    },
    Binding {
        label: "r",
        what: "Refresh",
        action: Action::Refresh,
        keys: &[(KeyCode::Char('r'), KeyModifiers::NONE)],
    },
    Binding {
        label: "d",
        what: "Diff",
        action: Action::Diff,
        keys: &[(KeyCode::Char('d'), KeyModifiers::NONE)],
    },
    Binding {
        label: "c",
        what: "Comment",
        action: Action::Comment,
        keys: &[(KeyCode::Char('c'), KeyModifiers::NONE)],
    },
    Binding {
        label: "a",
        what: "Approve",
        action: Action::Approve,
        keys: &[(KeyCode::Char('a'), KeyModifiers::NONE)],
    },
    Binding {
        label: "x",
        what: "Request changes",
        action: Action::RequestChanges,
        keys: &[(KeyCode::Char('x'), KeyModifiers::NONE)],
    },
];

/// What the screen has to show.
enum Load {
    /// The first answer has not come yet.
    Loading,
    /// The page; kept while a reload is in flight.
    Loaded(Box<PrPage>),
    /// The request failed: a message for the user.
    Failed(String),
}

/// The PR detail screen.
pub(crate) struct PrDetailScreen {
    repo: RepoName,
    number: PrNumber,
    load: Load,
    /// The latest request; only its answer is applied.
    pending: Option<RequestId>,
    /// First row shown. May exceed `max_scroll` (after a reload that shortened the content, or after
    /// `Published`, which asks for the end): every use brings it back within.
    scroll: usize,
    /// Largest useful `scroll` and rows per screen, measured by the last render (key handling has no frame).
    max_scroll: Cell<usize>,
    page: Cell<usize>,
    /// Scroll to the end when the next answer arrives (after a published Comment or Review).
    to_end: bool,
}

impl PrDetailScreen {
    /// Opened by `Enter` on the PR list (which passes `repo.name.clone()`); starts loading the [`PrPage`] at once.
    pub(crate) fn new(repo: RepoName, number: PrNumber, shared: &mut Shared) -> Self {
        let mut screen = Self {
            repo,
            number,
            load: Load::Loading,
            pending: None,
            scroll: 0,
            max_scroll: Cell::new(0),
            page: Cell::new(0),
            to_end: false,
        };
        screen.load(shared);
        screen
    }

    /// Requests the four parts of the page; the content already shown stays until the answer comes.
    fn load(&mut self, shared: &mut Shared) {
        let client = shared.client().clone();
        let repo = self.repo.clone();
        let number = self.number;
        self.pending = Some(shared.spawn(async move {
            let page = tokio::try_join!(
                client.pull(&repo, number),
                client.reviews(&repo, number),
                client.comments(&repo, number),
                client.current_user(),
            );
            Outcome::PrPage(page.map(|(pull, reviews, comments, viewer)| {
                Box::new(PrPage {
                    pull,
                    reviews,
                    comments,
                    viewer,
                })
            }))
        }));
    }

    fn scroll_down(&mut self, rows: usize) {
        self.scroll = self.scroll.saturating_add(rows).min(self.max_scroll.get());
    }

    fn scroll_up(&mut self, rows: usize) {
        self.scroll = self.scroll.min(self.max_scroll.get()).saturating_sub(rows);
    }

    /// A page of scrolling: the rows of the screen but one, so the last line stays in view.
    fn page_rows(&self) -> usize {
        self.page.get().saturating_sub(1).max(1)
    }

    /// The compose screen for `kind`, if the loaded PR allows it; nothing otherwise.
    fn open_compose(&self, kind: Kind) -> Flow {
        let Load::Loaded(page) = &self.load else {
            return Flow::Handled;
        };
        let allowed = match (actions(&page.pull, &page.viewer), kind) {
            (Actions::All, _) | (Actions::CommentOnly, Kind::Comment) => true,
            (Actions::CommentOnly | Actions::None(_), _) => false,
        };
        if !allowed {
            return Flow::Handled;
        }
        let target = Target {
            repo: self.repo.clone(),
            number: self.number,
            head: page.pull.head_sha.clone(),
        };
        Flow::Push(Box::new(ComposeScreen::new(target, kind)))
    }
}

impl View for PrDetailScreen {
    /// `<owner/name> · #<n>`.
    fn title(&self) -> String {
        format!("{} · {}", self.repo, self.number)
    }

    fn help(&self) -> Vec<HelpRow> {
        keys::help_rows(&TABLE)
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match keys::lookup(&TABLE, key) {
            Some(Action::Up) => self.scroll_up(1),
            Some(Action::Down) => self.scroll_down(1),
            Some(Action::PageUp) => self.scroll_up(self.page_rows()),
            Some(Action::PageDown) => self.scroll_down(self.page_rows()),
            Some(Action::Refresh) => self.load(shared),
            Some(Action::Diff) => {
                return Flow::Push(Box::new(DiffScreen::new(
                    self.repo.clone(),
                    self.number,
                    shared,
                )));
            }
            Some(Action::Comment) => return self.open_compose(Kind::Comment),
            Some(Action::Approve) => return self.open_compose(Kind::Review(Verdict::Approve)),
            Some(Action::RequestChanges) => {
                return self.open_compose(Kind::Review(Verdict::RequestChanges));
            }
            None => return Flow::Ignored,
        }
        Flow::Handled
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, _shared: &mut Shared) -> Option<Outcome> {
        if self.pending != Some(id) {
            return Some(outcome);
        }
        self.pending = None;
        match outcome {
            Outcome::PrPage(Ok(page)) => {
                tracing::debug!(
                    comments = page.comments.len(),
                    reviews = page.reviews.len(),
                    "pull request loaded"
                );
                self.load = Load::Loaded(page);
                if std::mem::take(&mut self.to_end) {
                    self.scroll = usize::MAX;
                }
            }
            Outcome::PrPage(Err(error)) => {
                tracing::warn!(%error, "loading the pull request failed");
                self.to_end = false;
                self.load = Load::Failed(error_message("load the pull request", &error));
            }
            Outcome::TaskFailed => {
                self.to_end = false;
                self.load = Load::Failed(super::TASK_FAILED.to_owned());
            }
            other => return Some(other),
        }
        None
    }

    /// [`Signal::Published`]: reloads, then scrolls to the end once the answer arrives.
    fn signal(&mut self, signal: Signal, shared: &mut Shared) {
        match signal {
            Signal::Published => {
                self.load(shared);
                self.to_end = true;
            }
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width.saturating_sub(2));
        let height = usize::from(area.height.saturating_sub(2));
        let lines = match &self.load {
            Load::Loading => Vec::new(),
            Load::Loaded(page) => content_lines(page),
            Load::Failed(message) => vec![Line::from(message.clone()).red()],
        };
        let rows: Vec<Line<'static>> = lines.iter().flat_map(|line| wrap(line, width)).collect();
        let max = rows.len().saturating_sub(height);
        self.max_scroll.set(max);
        self.page.set(height);
        let visible: Vec<Line<'static>> = rows
            .into_iter()
            .skip(self.scroll.min(max))
            .take(height)
            .collect();
        frame.render_widget(Paragraph::new(visible).block(Block::bordered()), area);
    }
}

/// The lines of the detail (ADR 0003 § 4), before wrapping. Every text from the network goes through `clean`
/// (here or in `markdown`).
fn content_lines(page: &PrPage) -> Vec<Line<'static>> {
    let pull = &page.pull;
    let mut lines = vec![
        Line::from(format!("{} {}", pull.number, clean(&pull.title))).bold(),
        Line::from(summary(pull)),
        Line::from(format!(
            "Created {} · updated {} · {} files · +{} -{}",
            pull.created_at.date(),
            pull.updated_at.date(),
            pull.changed_files,
            pull.additions,
            pull.deletions
        ))
        .dim(),
        Line::from(verdicts_text(&page.reviews)),
        Line::from(actions_text(actions(pull, &page.viewer))),
        Line::default(),
        Line::from("Description").bold(),
    ];
    match pull.body.as_deref().filter(|body| !body.is_empty()) {
        Some(body) => lines.extend(markdown(body, 0)),
        None => lines.push(Line::from("No description.")),
    }
    lines.push(Line::default());
    lines.extend(conversation(page));
    lines
}

/// `<state> · @<author> · <head> → <base>`, then the mergeability of an open PR.
fn summary(pull: &PullDetail) -> String {
    let state = match pull.state {
        PrState::Open { draft: true } => "draft",
        PrState::Open { draft: false } => "open",
        PrState::Closed => "closed",
        PrState::Merged => "merged",
    };
    let mut text = format!(
        "{state} · @{} · {} → {}",
        clean(&pull.author),
        clean(&pull.head_label),
        clean(&pull.base_ref)
    );
    if pull.state.is_open() {
        text.push_str(match pull.mergeable {
            Mergeable::Yes => " · mergeable",
            Mergeable::Conflicts => " · conflicts",
            Mergeable::Unknown => " · mergeability unknown",
        });
    }
    text
}

/// `Verdicts: @carol Approve · @dave Request changes`, or `Verdicts: none`.
fn verdicts_text(reviews: &[Review]) -> String {
    let given: Vec<String> = verdicts(reviews)
        .into_iter()
        .map(|(author, verdict)| {
            let what = match verdict {
                Verdict::Approve => "Approve",
                Verdict::RequestChanges => "Request changes",
            };
            format!("@{} {what}", clean(&author))
        })
        .collect();
    if given.is_empty() {
        "Verdicts: none".to_owned()
    } else {
        format!("Verdicts: {}", given.join(" · "))
    }
}

/// The line of actions (and, when something is refused, why).
fn actions_text(actions: Actions) -> &'static str {
    match actions {
        Actions::All => "c Comment · a Approve · x Request changes · d Diff",
        Actions::CommentOnly => "c Comment · d Diff · Verdicts are not offered on your own PR.",
        Actions::None(Refusal::Archived) => "d Diff · This repo is archived: read-only.",
        Actions::None(Refusal::Closed) => "d Diff · This PR is closed: no comment or review.",
        Actions::None(Refusal::Merged) => "d Diff · This PR is merged: no comment or review.",
    }
}

/// One item of the conversation: a Comment, or a Review that has a text.
struct Entry<'a> {
    at: Option<&'a Timestamp>,
    author: &'a str,
    /// `None` for a Comment.
    label: Option<&'static str>,
    text: &'a str,
}

/// `Conversation (<k>)` and its entries in chronological order (a Review without date last).
fn conversation(page: &PrPage) -> Vec<Line<'static>> {
    let comments = page.comments.iter().map(|comment| Entry {
        at: Some(&comment.created_at),
        author: &comment.author,
        label: None,
        text: &comment.body,
    });
    let reviews = page.reviews.iter().filter_map(|review| {
        Some(Entry {
            at: review.submitted_at.as_ref(),
            author: &review.author,
            label: Some(match review.state {
                ReviewState::Approved => "Approve",
                ReviewState::ChangesRequested => "Request changes",
                ReviewState::Commented => "review",
                ReviewState::Dismissed => "dismissed",
            }),
            text: review.body.as_deref()?,
        })
    });
    let mut entries: Vec<Entry<'_>> = comments.chain(reviews).collect();
    // Stable: a Comment and a Review of the same second keep their order of listing.
    entries.sort_by(|a, b| {
        a.at.is_none()
            .cmp(&b.at.is_none())
            .then_with(|| a.at.cmp(&b.at))
    });
    let mut lines = vec![Line::from(format!("Conversation ({})", entries.len())).bold()];
    if entries.is_empty() {
        lines.push(Line::from("No comments."));
    }
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        let mut header = format!("@{}", clean(entry.author));
        if let Some(at) = entry.at {
            header.push_str(&format!(" · {}", at.date()));
        }
        if let Some(label) = entry.label {
            header.push_str(&format!(" · {label}"));
        }
        lines.push(Line::from(header).bold());
        lines.extend(markdown(entry.text, 2));
    }
    lines
}

/// `line` cut into rows of at most `width` characters: at the last space that fits, else inside the word. Styles
/// are kept. Always at least one row. (Characters are counted, not their display width: a wide character may
/// overflow its row, which the terminal clips.)
fn wrap(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let cells: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|span| {
            let style = line.style.patch(span.style);
            span.content.chars().map(move |c| (c, style))
        })
        .collect();
    let mut rows: Vec<&[(char, Style)]> = Vec::new();
    let mut rest = cells.as_slice();
    loop {
        if rest.len() <= width {
            rows.push(rest);
            break;
        }
        // A space at index 0..=width ends a row (the space is dropped), unless the row would be only spaces.
        let at = rest[..=width]
            .iter()
            .rposition(|&(c, _)| c == ' ')
            .filter(|&at| rest[..at].iter().any(|&(c, _)| c != ' '));
        match at {
            Some(at) => {
                rows.push(&rest[..at]);
                rest = &rest[at + 1..];
            }
            None => {
                rows.push(&rest[..width]);
                rest = &rest[width..];
            }
        }
        if rest.is_empty() {
            break;
        }
    }
    rows.into_iter().map(row_line).collect()
}

/// A row of styled characters as a `Line`, consecutive characters of the same style in one span.
fn row_line(row: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut style = Style::new();
    for &(c, cell_style) in row {
        if !current.is_empty() && cell_style != style {
            spans.push(Span::styled(std::mem::take(&mut current), style));
        }
        style = cell_style;
        current.push(c);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, style));
    }
    Line::from(spans)
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
    if pull.archived {
        return Actions::None(Refusal::Archived);
    }
    match pull.state {
        PrState::Closed => Actions::None(Refusal::Closed),
        PrState::Merged => Actions::None(Refusal::Merged),
        PrState::Open { .. } if pull.author.eq_ignore_ascii_case(&viewer.login) => {
            Actions::CommentOnly
        }
        PrState::Open { .. } => Actions::All,
    }
}

/// Current verdict per reviewer: the latest Approved / ChangesRequested / Dismissed review of each author, in the
/// order of those latest reviews; an author whose latest is Dismissed is left out; Commented is not a verdict.
/// Pure, never panics.
pub(crate) fn verdicts(reviews: &[Review]) -> Vec<(String, Verdict)> {
    // One entry per author, moved to the end by each of their later verdict reviews; `None`: dismissed.
    let mut latest: Vec<(&str, Option<Verdict>)> = Vec::new();
    for review in reviews {
        let verdict = match review.state {
            ReviewState::Approved => Some(Verdict::Approve),
            ReviewState::ChangesRequested => Some(Verdict::RequestChanges),
            ReviewState::Dismissed => None,
            ReviewState::Commented => continue,
        };
        latest.retain(|(author, _)| !author.eq_ignore_ascii_case(&review.author));
        latest.push((&review.author, verdict));
    }
    latest
        .into_iter()
        .filter_map(|(author, verdict)| Some((author.to_owned(), verdict?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gh_client::{ApiBase, Client, Token};
    use gh_fake::{FakeGitHub, PullReview, PullState, ReviewState, State};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use tokio::sync::mpsc;

    /// The detail, the reviews and the signed-in user (`alice`) of PR #1 of `alice/notes` as the client reads
    /// them from the fake: the model types are built by `gh-client` only.
    async fn fetch(pull: gh_fake::Pull, archived: bool) -> (PullDetail, Vec<Review>, User) {
        let mut state = State::new(gh_fake::User {
            login: "alice".into(),
            id: 1,
        });
        let mut repo = gh_fake::Repo::new("alice", "notes");
        repo.archived = archived;
        state.repos = vec![repo];
        state.pulls = vec![pull];
        let token = state.token.clone();
        let fake = FakeGitHub::start(state).await.expect("fake server");
        let client = Client::new(
            ApiBase::parse(&fake.base_url()).expect("base url"),
            Token::new(token).expect("token"),
        )
        .expect("client");
        let name = RepoName::new("alice", "notes").expect("repo name");
        let number = PrNumber::new(1).expect("number");
        let detail = client.pull(&name, number).await.expect("pull");
        let reviews = client.reviews(&name, number).await.expect("reviews");
        let viewer = client.current_user().await.expect("user");
        (detail, reviews, viewer)
    }

    fn pull(author: &str, state: PullState) -> gh_fake::Pull {
        let mut pull = gh_fake::Pull::new("alice", "notes", 1, "Title", author);
        pull.state = state;
        pull
    }

    fn review(id: u64, author: &str, state: ReviewState) -> PullReview {
        PullReview {
            id,
            author: Some(author.to_owned()),
            state,
            body: String::new(),
            submitted_at: (state != ReviewState::Pending)
                .then(|| format!("2026-03-01T10:00:{id:02}Z")),
            commit_id: format!("{:040x}", 1),
        }
    }

    async fn verdicts_of(reviews: Vec<PullReview>) -> Vec<(String, Verdict)> {
        let mut pull = pull("bob", PullState::Open);
        pull.reviews = reviews;
        let (_, reviews, _) = fetch(pull, false).await;
        verdicts(&reviews)
    }

    fn approve(author: &str) -> (String, Verdict) {
        (author.to_owned(), Verdict::Approve)
    }

    fn changes(author: &str) -> (String, Verdict) {
        (author.to_owned(), Verdict::RequestChanges)
    }

    #[tokio::test]
    async fn an_open_pr_of_someone_else_offers_comment_and_both_verdicts() {
        let (detail, _, viewer) = fetch(pull("bob", PullState::Open), false).await;
        assert_eq!(actions(&detail, &viewer), Actions::All);
        let mut draft = pull("bob", PullState::Open);
        draft.draft = true;
        let (detail, _, viewer) = fetch(draft, false).await;
        assert_eq!(actions(&detail, &viewer), Actions::All);
    }

    #[tokio::test]
    async fn an_own_pr_offers_the_comment_only_whatever_the_case_of_the_login() {
        for author in ["alice", "ALICE", "Alice"] {
            let (detail, _, viewer) = fetch(pull(author, PullState::Open), false).await;
            assert_eq!(actions(&detail, &viewer), Actions::CommentOnly, "{author}");
        }
    }

    #[tokio::test]
    async fn a_closed_or_merged_pr_offers_nothing_even_when_it_is_own() {
        for (author, state, refusal) in [
            ("bob", PullState::Closed, Refusal::Closed),
            ("alice", PullState::Closed, Refusal::Closed),
            ("bob", PullState::Merged, Refusal::Merged),
            ("alice", PullState::Merged, Refusal::Merged),
        ] {
            let (detail, _, viewer) = fetch(pull(author, state), false).await;
            assert_eq!(
                actions(&detail, &viewer),
                Actions::None(refusal),
                "{author} {state:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_archived_repo_beats_closed_merged_and_own() {
        for (author, state) in [
            ("bob", PullState::Open),
            ("alice", PullState::Open),
            ("bob", PullState::Closed),
            ("alice", PullState::Merged),
        ] {
            let (detail, _, viewer) = fetch(pull(author, state), true).await;
            assert!(detail.archived);
            assert_eq!(
                actions(&detail, &viewer),
                Actions::None(Refusal::Archived),
                "{author} {state:?}"
            );
        }
    }

    #[tokio::test]
    async fn verdicts_list_the_latest_verdict_of_each_reviewer_in_the_order_of_those_reviews() {
        let got = verdicts_of(vec![
            review(1, "carol", ReviewState::Approved),
            review(2, "dave", ReviewState::ChangesRequested),
            review(3, "carol", ReviewState::ChangesRequested),
            review(4, "dave", ReviewState::Approved),
            review(5, "erin", ReviewState::Approved),
        ])
        .await;
        // Latest reviews: carol (3), dave (4), erin (5).
        assert_eq!(got, [changes("carol"), approve("dave"), approve("erin")]);
    }

    #[tokio::test]
    async fn verdicts_ignore_comments_pending_reviews_and_dismissed_verdicts() {
        let got = verdicts_of(vec![
            review(1, "carol", ReviewState::Approved),
            review(2, "carol", ReviewState::Commented),
            review(3, "dave", ReviewState::ChangesRequested),
            review(4, "dave", ReviewState::Dismissed),
            review(5, "alice", ReviewState::Pending),
            review(6, "erin", ReviewState::Commented),
        ])
        .await;
        // A comment after an approval does not take it back; a dismissed verdict leaves the reviewer out.
        assert_eq!(got, [approve("carol")]);
        assert!(verdicts_of(Vec::new()).await.is_empty());
    }

    #[tokio::test]
    async fn a_verdict_given_again_after_a_dismissal_counts() {
        let got = verdicts_of(vec![
            review(1, "dave", ReviewState::ChangesRequested),
            review(2, "dave", ReviewState::Dismissed),
            review(3, "dave", ReviewState::Approved),
        ])
        .await;
        assert_eq!(got, [approve("dave")]);
    }

    #[tokio::test]
    async fn reviewers_are_told_apart_without_regard_to_the_case_of_their_login() {
        let got = verdicts_of(vec![
            review(1, "Carol", ReviewState::Approved),
            review(2, "carol", ReviewState::ChangesRequested),
        ])
        .await;
        assert_eq!(got, [changes("carol")]);
    }

    #[test]
    fn the_table_binds_no_reserved_key() {
        assert!(!keys::uses_reserved_key(&TABLE));
    }

    #[test]
    fn every_help_row_fits_the_help_box() {
        for row in keys::help_rows(&TABLE) {
            assert!(row.label.chars().count() <= 8, "{row:?}");
            assert!(row.what.chars().count() <= 26, "{row:?}");
        }
    }

    fn texts(rows: &[Line<'_>]) -> Vec<String> {
        rows.iter()
            .map(|row| row.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn wrap_cuts_at_the_last_space_that_fits_or_inside_a_long_word() {
        let wrap_text =
            |text: &str, width: usize| texts(&wrap(&Line::from(text.to_owned()), width));
        assert_eq!(wrap_text("", 5), [""]);
        assert_eq!(wrap_text("abc", 5), ["abc"]);
        assert_eq!(wrap_text("abc def ghi", 7), ["abc def", "ghi"]);
        assert_eq!(wrap_text("abc def ghi", 8), ["abc def", "ghi"]);
        assert_eq!(wrap_text("abc def", 3), ["abc", "def"]);
        assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        // The indent of a long word is not made a row of its own.
        assert_eq!(wrap_text("  abcdefgh", 4), ["  ab", "cdef", "gh"]);
        // A trailing space that would start a row does not leave an empty one.
        assert_eq!(wrap_text("abcd ", 4), ["abcd"]);
        // Width 0 is treated as 1.
        assert_eq!(wrap_text("abc", 0), ["a", "b", "c"]);
    }

    #[test]
    fn wrap_keeps_the_styles_of_the_line_and_of_its_spans() {
        let line = Line::from(vec![Span::raw("ab "), Span::raw("cd").bold()]).dim();
        let rows = wrap(&line, 3);
        assert_eq!(texts(&rows), ["ab", "cd"]);
        let style = |row: &Line<'_>| row.spans[0].style.add_modifier;
        assert!(style(&rows[0]).contains(Modifier::DIM));
        assert!(!style(&rows[0]).contains(Modifier::BOLD));
        assert!(style(&rows[1]).contains(Modifier::DIM | Modifier::BOLD));
    }

    #[test]
    fn wrap_never_loses_a_character_nor_overflows_a_row() {
        const POOL: [&str; 6] = ["a", "b", "é", "日", " ", "  "];
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % 1000).expect("small")
        };
        for _ in 0..2000 {
            let len = next() % 30;
            let text: String = (0..len).map(|_| POOL[next() % POOL.len()]).collect();
            let width = next() % 8;
            let rows = texts(&wrap(&Line::from(text.clone()), width));
            assert!(!rows.is_empty());
            for row in &rows {
                assert!(
                    row.chars().count() <= width.max(1),
                    "{text:?} at {width}: {rows:?}"
                );
            }
            let kept: String = rows.concat().chars().filter(|c| *c != ' ').collect();
            let wanted: String = text.chars().filter(|c| *c != ' ').collect();
            assert_eq!(kept, wanted, "{text:?} at {width}: {rows:?}");
        }
    }

    /// What `screen` draws on a 100 × 30 test terminal, one row per line.
    fn drawn(terminal: &mut Terminal<TestBackend>, screen: &PrDetailScreen) -> String {
        terminal
            .draw(|frame| screen.render(frame, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn after_a_published_signal_the_reload_scrolls_to_the_end_once_it_arrives() {
        let mut pull = gh_fake::Pull::new("alice", "notes", 1, "Long", "bob");
        pull.body = Some(
            (1..=100)
                .map(|i| format!("line {i:03}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut state = State::new(gh_fake::User {
            login: "alice".into(),
            id: 1,
        });
        state.repos = vec![gh_fake::Repo::new("alice", "notes")];
        state.pulls = vec![pull];
        let token = state.token.clone();
        let fake = FakeGitHub::start(state).await.expect("fake server");
        let client = Client::new(
            ApiBase::parse(&fake.base_url()).expect("base url"),
            Token::new(token).expect("token"),
        )
        .expect("client");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut shared = Shared {
            client,
            tx,
            in_flight: 0,
            next_id: 0,
            notice: None,
            pushed: Vec::new(),
            popped: None,
            editor: crate::editor::EditorVars::default(),
            edit: None,
        };
        let mut screen = PrDetailScreen::new(
            RepoName::new("alice", "notes").expect("repo name"),
            PrNumber::new(1).expect("number"),
            &mut shared,
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        let (id, outcome) = rx.recv().await.expect("first answer");
        assert!(screen.apply(id, outcome, &mut shared).is_none());
        let top = drawn(&mut terminal, &screen);
        assert!(
            top.contains("line 001") && !top.contains("No comments."),
            "{top}"
        );

        screen.signal(Signal::Published, &mut shared);
        // Nothing moves before the answer: the content stays where it was.
        assert_eq!(drawn(&mut terminal, &screen), top);
        let (id, outcome) = rx.recv().await.expect("reload answer");
        assert!(screen.apply(id, outcome, &mut shared).is_none());
        let end = drawn(&mut terminal, &screen);
        assert!(
            end.contains("No comments.") && !end.contains("line 001"),
            "{end}"
        );

        // The request to scroll is spent: a plain reload keeps the position.
        screen.scroll_up(10);
        let before = drawn(&mut terminal, &screen);
        assert_ne!(before, end, "scroll_up did not move the content");
        screen.load(&mut shared);
        let (id, outcome) = rx.recv().await.expect("second reload answer");
        assert!(screen.apply(id, outcome, &mut shared).is_none());
        assert_eq!(drawn(&mut terminal, &screen), before);
    }
}
