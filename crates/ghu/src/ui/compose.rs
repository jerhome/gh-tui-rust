//! Writing, confirming and publishing a Comment (#8) or a Review (#10). Contract: ADR 0003 § 4 « Saisie et
//! confirmation » and § 6.
//!
//! The Comment path is written here: Writing → Confirm → Publishing, answered by [`super::Outcome::CommentPosted`].
//! The Review branches stay on a provisional body (title, `Not available yet.`, `Esc` goes back) until
//! dev[tui-review] writes them ([`super::Outcome::ReviewPosted`]). Success:
//! `Shared::pop(Some(Signal::Published))`. Nothing is sent before the Confirm phase has been answered with `y` or
//! `Enter`.

use gh_client::{
    BodyProblem, CommitSha, Error, MAX_BODY_CHARS, MessageBody, PrNumber, RepoName, Verdict,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::keys::{self, Binding};
use super::text::clean;
use super::text_area::{TextArea, TextKey, wrap};
use super::{Flow, HelpRow, Notice, Outcome, RequestId, Shared, Signal, View, error_message};
use crate::{EditorError, KeyCode, KeyEvent, KeyModifiers};

/// The PR a text is published on.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    /// The repo of the PR.
    pub(crate) repo: RepoName,
    /// The PR.
    pub(crate) number: PrNumber,
    /// `head.sha` seen on the detail (Review `commit_id`).
    #[allow(
        dead_code,
        reason = "ADR 0003: read by dev[tui-review] (Review commit_id)"
    )]
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
}

