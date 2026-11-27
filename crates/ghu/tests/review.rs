//! Ticket #10: writing, confirming and publishing a Review (Approve, Request changes) through the harness only,
//! against the fake GitHub. The signed-in user of every state is `alice`; the PRs live in `alice/notes`.

mod support;

use std::time::Duration;

use gh_fake::{
    Failure, FailureResponse, FakeGitHub, Method, Pull, PullReview, PullState, RecordedRequest,
    Repo, ReviewState, State, User,
};
use ghu::{KeyCode, KeyModifiers};
use support::Harness;

const HINT: &str = "Ctrl-S publish… · Ctrl-E editor · Esc cancel";
const REVIEWS: &str = "/repos/alice/notes/pulls/1/reviews";
const HEAD: &str = "00000000000000000000000000000000000000a1";

fn state_with(pulls: Vec<Pull>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = vec![Repo::new("alice", "notes")];
    state.pulls = pulls;
    state
}

/// An open PR #1 of `alice/notes` whose head is `HEAD`.
fn pr(author: &str) -> Pull {
    let mut pull = Pull::new("alice", "notes", 1, "One", author);
    pull.head_sha = HEAD.into();
    pull
}

async fn open_detail_in(state: State, closed_tab: bool) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    if closed_tab {
        app.press(KeyCode::Tab);
        app.wait_for("One").await;
    }
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    app.settle().await;
    app
}

async fn open_detail(state: State) -> Harness {
    open_detail_in(state, false).await
}

/// The compose screen of a Review (key `a` or `x`) on PR #1 of bob.
async fn review(key: char) -> Harness {
    review_on(state_with(vec![pr("bob")]), key).await
}

async fn review_on(state: State, key: char) -> Harness {
    let mut app = open_detail(state).await;
    app.press(KeyCode::Char(key));
    let title = if key == 'a' {
        "Review: Approve"
    } else {
        "Review: Request changes"
    };
    let screen = app.screen();
    assert!(
        screen.contains(&format!("ghu · alice/notes · #1 · {title}")) && screen.contains(HINT),
        "{screen}"
    );
    app
}

fn ctrl_s(app: &mut Harness) {
    app.press_with(KeyCode::Char('s'), KeyModifiers::CONTROL);
}

fn flat(screen: &str) -> String {
    screen.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

fn posts(app: &Harness) -> Vec<RecordedRequest> {
    app.fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect()
}

fn assert_on_screen(app: &mut Harness, needle: &str) {
    let screen = app.screen();
    assert!(
        screen.contains(needle),
        "{needle:?} not on screen:\n{screen}"
    );
}

/// The only POST the fake received: a Review on PR #1.
fn only_review(app: &Harness) -> RecordedRequest {
    let mut posts = posts(app);
    assert_eq!(posts.len(), 1, "{posts:?}");
    let post = posts.remove(0);
    assert_eq!(post.path, REVIEWS);
    assert!(post.body.is_some(), "a JSON body");
    post
}

/// Injects `response` once on the POST of the reviews of PR #1.
fn fail_once(app: &Harness, response: FailureResponse) {
    app.fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Post,
            path: REVIEWS.into(),
            response,
            remaining: Some(1),
        });
    });
}

const REFUSED: &str = "GitHub refused the review (422). If you started a review on the website (pending review), submit or delete it there; or the PR changed: press Esc, then r to refresh; the text comes back when you open the review again.";
const MAYBE: &str = "Cannot publish the review: no clear answer from GitHub. It may have been published anyway: press Esc, then r to check the PR; the text comes back when you open the review again.";

// ---------------------------------------------------------------------------------------------------------------
// Approve

#[tokio::test]
async fn approve_without_text_confirms_with_its_verdict_and_sends_no_body() {
    let mut app = review('a').await;
    ctrl_s(&mut app);
    let screen = app.screen();
    assert!(
        screen.contains("Publish this Review on alice/notes #1?")
            && screen.contains("Verdict: Approve")
            && screen.contains("(no text)")
            && screen.contains("y / Enter publish · n / Esc back to the text"),
        "{screen}"
    );
    assert!(posts(&app).is_empty(), "nothing is sent before y");
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Approve.").await;
    app.wait_for("Verdicts: @alice Approve").await;
    app.settle().await;
    let body = only_review(&app).body.unwrap();
    assert_eq!(body.as_object().map(|o| o.len()), Some(2), "{body}");
    assert_eq!(body["event"], "APPROVE");
    assert_eq!(body["commit_id"], HEAD);
}

