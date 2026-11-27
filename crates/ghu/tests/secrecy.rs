//! Ticket #2: a known token never shows on screen, on stdout or stderr, in logs (TRACE) or in `Debug` output.

mod support;

use gh_fake::{Failure, FailureResponse, Method, Pull, PullReview, ReviewState, State};
use ghu::Launch;
use ghu::{KeyCode, KeyModifiers};
use support::{Harness, LogCapture, TestEnv, launch};

const SECRET: &str = "ghp_KnownSecret0123456789abcdef";

fn assert_hidden(what: &str, text: &str) {
    assert!(!text.contains(SECRET), "token leaked in {what}:\n{text}");
}

fn state_with_secret() -> State {
    let mut state = State::demo();
    state.token = SECRET.into();
    state
}

#[tokio::test]
async fn the_token_never_shows_on_screen_or_in_logs() {
    let logs = LogCapture::start();
    let mut app = Harness::start(state_with_secret()).await;
    app.wait_for("alice/notes").await;
    assert_eq!(
        app.fake.requests()[0].token.as_deref(),
        Some(SECRET),
        "the fake did not receive the known token"
    );
    assert_hidden("the screen", &app.screen());
    let text = logs.text();
    assert!(!text.is_empty(), "nothing was captured");
    assert_hidden("the logs", &text);
}

#[tokio::test]
async fn the_token_never_shows_when_github_refuses_it() {
    let logs = LogCapture::start();
    let mut state = state_with_secret();
    state.failures.push(Failure {
        method: Method::Get,
        path: "/user/repos".into(),
        response: FailureResponse::Unauthorized,
        remaining: None,
    });
    let mut app = Harness::start(state).await;
    app.wait_for("GitHub rejected the token").await;
    assert_hidden("the screen", &app.screen());
    assert_hidden("the logs", &logs.text());
}

#[test]
fn the_token_never_shows_in_start_up_output_or_debug() {
    let logs = LogCapture::start();
    for (name, args) in [
        ("GH_TOKEN", vec![]),
        ("GITHUB_TOKEN", vec![]),
        ("GH_TOKEN", vec!["--help"]),
    ] {
        let mut env = TestEnv::default();
        env.set(name, SECRET);
        env.set("GHU_API_URL", "http://127.0.0.1:9");
        let (result, out, err) = launch(&args, &env);
        assert_hidden("stdout", &out);
        assert_hidden("stderr", &err);
        assert_hidden("Debug", &format!("{result:?}"));
        assert_hidden("Debug (alternate)", &format!("{result:#?}"));
        if let Launch::Tui(ctx) = result {
            assert_hidden("Debug of Context", &format!("{ctx:?}"));
        }
    }

    let mut env = TestEnv {
        gh_auth_token: Some(SECRET.into()),
        ..TestEnv::default()
    };
    env.set("GHU_API_URL", "http://example.com");
    let (result, out, err) = launch(&[], &env);
    assert!(matches!(result, Launch::Exit(1)), "{result:?}");
    assert_hidden("stdout", &out);
    assert_hidden("stderr", &err);
    assert_hidden("the logs", &logs.text());
}

/// A detail of PR #1 of `alice/notes` (by bob) with the known token, and the Review screen of `key` open.
async fn review_screen(state: State, key: char) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    app.settle().await;
    app.press(KeyCode::Char(key));
    app.wait_for("Ctrl-S publish…").await;
    app
}

fn review_state() -> State {
    let mut state = State::new(gh_fake::User {
        login: "alice".into(),
        id: 1,
    });
    state.token = SECRET.into();
    state.repos = vec![gh_fake::Repo::new("alice", "notes")];
    state.pulls = vec![Pull::new("alice", "notes", 1, "One", "bob")];
    state
}

#[tokio::test]
async fn the_token_never_shows_when_a_review_is_published() {
    let logs = LogCapture::start();
    let mut app = review_screen(review_state(), 'x').await;
    app.type_text("needs work");
    app.press_with(KeyCode::Char('s'), KeyModifiers::CONTROL);
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Request changes.").await;
    app.settle().await;
    let post = app
        .fake
        .requests()
        .into_iter()
        .find(|r| r.method == Method::Post)
        .expect("the review was posted");
    assert_eq!(post.token.as_deref(), Some(SECRET));
    assert_hidden("the screen", &app.screen());
    assert_hidden("the logs", &logs.text());
    // ADR 0003 § 7: the published text is never logged.
    assert!(
        !logs.text().contains("needs work"),
        "the text is in the logs"
    );
}

#[tokio::test]
async fn the_text_of_an_approve_is_not_logged_either() {
    let logs = LogCapture::start();
    let mut app = review_screen(review_state(), 'a').await;
    app.type_text("looks fine to me");
    app.press_with(KeyCode::Char('s'), KeyModifiers::CONTROL);
    app.press(KeyCode::Char('y'));
    app.wait_for("Review published: Approve.").await;
    app.settle().await;
    assert_hidden("the logs", &logs.text());
    assert!(
        !logs.text().contains("looks fine to me"),
        "the text is in the logs"
    );
}

#[tokio::test]
async fn the_token_never_shows_when_github_refuses_a_review_with_a_422() {
    let logs = LogCapture::start();
    let mut state = review_state();
    state.pulls[0].reviews.push(PullReview {
        id: 900,
        author: Some("alice".into()),
        state: ReviewState::Pending,
        body: String::new(),
        submitted_at: None,
        commit_id: String::new(),
    });
    let mut app = review_screen(state, 'a').await;
    app.press_with(KeyCode::Char('s'), KeyModifiers::CONTROL);
    app.press(KeyCode::Char('y'));
    app.wait_for("GitHub refused the review (422).").await;
    app.settle().await;
    assert_hidden("the screen", &app.screen());
    assert_hidden("the logs", &logs.text());
}
