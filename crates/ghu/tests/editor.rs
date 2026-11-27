//! Ticket #9: writing a Comment in the external editor (`Ctrl-E`), through the harness against the fake GitHub.
//! The editor is the fake one (`ghu-fake-editor`), named by `EDITOR` in the test environment only.

mod support;

use std::time::Duration;

use gh_fake::{Method, Pull, Repo, State, User};
use ghu::{KeyCode, KeyModifiers};
use support::Harness;

const FAKE_EDITOR: &str = env!("CARGO_BIN_EXE_ghu-fake-editor");

/// `EDITOR` running the fake editor in `mode` (the path quoted: it may hold spaces).
fn fake(mode: &str) -> String {
    format!("\"{FAKE_EDITOR}\" {mode}")
}

fn state() -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = vec![Repo::new("alice", "notes")];
    state.pulls = vec![Pull::new("alice", "notes", 1, "One", "bob")];
    state
}

/// The compose screen of a Comment on PR #1 of `alice/notes`, with `VISUAL` unset and `EDITOR` = `editor`.
async fn compose(editor: &str) -> Harness {
    compose_with(None, Some(editor.to_owned())).await
}

/// The compose screen of a Comment with the given `VISUAL` and `EDITOR` (`None` = unset).
async fn compose_with(visual: Option<String>, editor: Option<String>) -> Harness {
    let mut app = Harness::start_with(state(), move |env, _| {
        env.unset("VISUAL");
        env.unset("EDITOR");
        if let Some(v) = &visual {
            env.set("VISUAL", v);
        }
        if let Some(e) = &editor {
            env.set("EDITOR", e);
        }
    })
    .await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    app.settle().await;
    app.press(KeyCode::Char('c'));
    let screen = app.screen();
    assert!(
        screen.contains("ghu · alice/notes · #1 · Comment"),
        "{screen}"
    );
    app
}

fn ctrl(app: &mut Harness, c: char) {
    app.press_with(KeyCode::Char(c), KeyModifiers::CONTROL);
}