#[tokio::test]
async fn a_blank_text_on_approve_is_sent_as_no_body() {
    let mut app = review('a').await;
    app.type_text("  ");
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "(no text)");
    app.press(KeyCode::Enter);
    app.wait_for("Review published: Approve.").await;
    app.settle().await;
    let body = only_review(&app).body.unwrap();
    assert!(body.get("body").is_none(), "{body}");
}

#[tokio::test]
async fn approve_with_text_sends_it_and_the_detail_shows_it() {
    let mut app = review('a').await;
    app.type_text("Looks good");
    ctrl_s(&mut app);
    let screen = app.screen();
    assert!(
        screen.contains("Verdict: Approve") && screen.contains("Looks good"),
        "{screen}"
    );
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Approve.").await;
    app.wait_for("Verdicts: @alice Approve").await;
    app.settle().await;
    let body = only_review(&app).body.unwrap();
    assert_eq!(body["event"], "APPROVE");
    assert_eq!(body["body"], "Looks good");
    assert_eq!(body["commit_id"], HEAD);
    assert_on_screen(&mut app, "Looks good");
}

// ---------------------------------------------------------------------------------------------------------------
// Request changes

#[tokio::test]
async fn request_changes_refuses_a_blank_text_and_stays_writable() {
    let mut app = review('x').await;
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "Request changes needs a text.");
    assert_on_screen(&mut app, HINT);
    app.type_text("   ");
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "Request changes needs a text.");
    assert!(!app.screen().contains("Publish this Review"));
    assert!(posts(&app).is_empty());
    app.type_text("fix it");
    assert_on_screen(&mut app, "fix it");
}

#[tokio::test]
async fn request_changes_confirms_with_its_verdict_and_publishes_event_text_and_commit() {
    let mut app = review('x').await;
    app.type_text("Please fix");
    ctrl_s(&mut app);
    let screen = app.screen();
    assert!(
        screen.contains("Publish this Review on alice/notes #1?")
            && screen.contains("Verdict: Request changes")
            && screen.contains("Please fix"),
        "{screen}"
    );
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Request changes.").await;
    app.wait_for("Verdicts: @alice Request changes").await;
    app.settle().await;
    let body = only_review(&app).body.unwrap();
    assert_eq!(body.as_object().map(|o| o.len()), Some(3), "{body}");
    assert_eq!(body["event"], "REQUEST_CHANGES");
    assert_eq!(body["body"], "Please fix");
    assert_eq!(body["commit_id"], HEAD);
}

#[tokio::test]
async fn n_and_esc_on_the_confirmation_go_back_to_the_intact_text() {
    for back in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut app = review('x').await;
        app.type_text("keep me");
        ctrl_s(&mut app);
        app.press(back);
        assert_on_screen(&mut app, HINT);
        assert_on_screen(&mut app, "keep me");
        assert!(posts(&app).is_empty());
    }
}

