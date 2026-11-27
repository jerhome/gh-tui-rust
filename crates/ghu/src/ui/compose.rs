//! Writing, confirming and publishing a Comment (#8) or a Review (#10). Contract: ADR 0003 § 4 « Saisie et
//! confirmation » and § 6.
//!
//! Writing → Confirm → Publishing, answered by [`super::Outcome::CommentPosted`] or
//! [`super::Outcome::ReviewPosted`]. Success: `Shared::pop(Some(Signal::Published))`. Nothing is sent before the Confirm phase has been answered with `y` or
//! `Enter`.

use gh_client::{
    BodyProblem, CommitSha, Error, MAX_BODY_CHARS, MessageBody, PrNumber, RepoName, ReviewRequest,
    Verdict,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::keys::{self, Binding};
use super::text::clean;
use super::text_area::{TextArea, TextKey, wrap};
use super::{
    Draft, Flow, HelpRow, Notice, Outcome, RequestId, Shared, Signal, View, error_message,
};
use crate::{EditorError, KeyCode, KeyEvent, KeyModifiers};

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

/// Where the user is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Typing the text.
    Writing,
    /// Reading what will be published, waiting for `y` / `Enter` or `n` / `Esc`.
    Confirm,
    /// The one request in flight.
    Publishing(RequestId),
}

/// Keys of the Writing phase that are not text.
#[derive(Clone, Copy)]
enum WritingAction {
    Publish,
    Cancel,
    /// Help row only: `Ctrl-E` is taken by the text field first (it asks for the editor).
    Edit,
}

const WRITING: [Binding<WritingAction>; 3] = [
    Binding {
        label: "Ctrl-S",
        what: "Review, then publish",
        action: WritingAction::Publish,
        keys: &[(KeyCode::Char('s'), KeyModifiers::CONTROL)],
    },
    Binding {
        label: "Ctrl-E",
        what: "Open $VISUAL / $EDITOR",
        action: WritingAction::Edit,
        keys: &[(KeyCode::Char('e'), KeyModifiers::CONTROL)],
    },
    Binding {
        label: "Esc",
        what: "Cancel (twice if text)",
        action: WritingAction::Cancel,
        keys: &[(KeyCode::Esc, KeyModifiers::NONE)],
    },
];

/// Keys of the Confirm phase.
#[derive(Clone, Copy)]
enum ConfirmAction {
    Publish,
    Back,
}