/// The screen with its lines joined by one space.
fn flat(screen: &str) -> String {
    screen.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

/// Asserts the text is unchanged (`kept` still in the field) and the footer starts with `message`.
fn assert_refused(app: &mut Harness, message: &str) {
    let screen = app.screen();
    assert!(flat(&screen).contains(message), "{message}\n{screen}");
    assert!(screen.contains("kept"), "{screen}");
}

#[tokio::test]
async fn the_text_goes_to_the_editor_comes_back_and_is_published() {
    let mut app = compose(&fake("upper")).await;
    app.type_text("hello there");
    ctrl(&mut app, 'e');
    let screen = app.screen();
    assert!(screen.contains("HELLO THERE"), "{screen}");
    assert!(!screen.contains("hello there"), "{screen}");
    ctrl(&mut app, 's');
    app.wait_for("Publish this Comment on alice/notes #1?")
        .await;
    app.press(KeyCode::Char('y'));
    app.wait_for("Comment published.").await;
    app.settle().await;
    let posted: Vec<_> = app
        .fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect();
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(
        posted[0].body.as_ref().and_then(|b| b["body"].as_str()),
        Some("HELLO THERE")
    );
}

#[tokio::test]
async fn a_multi_line_text_written_by_the_editor_fills_the_field() {
    let mut app = compose(&fake(r#"write "Hello\nfrom the editor""#)).await;
    app.type_text("old");
    ctrl(&mut app, 'e');
    let screen = app.screen();
    let row = |l: &str| l.trim_matches(|c| c == '│' || c == ' ').to_owned();
    let hello = screen.lines().position(|l| row(l) == "Hello");
    let from = screen.lines().position(|l| row(l) == "from the editor");
    assert!(
        matches!((hello, from), (Some(h), Some(f)) if f == h + 1),
        "{screen}"
    );
    assert!(!screen.contains("old"), "{screen}");
}

#[tokio::test]
async fn an_editor_that_is_not_found_is_named() {
    let mut app = compose("/nonexistent/ghu-editor").await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "Cannot open the editor: /nonexistent/ghu-editor was not found. Set VISUAL or EDITOR.",
    );
}

#[tokio::test]
async fn visual_is_used_before_editor() {
    let mut app = compose_with(Some("/nonexistent/ghu-visual".into()), Some(fake("upper"))).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "Cannot open the editor: /nonexistent/ghu-visual was not found.",
    );
}

/// A Comment composer with the given `VISUAL` and `EDITOR`, after typing "kept" and Ctrl-E.
async fn edit_with(visual: Option<String>, editor: Option<String>) -> Harness {
    let mut app = compose_with(visual, editor).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    app
}

#[tokio::test]
async fn visual_runs_and_editor_is_not_tried_when_both_work() {
    let mut app = edit_with(
        Some(fake(r#"write "from visual""#)),
        Some(fake(r#"write "from editor""#)),
    )
    .await;
    let screen = app.screen();
    assert!(screen.contains("from visual"), "{screen}");
    assert!(!screen.contains("from editor"), "{screen}");
}

#[tokio::test]
async fn a_blank_visual_falls_back_to_editor() {
    let mut app = edit_with(Some("   ".into()), Some(fake(r#"write "from editor""#))).await;
    let screen = app.screen();
    assert!(screen.contains("from editor"), "{screen}");
}

#[tokio::test]
async fn an_unbalanced_quote_is_refused() {
    let mut app = compose(r#""vim"#).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "Cannot read EDITOR: unbalanced quote or no program.",
    );
}

#[tokio::test]
async fn an_editor_failure_keeps_the_text() {
    let mut app = compose(&fake("fail 3")).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "The editor failed (exit code 3): the text is unchanged.",
    );
}

#[tokio::test]
async fn a_text_that_is_not_utf8_keeps_the_text() {
    let mut app = compose(&fake("bytes")).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "The edited text is not UTF-8: the text is unchanged.",
    );
}

#[tokio::test]
async fn one_mebibyte_is_accepted() {
    let mut app = compose(&fake("fill 1048576")).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    let screen = app.screen();
    assert!(!screen.contains("kept"), "{screen}");
    assert!(!screen.contains("larger than 1 MiB"), "{screen}");
    assert!(screen.contains(&"a".repeat(60)), "{screen}");
}

#[tokio::test]
async fn one_byte_more_than_one_mebibyte_is_too_large() {
    let mut app = compose(&fake("fill 1048577")).await;
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "The edited text is larger than 1 MiB: the text is unchanged.",
    );
}

/// `keep` exits at once without touching the file: an editor that handed the file to another window.
#[tokio::test]
async fn an_editor_that_returns_at_once_is_reported() {
    let mut app = compose(&fake("keep")).await;
    app.set_edit_threshold(Duration::from_secs(30));
    app.type_text("kept");
    ctrl(&mut app, 'e');
    assert_refused(
        &mut app,
        "The editor returned at once: the text is unchanged. Set VISUAL to an editor that waits",
    );
}

/// Counter-test: a fast editor that wrote something is not an editor that returned at once.
#[tokio::test]
async fn a_fast_editor_that_wrote_is_not_reported() {
    let mut app = compose(&fake("write done")).await;
    app.set_edit_threshold(Duration::from_secs(30));
    app.type_text("kept");
    ctrl(&mut app, 'e');
    let screen = app.screen();
    assert!(!screen.contains("returned at once"), "{screen}");
    assert!(
        screen.contains("done") && !screen.contains("kept"),
        "{screen}"
    );
}

/// Counter-test: with the check disabled, `keep` gives the text back unchanged, without error.
#[tokio::test]
async fn without_the_check_an_unchanged_text_comes_back_silently() {
    let mut app = compose(&fake("keep")).await;
    app.set_edit_threshold(Duration::ZERO);
    app.type_text("kept");
    ctrl(&mut app, 'e');
    let screen = app.screen();
    assert!(screen.contains("kept"), "{screen}");
    assert!(!screen.contains("returned at once"), "{screen}");
    assert!(!screen.contains("editor failed"), "{screen}");
}
