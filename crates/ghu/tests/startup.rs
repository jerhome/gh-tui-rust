//! Ticket #2: start-up through `ghu::prepare` (arguments, token, API URL), checked through the harness.

mod support;

use std::sync::atomic::Ordering;

use gh_fake::State;
use ghu::Launch;
use support::{Harness, TestEnv, launch};

/// A token the fake server refuses.
const WRONG: &str = "wrong-token";

#[test]
fn version_prints_the_version_and_exits_0() {
    let (result, out, err) = launch(&["--version"], &TestEnv::default());
    assert!(matches!(result, Launch::Exit(0)), "{result:?}");
    assert_eq!(out.trim(), "ghu 0.1.0");
    assert_eq!(err, "");
}

#[test]
fn help_prints_usage_and_exits_0() {
    let (result, out, err) = launch(&["--help"], &TestEnv::default());
    assert!(matches!(result, Launch::Exit(0)), "{result:?}");
    assert!(out.contains("Usage: ghu"), "{out}");
    assert_eq!(err, "");
}

#[test]
fn unknown_option_is_a_usage_error_with_exit_2() {
    let (result, out, err) = launch(&["--no-such-option"], &TestEnv::default());
    assert!(matches!(result, Launch::Exit(2)), "{result:?}");
    assert_eq!(out, "");
    assert!(err.contains("--no-such-option"), "{err}");
}

#[tokio::test]
async fn gh_token_wins_over_github_token_and_gh() {
    let mut runs = None;
    let mut app = Harness::start_with(State::demo(), |env, _| {
        env.set("GITHUB_TOKEN", WRONG);
        env.gh_auth_token = Some(WRONG.to_owned());
        runs = Some(env.gh_runs.clone());
    })
    .await;
    app.wait_for("alice/notes").await;
    assert_eq!(runs.expect("set").load(Ordering::SeqCst), 0, "gh was run");
}

#[tokio::test]
async fn github_token_is_used_when_gh_token_is_unset() {
    let mut runs = None;
    let mut app = Harness::start_with(State::demo(), |env, fake| {
        env.unset("GH_TOKEN");
        env.set("GITHUB_TOKEN", &fake.with_state(|s| s.token.clone()));
        env.gh_auth_token = Some(WRONG.to_owned());
        runs = Some(env.gh_runs.clone());
    })
    .await;
    app.wait_for("alice/notes").await;
    assert_eq!(runs.expect("set").load(Ordering::SeqCst), 0, "gh was run");
}

#[tokio::test]
async fn gh_auth_token_is_used_when_no_variable_is_set() {
    let mut runs = None;
    let mut app = Harness::start_with(State::demo(), |env, fake| {
        env.unset("GH_TOKEN");
        env.gh_auth_token = Some(format!("{}\n", fake.with_state(|s| s.token.clone())));
        runs = Some(env.gh_runs.clone());
    })
    .await;
    app.wait_for("alice/notes").await;
    assert_eq!(runs.expect("set").load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_empty_gh_token_counts_as_unset() {
    let mut app = Harness::start_with(State::demo(), |env, fake| {
        env.set("GH_TOKEN", "");
        env.set("GITHUB_TOKEN", &fake.with_state(|s| s.token.clone()));
    })
    .await;
    app.wait_for("alice/notes").await;
}

#[test]
fn without_a_token_ghu_exits_1_and_says_how_to_provide_one() {
    let mut env = TestEnv::default();
    env.set("GH_TOKEN", "");
    env.set("GITHUB_TOKEN", "  ");
    env.gh_auth_token = Some("\n".to_owned());
    let (result, out, err) = launch(&[], &env);
    assert!(matches!(result, Launch::Exit(1)), "{result:?}");
    assert_eq!(out, "");
    assert!(err.contains("GH_TOKEN"), "{err}");
    assert!(err.contains("gh auth login"), "{err}");
}

#[test]
fn an_invalid_api_url_exits_1_naming_the_variable() {
    for url in ["not a url", "http://api.example.com", "ftp://127.0.0.1"] {
        let mut env = TestEnv::default();
        env.set("GH_TOKEN", "some-token");
        env.set("GHU_API_URL", url);
        let (result, out, err) = launch(&[], &env);
        assert!(matches!(result, Launch::Exit(1)), "{url}: {result:?}");
        assert_eq!(out, "", "{url}");
        assert!(err.contains("GHU_API_URL"), "{url}: {err}");
    }
}

#[test]
fn the_api_url_defaults_to_the_public_github_api() {
    let mut env = TestEnv::default();
    env.set("GH_TOKEN", "some-token");
    env.set("GHU_API_URL", "");
    let (result, _, err) = launch(&[], &env);
    let Launch::Tui(ctx) = result else {
        panic!("expected the TUI, got {result:?}: {err}");
    };
    assert!(
        format!("{ctx:?}").contains("https://api.github.com"),
        "{ctx:?}"
    );
}