const WRITING: [Binding<WritingAction>; 2] = [
    Binding {
        label: "Ctrl-S",
        what: "Review, then publish",
        action: WritingAction::Publish,
        keys: &[(KeyCode::Char('s'), KeyModifiers::CONTROL)],
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

/// Shown when the network gave no answer (or the task died): the Comment may exist.
const MAYBE_PUBLISHED: &str = "Cannot publish the comment: no answer from GitHub. It may have been published \
                               anyway: refresh the PR (Esc, r) before trying again.";

/// The compose screen.
pub(crate) struct ComposeScreen {
    target: Target,
    kind: Kind,
    text: TextArea,
    phase: Phase,
    /// The first `Esc` on a text was seen and no other key since.
    esc_armed: bool,
    /// Why the last publication failed, drawn in red above the text until the next key.
    error: Option<String>,
}

impl ComposeScreen {
    /// Opened by `c`, `a` or `x` on the detail, only when `pr_detail::actions` allows it. No request at opening.
    pub(crate) fn new(target: Target, kind: Kind) -> Self {
        Self {
            target,
            kind,
            text: TextArea::new(),
            phase: Phase::Writing,
            esc_armed: false,
            error: None,
        }
    }

    fn key_writing(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        let armed = std::mem::take(&mut self.esc_armed);
        self.error = None;
        if self.text.key(key, shared) == TextKey::Handled {
            return Flow::Handled;
        }
        match keys::lookup(&WRITING, key) {
            Some(WritingAction::Publish) => self.check_text(shared),
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
            None => {}
        }
        Flow::Handled
    }

    /// Ctrl-S: the text must be publishable, then the user is asked to confirm.
    fn check_text(&mut self, shared: &mut Shared) {
        match MessageBody::new(&self.text.text()) {
            Ok(_) => self.phase = Phase::Confirm,
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
            None => return Flow::Ignored,
        }
        Flow::Handled
    }

    /// The only place that sends: one request, never repeated by itself.
    fn publish(&mut self, shared: &mut Shared) {
        let text = self.text.text();
        let body = match MessageBody::new(&text) {
            Ok(body) => body,
            Err(error) => {
                // Cannot happen (the text was checked and the Confirm phase does not edit it); stay safe.
                self.phase = Phase::Writing;
                self.error = Some(error_message("publish the comment", &error));
                return;
            }
        };
        tracing::debug!(chars = text.chars().count(), "publishing a comment");
        let client = shared.client().clone();
        let repo = self.target.repo.clone();
        let number = self.target.number;
        let id = shared.spawn(async move {
            Outcome::CommentPosted(client.post_comment(&repo, number, &body).await)
        });
        self.phase = Phase::Publishing(id);
    }

    fn key_publishing(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => shared.notify(Notice::Info(
                "Publishing: wait for GitHub's answer.".to_owned(),
            )),
            (KeyCode::Enter | KeyCode::Char('y' | 'n'), KeyModifiers::NONE) => {}
            _ => return Flow::Ignored,
        }
        Flow::Handled
    }

    /// Back to Writing with the text intact and a message above it.
    fn failed(&mut self, message: String) {
        self.phase = Phase::Writing;
        self.error = Some(message);
    }

    fn render_provisional(frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Paragraph::new(" Not available yet.").block(Block::bordered()),
            area,
        );
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
        let header = format!(
            " Publish this Comment on {} {}?",
            self.target.repo, self.target.number
        );
        let mut lines = vec![
            Line::from(header).bold(),
            Line::from(""),
            Line::from(" Text:"),
        ];
        // Text rows after the three lines above; the last one says how many are left when the text is cut.
        let room = usize::from(top.height).saturating_sub(lines.len());
        let width = usize::from(top.width.saturating_sub(2)).max(1);
        let text = self.text.text();
        let rows: Vec<String> = text
            .split('\n')
            .flat_map(|line| wrap(&clean(line), width))
            .collect();
        let shown = if rows.len() <= room {
            rows.len()
        } else {
            room.saturating_sub(1)
        };
        lines.extend(
            rows[..shown]
                .iter()
                .map(|row| Line::from(format!(" {row}"))),
        );
        if shown < rows.len() && room > 0 {
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
        match (self.kind, self.phase) {
            (Kind::Review(_), _) | (_, Phase::Publishing(_)) => Vec::new(),
            (Kind::Comment, Phase::Writing) => keys::help_rows(&WRITING),
            (Kind::Comment, Phase::Confirm) => keys::help_rows(&CONFIRM),
        }
    }

    /// Only the Writing phase of a Comment takes text (every key but `Ctrl-C`).
    fn editing(&self) -> bool {
        self.kind == Kind::Comment && self.phase == Phase::Writing
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        if self.kind != Kind::Comment {
            // Provisional Review screens: every key goes to the global table (`Esc` back).
            return Flow::Ignored;
        }
        match self.phase {
            Phase::Writing => self.key_writing(key, shared),
            Phase::Confirm => self.key_confirm(key, shared),
            Phase::Publishing(_) => self.key_publishing(key, shared),
        }
    }

    fn paste(&mut self, text: &str, _shared: &mut Shared) {
        if self.editing() {
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
            Outcome::CommentPosted(Err(Error::Network { .. })) | Outcome::TaskFailed => {
                tracing::warn!("no answer to the comment publication");
                self.failed(MAYBE_PUBLISHED.to_owned());
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
        if self.kind != Kind::Comment {
            Self::render_provisional(frame, area);
            return;
        }
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
    use crate::ui::text_area::test_shared;

    fn screen(kind: Kind) -> ComposeScreen {
        ComposeScreen::new(
            Target {
                repo: "alice/notes".parse().expect("repo name"),
                number: PrNumber::new(1).expect("number"),
                head: CommitSha::parse(&"a".repeat(40)).expect("sha"),
            },
            kind,
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
                && message.contains("refresh the PR (Esc, r) before trying again"),
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
        let mut compose = screen(Kind::Comment);
        for phase in [Phase::Writing, Phase::Confirm] {
            compose.phase = phase;
            let rows = compose.help();
            assert_eq!(rows.len(), 2);
            for row in rows {
                assert!(row.label.chars().count() <= 8 && row.what.chars().count() <= 26);
            }
        }
        assert!(screen(Kind::Review(Verdict::Approve)).help().is_empty());
    }

    #[test]
    fn a_review_screen_stays_provisional_it_takes_no_text_and_no_key() {
        let mut shared = test_shared();
        let mut compose = screen(Kind::Review(Verdict::RequestChanges));
        assert!(!compose.editing());
        compose.paste("ignored", &mut shared);
        assert_eq!(compose.text.text(), "");
        let flow = compose.key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &mut shared,
        );
        assert!(matches!(flow, Flow::Ignored));
        assert_eq!(compose.phase, Phase::Writing);
        assert_eq!(
            compose.title(),
            "alice/notes · #1 · Review: Request changes"
        );
    }
}
