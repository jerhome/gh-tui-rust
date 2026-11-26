//! Ticket #2: a known token never shows on screen, on stdout or stderr, in logs (TRACE) or in `Debug` output.

mod support;

use gh_fake::{Failure, FailureResponse, Method, State};
use ghu::Launch;
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