#[tokio::test]
async fn the_text_limit_is_checked_for_a_review_too() {
    let mut app = review('x').await;
    app.paste(&"a".repeat(65537));
    ctrl_s(&mut app);
    assert_on_screen(
        &mut app,
        "The text is too long: 65537 characters (GitHub allows 65536).",
    );
    assert!(posts(&app).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Where a Review is offered

#[tokio::test]
async fn an_own_pr_offers_no_verdict_but_still_a_comment() {
    let mut app = open_detail(state_with(vec![pr("alice")])).await;
    assert_on_screen(
        &mut app,
        "c Comment · d Diff · Verdicts are not offered on your own PR.",
    );
    for key in ['a', 'x'] {
        app.press(KeyCode::Char(key));
        let screen = app.screen();
        assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
        assert!(!screen.contains(HINT), "{screen}");
    }
    app.press(KeyCode::Char('c'));
    assert_on_screen(&mut app, "ghu · alice/notes · #1 · Comment");
    assert!(posts(&app).is_empty());
}

#[tokio::test]
async fn a_closed_or_merged_pr_and_an_archived_repo_offer_no_verdict() {
    for pull_state in [PullState::Closed, PullState::Merged] {
        let mut pull = pr("bob");
        pull.state = pull_state;
        let mut app = open_detail_in(state_with(vec![pull]), true).await;
        for key in ['a', 'x'] {
            app.press(KeyCode::Char(key));
            let screen = app.screen();
            assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
            assert!(!screen.contains(HINT), "{screen}");
        }
        assert!(posts(&app).is_empty());
    }
    // Repo archived after the load: the refresh shows it.
    let mut app = open_detail(state_with(vec![pr("bob")])).await;
    app.fake.with_state(|s| s.repos[0].archived = true);
    app.press(KeyCode::Char('r'));
    app.wait_for("This repo is archived: read-only.").await;
    app.settle().await;
    assert_on_screen(&mut app, "This repo is archived: read-only.");
    for key in ['a', 'x'] {
        app.press(KeyCode::Char(key));
        let screen = app.screen();
        assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
        assert!(!screen.contains(HINT), "{screen}");
    }
    assert!(posts(&app).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Failures

#[tokio::test]
async fn a_pending_review_of_the_user_gives_the_422_message_and_keeps_the_text() {
    let mut state = state_with(vec![pr("bob")]);
    state.pulls[0].reviews.push(PullReview {
        id: 900,
        author: Some("alice".into()),
        state: ReviewState::Pending,
        body: String::new(),
        submitted_at: None,
        commit_id: HEAD.into(),
    });
    let mut app = review_on(state, 'x').await;
    app.type_text("my words");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("GitHub refused the review (422).").await;
    app.settle().await;
    let screen = app.screen();
    assert!(flat(&screen).contains(REFUSED), "{screen}");
    assert!(
        screen.contains("my words") && screen.contains(HINT) && !screen.contains("Publishing…"),
        "{screen}"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    app.settle().await;
    assert_eq!(posts(&app).len(), 1, "no second request");
}

#[tokio::test]
async fn a_422_for_a_changed_pr_has_the_same_message_and_a_retry_goes_through() {
    let mut app = review('a').await;
    fail_once(
        &app,
        FailureResponse::Unprocessable {
            message: "commit_id is not part of the pull request".into(),
        },
    );
    app.type_text("fine");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("GitHub refused the review (422).").await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        flat(&screen).contains(REFUSED) && screen.contains("fine"),
        "{screen}"
    );
    // The user decides the retry; the failure was injected once.
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Approve.").await;
    assert_eq!(posts(&app).len(), 2);
}

#[tokio::test]
async fn other_refusals_use_the_review_wording() {
    let mut app = review('a').await;
    fail_once(
        &app,
        FailureResponse::Forbidden {
            message: "Resource not accessible".into(),
        },
    );
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the review: GitHub denied access (403).")
        .await;
    assert_eq!(posts(&app).len(), 1);
}

#[tokio::test]
async fn a_500_may_have_published_the_review_and_says_so() {
    // The fake has no 502 answer: every 5xx goes through the same rule (unit test `uncertain`).
    let mut app = review('x').await;
    fail_once(&app, FailureResponse::ServerError);
    app.type_text("maybe");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the review").await;
    app.settle().await;
    let screen = app.screen();
    assert!(flat(&screen).contains(MAYBE), "{screen}");
    assert!(
        screen.contains("maybe") && screen.contains(HINT),
        "{screen}"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(posts(&app).len(), 1, "never resent by itself");
}

#[tokio::test]
async fn a_lost_network_says_it_may_have_been_published() {
    let mut app = review('a').await;
    app.type_text("net");
    ctrl_s(&mut app);
    let old = {
        let placeholder = FakeGitHub::start(state_with(Vec::new())).await.unwrap();
        std::mem::replace(&mut app.fake, placeholder)
    };
    drop(old);
    tokio::time::sleep(Duration::from_millis(100)).await;
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the review").await;
    app.settle().await;
    let screen = app.screen();
    assert!(flat(&screen).contains(MAYBE), "{screen}");
    assert!(screen.contains("net"), "{screen}");
}

// ---------------------------------------------------------------------------------------------------------------
// Drafts, q

/// A Request changes screen whose publication failed with a 500, with "my draft" in the field.
async fn failed_request_changes() -> Harness {
    let mut app = review('x').await;
    fail_once(&app, FailureResponse::ServerError);
    app.type_text("my draft");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the review").await;
    app.settle().await;
    app
}

#[tokio::test]
async fn a_draft_comes_back_for_the_same_verdict_only() {
    let mut app = failed_request_changes().await;
    app.press(KeyCode::Esc);
    app.wait_for("Text kept: open the review again to get it back.")
        .await;
    app.settle().await;
    assert_on_screen(&mut app, "Description");
    // Another verdict and a Comment start empty; the draft stays for its own Verdict.
    app.press(KeyCode::Char('a'));
    let screen = app.screen();
    assert!(
        screen.contains("Review: Approve") && !screen.contains("my draft"),
        "{screen}"
    );
    app.press(KeyCode::Esc);
    app.press(KeyCode::Char('c'));
    assert!(!app.screen().contains("my draft"));
    app.press(KeyCode::Esc);
    app.press(KeyCode::Char('x'));
    let screen = app.screen();
    assert!(
        screen.contains("Review: Request changes") && screen.contains("my draft"),
        "{screen}"
    );
    // Reopened with a draft: no failure message, and publishing it works.
    assert!(!screen.contains("Cannot publish"), "{screen}");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Request changes.").await;
    app.settle().await;
    assert_eq!(posts(&app).len(), 2);
}

/// After a 422: Esc keeps the text, `r` reloads the PR, the same Verdict gets the text back, the other starts empty.
#[tokio::test]
async fn after_a_422_esc_and_a_refresh_the_same_verdict_gets_its_text_back_and_the_other_starts_empty()
 {
    for (key, other, title, other_title) in [
        ('x', 'a', "Review: Request changes", "Review: Approve"),
        ('a', 'x', "Review: Approve", "Review: Request changes"),
    ] {
        let mut app = review(key).await;
        fail_once(
            &app,
            FailureResponse::Unprocessable {
                message: "Unprocessable".into(),
            },
        );
        app.type_text("kept words");
        ctrl_s(&mut app);
        app.press(KeyCode::Char('y'));
        app.wait_for("GitHub refused the review (422).").await;
        app.settle().await;
        app.press(KeyCode::Esc);
        app.wait_for("Text kept: open the review again to get it back.")
            .await;
        app.settle().await;
        let gets = |app: &Harness| {
            app.fake
                .requests()
                .into_iter()
                .filter(|r| r.method == Method::Get)
                .count()
        };
        let before = gets(&app);
        app.press(KeyCode::Char('r'));
        for _ in 0..100 {
            if gets(&app) > before {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gets(&app) > before, "the refresh reloaded the PR");
        app.settle().await;
        assert_on_screen(&mut app, "Description");
        app.press(KeyCode::Char(key));
        let screen = app.screen();
        assert!(
            screen.contains(title)
                && screen.contains("kept words")
                && !screen.contains("GitHub refused"),
            "{screen}"
        );
        app.press(KeyCode::Esc);
        app.press(KeyCode::Esc);
        app.settle().await;
        app.press(KeyCode::Char(other));
        let screen = app.screen();
        assert!(
            screen.contains(other_title) && !screen.contains("kept words"),
            "{screen}"
        );
    }
}

#[tokio::test]
async fn q_does_nothing_on_the_confirmation_and_while_publishing() {
    let mut app = review('x').await;
    app.type_text("keep");
    assert_on_screen(&mut app, "keep");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('q'));
    assert!(!app.is_finished());
    assert_on_screen(&mut app, "Verdict: Request changes");
    app.fake
        .with_state(|s| s.latency = Duration::from_millis(400));
    app.press(KeyCode::Char('y'));
    assert_on_screen(&mut app, "Publishing…");
    app.press(KeyCode::Char('q'));
    app.press(KeyCode::Esc);
    assert!(!app.is_finished());
    let screen = app.screen();
    assert!(
        screen.contains("Publishing: wait for GitHub's answer.") && screen.contains("Publishing…"),
        "{screen}"
    );
    app.wait_for("Review published: Request changes.").await;
    app.settle().await;
    assert_eq!(only_review(&app).body.unwrap()["body"], "keep");
}

#[tokio::test]
async fn q_is_text_while_writing_a_review() {
    let mut app = review('a').await;
    app.type_text("qq");
    assert!(!app.is_finished());
    assert_on_screen(&mut app, "qq");
}