const CONFIRM: [Binding<ConfirmAction>; 2] = [
    Binding {
        label: "y/Enter",
        what: "Publish",
        action: ConfirmAction::Publish,
        keys: &[
            (KeyCode::Char('y'), KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "n/Esc",
        what: "Back to the text",
        action: ConfirmAction::Back,
        keys: &[
            (KeyCode::Char('n'), KeyModifiers::NONE),
            (KeyCode::Esc, KeyModifiers::NONE),
        ],
    },
];

const WRITING_HINT: &str = " Ctrl-S publish… · Ctrl-E editor · Esc cancel";
const CONFIRM_HINT: &str = " y / Enter publish · n / Esc back to the text";

/// Whether GitHub may have created the text although the answer was not a clear success: no answer, an answer
/// that cannot be used (usually a 2xx), or a 5xx (a gateway error, or a failure after the write).
fn uncertain(error: &Error) -> bool {
    matches!(
        error,
        Error::Network { .. }
            | Error::UnexpectedResponse { .. }
            | Error::Api {
                status: 500..=599,
                ..
            }
    )
}

/// Shown when the publication has no clear answer: the `what` (`comment` or `review`) may exist.
fn maybe_published(what: &str) -> String {
    format!(
        "Cannot publish the {what}: no clear answer from GitHub. It may have been published anyway: press Esc, \
         then r to check the PR; the text comes back when you open the {what} again."
    )
}

/// Shown when GitHub refuses a Review with a 422: a pending review of the user, or a PR that changed since the load.
const REVIEW_REFUSED: &str = "GitHub refused the review (422). If you started a review on the website (pending review), \
     submit or delete it there; or the PR changed: press Esc, then r to refresh; the text comes back when you open \
     the review again.";

/// The key of the global help (`?`, with or without Shift).
fn is_help(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('?')
        && (key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT)
}

/// The compose screen.
pub(crate) struct ComposeScreen {
    target: Target,
    kind: Kind,
    text: TextArea,
    phase: Phase,
    /// The first `Esc` on a text was seen and no other key since.
    esc_armed: bool,
    /// Why the last publication failed, drawn in red above the text until the next publication.
    error: Option<String>,
}

impl ComposeScreen {
    /// `comment` or `review`: the word of the messages.
    fn what(&self) -> &'static str {
        match self.kind {
            Kind::Comment => "comment",
            Kind::Review(_) => "review",
        }
    }

    /// Opened by `c`, `a` or `x` on the detail, only when `pr_detail::actions` allows it. No request at opening.
    /// `draft`: the text kept for this `kind` (`Signal::Kept`), put in the field with the cursor at the end and
    /// no message; `None`: an empty field.
    pub(crate) fn new(target: Target, kind: Kind, draft: Option<String>) -> Self {
        let mut text = TextArea::new();
        if let Some(draft) = draft {
            text.paste(&draft);
        }
        Self {
            target,
            kind,
            text,
            phase: Phase::Writing,
            esc_armed: false,
            error: None,
        }
    }

    fn key_writing(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        let armed = std::mem::take(&mut self.esc_armed);
        if self.text.key(key, shared) == TextKey::Handled {
            return Flow::Handled;
        }
        match keys::lookup(&WRITING, key) {
            Some(WritingAction::Publish) => self.check_text(shared),
            Some(WritingAction::Cancel) if self.error.is_some() => self.close_keeping_text(shared),
            Some(WritingAction::Cancel) => {
                if armed || self.text.text().trim().is_empty() {
                    shared.pop(None);
                } else {
                    self.esc_armed = true;
                    shared.notify(Notice::Info(
                        "Press Esc again to discard the text.".to_owned(),
                    ));
                }
            }
            // Ctrl-E never gets here (the text field handled it); the row is for the help.
            Some(WritingAction::Edit) | None => {}
        }
        Flow::Handled
    }

    /// `Esc` after a failed publication: the screen closes at once and the detail below keeps the text.
    fn close_keeping_text(&mut self, shared: &mut Shared) {
        let text = self.text.text();
        if text.trim().is_empty() {
            shared.pop(None);
            return;
        }
        let what = self.what();
        shared.notify(Notice::Info(format!(
            "Text kept: open the {what} again to get it back."
        )));
        shared.pop(Some(Signal::Kept(Draft {
            kind: self.kind,
            text,
        })));
    }

    /// Ctrl-S: the text must be publishable, then the user is asked to confirm.
    fn check_text(&mut self, shared: &mut Shared) {
        match MessageBody::new(&self.text.text()) {
            Ok(_) => self.phase = Phase::Confirm,
            // An Approve may go without text.
            Err(Error::InvalidBody {
                problem: BodyProblem::Blank,
            }) if self.kind == Kind::Review(Verdict::Approve) => self.phase = Phase::Confirm,
            Err(Error::InvalidBody {
                problem: BodyProblem::Blank,
            }) if self.kind == Kind::Review(Verdict::RequestChanges) => {
                shared.notify(Notice::Error("Request changes needs a text.".to_owned()))
            }
            Err(Error::InvalidBody {
                problem: BodyProblem::Blank,
            }) => shared.notify(Notice::Error("Write a text first.".to_owned())),
            Err(Error::InvalidBody {
                problem: BodyProblem::TooLong { chars },
            }) => shared.notify(Notice::Error(format!(
                "The text is too long: {chars} characters (GitHub allows {MAX_BODY_CHARS})."
            ))),
            Err(other) => shared.notify(Notice::Error(error_message("check the text", &other))),
        }
    }

    fn key_confirm(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match keys::lookup(&CONFIRM, key) {
            Some(ConfirmAction::Publish) => self.publish(shared),
            Some(ConfirmAction::Back) => self.phase = Phase::Writing,
            None if is_help(key) => return Flow::Ignored,
            None => {}
        }
        Flow::Handled
    }

    /// The only place that sends: one request, never repeated by itself.
    fn publish(&mut self, shared: &mut Shared) {
        self.error = None;
        let text = self.text.text();
        let body = match MessageBody::new(&text) {
            Ok(body) => Some(body),
            // Only an Approve may go without text: it is sent without `body`.
            Err(Error::InvalidBody {
                problem: BodyProblem::Blank,
            }) if self.kind == Kind::Review(Verdict::Approve) => None,
            Err(error) => {
                // Cannot happen (the text was checked and the Confirm phase does not edit it); stay safe.
                self.phase = Phase::Writing;
                self.error = Some(error_message(
                    &format!("publish the {}", self.what()),
                    &error,
                ));
                return;
            }
        };
        tracing::debug!(
            chars = text.chars().count(),
            what = self.what(),
            "publishing"
        );
        let client = shared.client().clone();
        let repo = self.target.repo.clone();
        let number = self.target.number;
        let id = match (self.kind, body) {
            (Kind::Comment, Some(body)) => shared.spawn(async move {
                Outcome::CommentPosted(client.post_comment(&repo, number, &body).await)
            }),
            (Kind::Review(verdict), body) => {
                let request = match (verdict, body) {
                    (Verdict::Approve, body) => ReviewRequest::Approve { body },
                    (Verdict::RequestChanges, Some(body)) => ReviewRequest::RequestChanges { body },
                    (Verdict::RequestChanges, None) => {
                        // Cannot happen (Ctrl-S refuses a blank text); stay safe.
                        self.phase = Phase::Writing;
                        self.error = Some("Request changes needs a text.".to_owned());
                        return;
                    }
                };
                let head = self.target.head.clone();
                shared.spawn(async move {
                    Outcome::ReviewPosted(client.post_review(&repo, number, &head, &request).await)
                })
            }
            (Kind::Comment, None) => {
                // Cannot happen (a blank Comment is refused by Ctrl-S); stay safe.
                self.phase = Phase::Writing;
                self.error = Some("Write a text first.".to_owned());
                return;
            }
        };
        self.phase = Phase::Publishing(id);
    }

    fn key_publishing(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match (key.code, key.modifiers) {
            (KeyCode::Esc | KeyCode::Char('q'), KeyModifiers::NONE) => shared.notify(Notice::Info(
                "Publishing: wait for GitHub's answer.".to_owned(),
            )),
            _ if is_help(key) => return Flow::Ignored,
            _ => {}
        }
        Flow::Handled
    }

    /// Back to Writing with the text intact and a message above it.
    fn failed(&mut self, message: String) {
        self.phase = Phase::Writing;
        self.error = Some(message);
    }

    fn render_writing(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width.max(1));
        let error_rows = self.error.as_ref().map_or(0, |message| {
            (message.chars().count() / width + 2).min(usize::from(area.height) / 2)
        });
        let [hint, error, zone] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::try_from(error_rows).unwrap_or(0)),
            Constraint::Min(0),
        ])
        .areas(area);
        frame.render_widget(Line::from(WRITING_HINT), hint);
        if let Some(message) = &self.error {
            frame.render_widget(
                Paragraph::new(format!(" {message}"))
                    .wrap(Wrap { trim: true })
                    .red(),
                error,
            );
        }
        let block = Block::bordered();
        let inner = block.inner(zone);
        frame.render_widget(block, zone);
        self.text.render(frame, inner);
    }

    fn render_confirm(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [top, bottom] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(inner);
        let width = usize::from(top.width.saturating_sub(2)).max(1);
        let header = format!(
            "Publish this {} on {} {}?",
            match self.kind {
                Kind::Comment => "Comment",
                Kind::Review(_) => "Review",
            },
            self.target.repo,
            self.target.number
        );
        // The header may be long (owner/name): it is wrapped like the text.
        let mut lines: Vec<Line<'_>> = wrap(&clean(&header), width)
            .into_iter()
            .map(|row| Line::from(format!(" {row}")).bold())
            .collect();
        if let Kind::Review(verdict) = self.kind {
            lines.push(Line::from(match verdict {
                Verdict::Approve => " Verdict: Approve",
                Verdict::RequestChanges => " Verdict: Request changes",
            }));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(" Text:"));
        let text = self.text.text();
        let rows: Vec<String> = if text.trim().is_empty() {
            vec!["(no text)".to_owned()]
        } else {
            text.split('\n')
                .flat_map(|line| wrap(&clean(line), width))
                .collect()
        };
        let height = usize::from(top.height);
        let room = height.saturating_sub(lines.len());
        if rows.len() <= room {
            lines.extend(rows.iter().map(|row| Line::from(format!(" {row}"))));
        } else {
            // The last row of the area always says how many lines are left: the fixed lines give way if need be.
            let shown = room.saturating_sub(1);
            lines.truncate(height.saturating_sub(1 + shown));
            lines.extend(
                rows[..shown]
                    .iter()
                    .map(|row| Line::from(format!(" {row}"))),
            );
            lines.push(Line::from(format!(" … {} more lines", rows.len() - shown)).dim());
        }
        frame.render_widget(Paragraph::new(lines), top);
        frame.render_widget(
            Paragraph::new(vec![Line::from(""), Line::from(CONFIRM_HINT)]),
            bottom,
        );
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
        match self.phase {
            Phase::Publishing(_) => Vec::new(),
            Phase::Writing => keys::help_rows(&WRITING),
            Phase::Confirm => keys::help_rows(&CONFIRM),
        }
    }

    /// Only the Writing phase takes text (every key but `Ctrl-C`).
    fn editing(&self) -> bool {
        self.phase == Phase::Writing
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match self.phase {
            Phase::Writing => self.key_writing(key, shared),
            Phase::Confirm => self.key_confirm(key, shared),
            Phase::Publishing(_) => self.key_publishing(key, shared),
        }
    }

    fn paste(&mut self, text: &str, _shared: &mut Shared) {
        if self.editing() {
            self.esc_armed = false;
            self.text.paste(text);
        }
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, shared: &mut Shared) -> Option<Outcome> {
        if self.phase != Phase::Publishing(id) {
            return Some(outcome);
        }
        match outcome {
            Outcome::CommentPosted(Ok(_)) => {
                shared.notify(Notice::Info("Comment published.".to_owned()));
                shared.pop(Some(Signal::Published));
            }
            Outcome::ReviewPosted(Ok(_)) => {
                let verdict = match self.kind {
                    Kind::Review(Verdict::RequestChanges) => "Request changes",
                    _ => "Approve",
                };
                shared.notify(Notice::Info(format!("Review published: {verdict}.")));
                shared.pop(Some(Signal::Published));
            }
            Outcome::CommentPosted(Err(error)) | Outcome::ReviewPosted(Err(error))
                if uncertain(&error) =>
            {
                tracing::warn!(%error, "no clear answer to the publication");
                self.failed(maybe_published(self.what()));
            }
            Outcome::TaskFailed => {
                tracing::warn!("the publication task died");
                self.failed(maybe_published(self.what()));
            }
            Outcome::ReviewPosted(Err(Error::Unprocessable { .. })) => {
                tracing::warn!("GitHub refused the review (422)");
                self.failed(REVIEW_REFUSED.to_owned());
            }
            Outcome::ReviewPosted(Err(error)) => {
                tracing::warn!(%error, "publishing the review failed");
                self.failed(error_message("publish the review", &error));
            }
            Outcome::CommentPosted(Err(error)) => {
                tracing::warn!(%error, "publishing the comment failed");
                self.failed(error_message("publish the comment", &error));
            }
            other => return Some(other),
        }
        None
    }

    /// Forwards the editor's answer to the text field.
    fn edited(&mut self, result: Result<String, EditorError>, shared: &mut Shared) {
        self.text.edited(result, shared);
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        match self.phase {
            Phase::Writing => self.render_writing(frame, area),
            Phase::Confirm => self.render_confirm(frame, area),
            Phase::Publishing(_) => {
                frame.render_widget(
                    Paragraph::new(" Publishing…").block(Block::bordered()),
                    area,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Draft;
    use crate::ui::text_area::test_shared;

    fn screen(kind: Kind) -> ComposeScreen {
        ComposeScreen::new(
            Target {
                repo: "alice/notes".parse().expect("repo name"),
                number: PrNumber::new(1).expect("number"),
                head: CommitSha::parse(&"a".repeat(40)).expect("sha"),
            },
            kind,
            None,
        )
    }

    /// A Comment screen with a text, in the Publishing phase of a request that exists (no network: the work
    /// answers at once).
    async fn publishing(shared: &mut Shared) -> (ComposeScreen, RequestId) {
        let mut compose = screen(Kind::Comment);
        compose.text.paste("my text");
        let id = shared.spawn(async { Outcome::TaskFailed });
        compose.phase = Phase::Publishing(id);
        (compose, id)
    }

    #[tokio::test]
    async fn a_failed_publication_task_goes_back_to_writing_with_the_text_and_the_may_have_been_published_message()
     {
        let mut shared = test_shared();
        let (mut compose, id) = publishing(&mut shared).await;
        let left = compose.apply(id, Outcome::TaskFailed, &mut shared);
        assert!(left.is_none(), "the screen took the outcome");
        assert_eq!(compose.phase, Phase::Writing);
        assert_eq!(compose.text.text(), "my text");
        let message = compose.error.as_deref().expect("a message");
        assert!(
            message.starts_with("Cannot publish the comment: ")
                && message.contains("may have been published")
                && message.contains("press Esc, then r to check the PR"),
            "{message}"
        );
        assert!(shared.popped.is_none(), "the screen stays open");
        assert!(compose.editing());
    }

    #[tokio::test]
    async fn an_outcome_of_another_request_or_phase_is_given_back_untouched() {
        let mut shared = test_shared();
        let (mut compose, id) = publishing(&mut shared).await;
        let other = shared.spawn(async { Outcome::TaskFailed });
        assert_ne!(id, other);
        assert!(
            compose
                .apply(other, Outcome::TaskFailed, &mut shared)
                .is_some()
        );
        assert_eq!(compose.phase, Phase::Publishing(id));
        assert!(compose.error.is_none());
        // Not waiting for anything: nothing is claimed.
        let mut idle = screen(Kind::Comment);
        assert!(idle.apply(id, Outcome::TaskFailed, &mut shared).is_some());
        assert_eq!(idle.phase, Phase::Writing);
        // Another kind of outcome for the right id is not ours either.
        let left = compose.apply(id, Outcome::Pulls(Ok(Vec::new())), &mut shared);
        assert!(matches!(left, Some(Outcome::Pulls(_))));
    }

    #[test]
    fn the_key_tables_use_no_reserved_key_and_their_help_fits() {
        assert!(!keys::uses_reserved_key(&WRITING));
        assert!(!keys::uses_reserved_key(&CONFIRM));
        for kind in [
            Kind::Comment,
            Kind::Review(Verdict::Approve),
            Kind::Review(Verdict::RequestChanges),
        ] {
            let mut compose = screen(kind);
            for phase in [Phase::Writing, Phase::Confirm] {
                compose.phase = phase;
                let rows = compose.help();
                assert_eq!(rows.len(), if phase == Phase::Writing { 3 } else { 2 });
                for row in rows {
                    assert!(row.label.chars().count() <= 8 && row.what.chars().count() <= 26);
                }
            }
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A Comment screen showing the message of a failed publication, with `text` in the field.
    fn failed_screen(kind: Kind, text: &str) -> ComposeScreen {
        let mut compose = screen(kind);
        compose.text.paste(text);
        compose.error = Some("Cannot publish: failed".to_owned());
        compose
    }

    fn api(status: u16) -> Error {
        Error::Api {
            status,
            message: String::new(),
        }
    }

    #[test]
    fn uncertain_is_true_for_no_answer_unreadable_answer_and_5xx_only() {
        let yes = [
            Error::Network {
                message: "reset".to_owned(),
            },
            Error::UnexpectedResponse {
                message: "bad json".to_owned(),
            },
            api(500),
            api(502),
            api(599),
        ];
        for error in &yes {
            assert!(uncertain(error), "{error:?}");
        }
        let no = [
            api(499),
            api(600),
            Error::Forbidden {
                message: String::new(),
            },
            Error::NotFound,
            Error::Unprocessable {
                message: String::new(),
            },
            Error::RateLimited { reset_at: None },
            Error::Unauthorized,
        ];
        for error in &no {
            assert!(!uncertain(error), "{error:?}");
        }
    }

    #[tokio::test]
    async fn an_uncertain_error_gives_the_may_have_been_published_message() {
        for error in [
            Error::Network {
                message: "reset".to_owned(),
            },
            api(502),
        ] {
            let mut shared = test_shared();
            let (mut compose, id) = publishing(&mut shared).await;
            assert!(
                compose
                    .apply(id, Outcome::CommentPosted(Err(error)), &mut shared)
                    .is_none()
            );
            assert_eq!(
                compose.error.as_deref(),
                Some(maybe_published("comment").as_str())
            );
        }
        // A certain failure keeps its own message.
        let mut shared = test_shared();
        let (mut compose, id) = publishing(&mut shared).await;
        compose.apply(id, Outcome::CommentPosted(Err(api(404))), &mut shared);
        assert!(
            compose
                .error
                .as_deref()
                .is_some_and(|m| m.contains("HTTP 404")),
            "{:?}",
            compose.error
        );
    }

    #[test]
    fn the_uncertain_message_is_exact_for_a_comment_and_a_review() {
        assert_eq!(
            maybe_published("comment"),
            "Cannot publish the comment: no clear answer from GitHub. It may have been published anyway: press Esc, then r to check the PR; the text comes back when you open the comment again."
        );
        assert!(maybe_published("review").ends_with("open the review again."));
    }

    #[test]
    fn esc_after_a_failure_keeps_the_text_in_one_press() {
        for (kind, word) in [
            (Kind::Comment, "comment"),
            (Kind::Review(Verdict::RequestChanges), "review"),
        ] {
            let mut shared = test_shared();
            let mut compose = failed_screen(kind, "my text\nline 2");
            compose.key_writing(key(KeyCode::Esc), &mut shared);
            assert_eq!(
                shared.popped,
                Some(Some(Signal::Kept(Draft {
                    kind,
                    text: "my text\nline 2".to_owned(),
                })))
            );
            assert!(
                matches!(&shared.notice, Some(Notice::Info(m)) if m == &format!("Text kept: open the {word} again to get it back.")),
                "{:?}",
                shared.notice
            );
        }
    }

    #[test]
    fn esc_after_a_failure_on_a_blank_text_closes_without_a_draft() {
        let mut shared = test_shared();
        let mut compose = failed_screen(Kind::Comment, "  \n ");
        compose.key_writing(key(KeyCode::Esc), &mut shared);
        assert_eq!(shared.popped, Some(None));
    }

    #[tokio::test]
    async fn the_failure_message_stays_while_typing_pasting_and_refusing_and_goes_at_the_next_publication()
     {
        let mut shared = test_shared();
        let mut compose = failed_screen(Kind::Comment, "x");
        compose.key_writing(key(KeyCode::Char('y')), &mut shared);
        compose.paste("more", &mut shared);
        compose.key_writing(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &mut shared,
        );
        assert_eq!(compose.phase, Phase::Confirm);
        assert!(compose.error.is_some());
        compose.key_confirm(key(KeyCode::Char('y')), &mut shared);
        assert!(matches!(compose.phase, Phase::Publishing(_)));
        assert!(compose.error.is_none());
    }

    #[test]
    fn a_draft_fills_the_field_with_the_cursor_at_the_end_and_no_message() {
        let mut shared = test_shared();
        let mut compose = ComposeScreen::new(
            screen(Kind::Comment).target,
            Kind::Comment,
            Some("kept\ntext".to_owned()),
        );
        assert_eq!(compose.text.text(), "kept\ntext");
        assert!(compose.error.is_none() && !compose.esc_armed);
        compose.key_writing(key(KeyCode::Char('!')), &mut shared);
        assert_eq!(compose.text.text(), "kept\ntext!");
        // Without the failure message, Esc is the double Esc again.
        compose.key_writing(key(KeyCode::Esc), &mut shared);
        assert!(shared.popped.is_none() && compose.esc_armed);
    }

    #[test]
    fn a_paste_disarms_the_double_esc() {
        let mut shared = test_shared();
        let mut compose = screen(Kind::Comment);
        compose.key_writing(key(KeyCode::Char('a')), &mut shared);
        compose.key_writing(key(KeyCode::Esc), &mut shared);
        assert!(compose.esc_armed);
        compose.paste("b", &mut shared);
        assert!(!compose.esc_armed);
        compose.key_writing(key(KeyCode::Esc), &mut shared);
        assert!(shared.popped.is_none(), "the second Esc only arms again");
    }

    #[test]
    fn the_confirmation_takes_every_key_but_the_help_key() {
        let mut shared = test_shared();
        let mut compose = screen(Kind::Comment);
        compose.text.paste("t");
        compose.phase = Phase::Confirm;
        for code in [KeyCode::Char('q'), KeyCode::Char('x'), KeyCode::Left] {
            assert!(matches!(
                compose.key_confirm(key(code), &mut shared),
                Flow::Handled
            ));
        }
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            assert!(matches!(
                compose.key_confirm(KeyEvent::new(KeyCode::Char('?'), modifiers), &mut shared),
                Flow::Ignored
            ));
        }
        assert_eq!(compose.phase, Phase::Confirm);
    }

    #[tokio::test]
    async fn publishing_takes_every_key_but_the_help_key_and_esc_and_q_say_to_wait() {
        let mut shared = test_shared();
        let (mut compose, id) = publishing(&mut shared).await;
        for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Left] {
            assert!(matches!(
                compose.key_publishing(key(code), &mut shared),
                Flow::Handled
            ));
            assert!(shared.notice.is_none());
        }
        for code in [KeyCode::Esc, KeyCode::Char('q')] {
            shared.notice = None;
            assert!(matches!(
                compose.key_publishing(key(code), &mut shared),
                Flow::Handled
            ));
            assert!(
                matches!(&shared.notice, Some(Notice::Info(m)) if m == "Publishing: wait for GitHub's answer."),
                "{code:?}"
            );
        }
        assert!(matches!(
            compose.key_publishing(key(KeyCode::Char('?')), &mut shared),
            Flow::Ignored
        ));
        assert_eq!(compose.phase, Phase::Publishing(id));
        assert!(shared.popped.is_none());
    }

    /// What `render_confirm` draws on a `width` × `height` terminal, one row per line.
    fn confirm_drawn(compose: &ComposeScreen, width: u16, height: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| compose.render_confirm(frame, frame.area()))
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

    #[test]
    fn a_long_confirmation_header_is_wrapped_not_cut() {
        let name = "n".repeat(60);
        let mut compose = ComposeScreen::new(
            Target {
                repo: format!("alice/{name}").parse().expect("repo name"),
                number: PrNumber::new(12).expect("number"),
                head: CommitSha::parse(&"a".repeat(40)).expect("sha"),
            },
            Kind::Comment,
            Some("body".to_owned()),
        );
        compose.phase = Phase::Confirm;
        let screen = confirm_drawn(&compose, 40, 14);
        let squashed: String = screen
            .chars()
            .filter(|c| !matches!(c, ' ' | '│' | '\n'))
            .collect();
        assert!(
            squashed.contains(&format!("PublishthisCommentonalice/{name}#12?")),
            "{screen}"
        );
        assert!(screen.contains("body"), "{screen}");
    }

    #[test]
    fn a_cut_text_always_says_how_many_lines_are_left() {
        let text = (1..=10)
            .map(|i| format!("row {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut compose = screen(Kind::Comment);
        compose.text.paste(&text);
        compose.phase = Phase::Confirm;
        // Heights 5 and 7 leave 1 and 3 rows above the 2-row footer: no room for the text at all.
        for height in [5, 6, 7, 8] {
            let screen = confirm_drawn(&compose, 60, height);
            let shown = screen.lines().filter(|l| l.contains("row ")).count();
            let more = screen
                .lines()
                .find_map(|l| {
                    l.trim_matches(|c| c == '│' || c == ' ')
                        .strip_prefix("… ")?
                        .strip_suffix(" more lines")?
                        .parse::<usize>()
                        .ok()
                })
                .unwrap_or_else(|| panic!("height {height}: no '… k more lines':\n{screen}"));
            assert_eq!(shown + more, 10, "height {height}\n{screen}");
        }
    }

    #[test]
    fn a_review_screen_takes_text_while_writing_and_q_is_text_there() {
        let mut shared = test_shared();
        let mut compose = screen(Kind::Review(Verdict::RequestChanges));
        assert!(compose.editing());
        compose.paste("some text", &mut shared);
        assert_eq!(compose.text.text(), "some text");
        assert!(matches!(
            compose.key(key(KeyCode::Char('q')), &mut shared),
            Flow::Handled
        ));
        assert_eq!(compose.text.text(), "some textq");
        assert_eq!(
            compose.title(),
            "alice/notes · #1 · Review: Request changes"
        );
        compose.phase = Phase::Confirm;
        assert!(!compose.editing());
    }

    #[test]
    fn ctrl_s_on_a_blank_text_depends_on_the_kind() {
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        for (kind, confirms) in [
            (Kind::Review(Verdict::Approve), true),
            (Kind::Review(Verdict::RequestChanges), false),
            (Kind::Comment, false),
        ] {
            let mut shared = test_shared();
            let mut compose = screen(kind);
            compose.key(ctrl_s, &mut shared);
            assert_eq!(compose.phase == Phase::Confirm, confirms, "{kind:?}");
        }
    }

    #[tokio::test]
    async fn a_review_422_gives_its_message_but_a_comment_422_does_not() {
        let unprocessable = || Error::Unprocessable {
            message: String::new(),
        };
        let mut shared = test_shared();
        let mut review = screen(Kind::Review(Verdict::Approve));
        let id = shared.spawn(async { Outcome::TaskFailed });
        review.phase = Phase::Publishing(id);
        assert!(
            review
                .apply(id, Outcome::ReviewPosted(Err(unprocessable())), &mut shared)
                .is_none()
        );
        assert_eq!(review.error.as_deref(), Some(REVIEW_REFUSED));
        assert_eq!(review.phase, Phase::Writing);
        let (mut comment, id) = publishing(&mut shared).await;
        comment.apply(
            id,
            Outcome::CommentPosted(Err(unprocessable())),
            &mut shared,
        );
        assert_ne!(comment.error.as_deref(), Some(REVIEW_REFUSED));
    }
}
