//! Ticket #2: Repo list, loading indicator, help, quit, errors (through the harness only).

mod support;

use std::time::Duration;

use gh_fake::{Failure, FailureResponse, Method, State};
use ghu::{KeyCode, KeyModifiers};
use support::Harness;

/// The line of `screen` that contains `needle`.
fn line_of<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not on screen:\n{screen}"))
}

#[tokio::test]
async fn repo_list_shows_the_users_own_repos() {
    let mut app = Harness::start(State::demo()).await;
    app.wait_for("alice/notes").await;
    let screen = app.screen();
    for repo in ["alice/dotfiles", "alice/notes", "alice/secret-garden"] {
        assert!(screen.contains(repo), "{repo} missing:\n{screen}");
    }
    assert!(screen.contains("Plain-text notes and snippets"), "{screen}");
}

#[tokio::test]
async fn forks_are_marked() {
    let mut app = Harness::start(State::demo()).await;
    app.wait_for("alice/notes").await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "alice/dotfiles").contains("fork"),
        "{screen}"
    );
    assert!(
        !line_of(&screen, "alice/notes").contains("fork"),
        "{screen}"
    );
}

#[tokio::test]
async fn a_slow_answer_shows_loading_and_keys_still_work() {
    let mut state = State::demo();
    state.latency = Duration::from_millis(500);
    let mut app = Harness::start(state).await;
    let screen = app.screen();
    assert!(screen.contains("Loading"), "{screen}");

    app.press(KeyCode::Char('?'));
    let screen = app.screen();
    assert!(
        screen.contains("Toggle this help"),
        "help not shown:\n{screen}"
    );
    assert!(
        screen.contains("Loading"),
        "answer came too early:\n{screen}"
    );

    app.press(KeyCode::Char('?'));
    app.wait_for("alice/notes").await;
    let screen = app.screen();
    assert!(!screen.contains("Loading"), "{screen}");
    assert!(!screen.contains("Toggle this help"), "{screen}");
}

#[tokio::test]
async fn esc_closes_the_help_and_does_nothing_on_the_first_screen() {
    let mut app = Harness::start(State::demo()).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Char('?'));
    assert!(app.screen().contains("Toggle this help"));
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(!screen.contains("Toggle this help"), "{screen}");
    app.press(KeyCode::Esc);
    assert!(!app.is_finished());
    assert!(app.screen().contains("alice/notes"));
}

#[tokio::test]
async fn q_quits() {
    let mut app = Harness::start(State::demo()).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Char('q'));
    assert!(app.is_finished());
}

#[tokio::test]
async fn q_quits_from_the_help() {
    let mut app = Harness::start(State::demo()).await;
    app.press(KeyCode::Char('?'));
    app.press(KeyCode::Char('q'));
    assert!(app.is_finished());
}

#[tokio::test]
async fn ctrl_c_quits_even_while_loading() {
    let mut state = State::demo();
    state.latency = Duration::from_millis(500);
    let mut app = Harness::start(state).await;
    app.press_with(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(app.is_finished());
}

#[tokio::test]
async fn a_plain_c_does_not_quit() {
    let mut app = Harness::start(State::demo()).await;
    app.press(KeyCode::Char('c'));
    assert!(!app.is_finished());
}

#[tokio::test]
async fn a_refused_token_shows_an_explicit_message() {
    let mut state = State::demo();
    state.failures.push(Failure {
        method: Method::Get,
        path: "/user/repos".into(),
        response: FailureResponse::Unauthorized,
        remaining: None,
    });
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("GitHub rejected the token"), "{screen}");
    assert!(screen.contains("gh auth login"), "{screen}");
    assert!(!screen.contains("alice/notes"), "{screen}");
}

#[tokio::test]
async fn a_token_refused_by_the_server_also_shows_the_message() {
    let mut app = Harness::start_with(State::demo(), |env, _| {
        env.set("GH_TOKEN", "not-the-right-one");
    })
    .await;
    app.wait_for("GitHub rejected the token").await;
}
