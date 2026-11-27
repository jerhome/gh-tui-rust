//! Ticket #5: the PR list of one repo (through the harness only). Every test uses a state with one affiliated
//! repo, `alice/notes`, so `Enter` on the Repo list opens it.

mod support;

use std::time::Duration;

use gh_fake::{Failure, FailureResponse, Method, Pull, PullState, Repo, State, User};
use ghu::KeyCode;
use support::Harness;

/// A state with the single repo `alice/notes` and `pulls`.
fn state_with(pulls: Vec<Pull>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = vec![Repo::new("alice", "notes")];
    state.pulls = pulls;
    state
}

/// A PR of `alice/notes` updated on 2026-03-`day`.
fn pr(number: u64, title: &str, author: &str, pull_state: PullState, day: u32) -> Pull {
    let mut pull = Pull::new("alice", "notes", number, title, author);
    pull.state = pull_state;
    pull.updated_at = format!("2026-03-{day:02}T12:30:00Z");
    pull
}

/// Starts the app and opens the PR list of `alice/notes`; waits for the first answer.
async fn open_pulls(state: State) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app
}

fn line_of<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not on screen:\n{screen}"))
}

fn pulls_failing(response: FailureResponse) -> State {
    let mut state = state_with(vec![pr(1, "Fix", "bob", PullState::Open, 3)]);
    state.failures.push(Failure {
        method: Method::Get,
        path: "/repos/alice/notes/pulls".into(),
        response,
        remaining: None,
    });
    state
}

#[tokio::test]
async fn a_row_shows_number_state_title_author_and_date() {
    let mut app = open_pulls(state_with(vec![pr(
        7,
        "Fix typo in the README",
        "bob",
        PullState::Open,
        3,
    )]))
    .await;
    let screen = app.screen();
    assert!(
        screen.contains("alice/notes · Pull requests (open)"),
        "{screen}"
    );
    let line = line_of(&screen, "#7");
    for part in ["open", "Fix typo in the README", "@bob", "2026-03-03"] {
        assert!(line.contains(part), "{part} missing in {line:?}");
    }
}

#[tokio::test]
async fn a_draft_pr_is_marked() {
    let mut draft = pr(2, "WIP: search", "carol", PullState::Open, 5);
    draft.draft = true;
    let mut app = open_pulls(state_with(vec![
        pr(1, "Plain one", "bob", PullState::Open, 3),
        draft,
    ]))
    .await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "WIP: search").contains("draft"),
        "{screen}"
    );
    assert!(!line_of(&screen, "Plain one").contains("draft"), "{screen}");
}

#[tokio::test]
async fn the_most_recently_updated_pr_comes_first() {
    let mut app = open_pulls(state_with(vec![
        pr(1, "Oldest", "bob", PullState::Open, 1),
        pr(2, "Newest", "bob", PullState::Open, 20),
        pr(3, "Middle", "bob", PullState::Open, 10),
    ]))
    .await;
    let screen = app.screen();
    let at = |title: &str| {
        screen
            .find(title)
            .unwrap_or_else(|| panic!("{title}:\n{screen}"))
    };
    assert!(
        at("Newest") < at("Middle") && at("Middle") < at("Oldest"),
        "{screen}"
    );
}

#[tokio::test]
async fn tab_shows_closed_and_merged_prs_with_the_right_request() {
    let mut app = open_pulls(state_with(vec![
        pr(1, "Still open", "bob", PullState::Open, 3),
        pr(2, "Was closed", "bob", PullState::Closed, 4),
        pr(3, "Was merged", "carol", PullState::Merged, 5),
    ]))
    .await;
    assert!(!app.screen().contains("Was closed"));
    app.press(KeyCode::Tab);
    app.wait_for("Was merged").await;
    let screen = app.screen();
    assert!(screen.contains("(closed / merged)"), "{screen}");
    assert!(
        line_of(&screen, "Was closed").contains("closed"),
        "{screen}"
    );
    assert!(
        line_of(&screen, "Was merged").contains("merged"),
        "{screen}"
    );
    assert!(!screen.contains("Still open"), "{screen}");
    let queried = app.fake.requests().iter().any(|r| {
        r.path == "/repos/alice/notes/pulls"
            && r.query
                .as_deref()
                .is_some_and(|q| q.contains("state=closed&sort=updated&direction=desc"))
    });
    assert!(queried, "{:?}", app.fake.requests());

    app.press(KeyCode::Tab);
    app.wait_for("Still open").await;
    assert!(app.screen().contains("(open)"));
}

#[tokio::test]
async fn r_reloads_and_shows_a_pr_added_on_the_server() {
    let mut app = open_pulls(state_with(vec![pr(1, "First", "bob", PullState::Open, 3)])).await;
    app.fake.with_state(|s| {
        s.pulls
            .push(pr(2, "Added later", "bob", PullState::Open, 9))
    });
    assert!(!app.screen().contains("Added later"));
    app.press(KeyCode::Char('r'));
    app.wait_for("Added later").await;
}

#[tokio::test]
async fn esc_returns_to_the_unchanged_repo_list() {
    let mut app = open_pulls(state_with(vec![pr(1, "First", "bob", PullState::Open, 3)])).await;
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(screen.contains("ghu · Repo list"), "{screen}");
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(!screen.contains("First"), "{screen}");
}

#[tokio::test]
async fn an_empty_open_list_and_an_empty_closed_list_have_their_own_message() {
    let mut app = open_pulls(state_with(Vec::new())).await;
    assert!(
        app.screen().contains("No open pull requests."),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Tab);
    app.wait_for("No closed or merged pull requests.").await;
    assert!(!app.screen().contains("No open pull requests."));
}

#[tokio::test]
async fn a_404_shows_the_message() {
    let mut app = open_pulls(pulls_failing(FailureResponse::NotFound)).await;
    let screen = app.screen();
    assert!(screen.contains("Cannot load the pull requests"), "{screen}");
    assert!(!screen.contains("Loading"), "{screen}");
}

#[tokio::test]
async fn a_server_error_shows_the_message_and_r_retries() {
    let mut state = pulls_failing(FailureResponse::ServerError);
    state.failures[0].remaining = Some(1);
    let mut app = open_pulls(state).await;
    assert!(
        app.screen().contains("Cannot load the pull requests"),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Char('r'));
    app.wait_for("Fix").await;
    assert!(!app.screen().contains("Cannot load"));
}

#[tokio::test]
async fn leaving_before_the_answer_arrives_does_not_panic_nor_show_stale_rows() {
    let mut state = state_with(vec![pr(1, "Still open", "bob", PullState::Open, 3)]);
    state.latency = Duration::from_millis(150);
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app.press(KeyCode::Tab);
    app.press(KeyCode::Esc);
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("ghu · Repo list"), "{screen}");
    assert!(!screen.contains("Still open"), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn a_long_title_is_cut_on_one_row_and_the_rest_of_the_row_stays_visible() {
    let long = "x".repeat(300);
    let mut app = open_pulls(state_with(vec![pr(5, &long, "bob", PullState::Open, 3)])).await;
    let screen = app.screen();
    let rows = screen.lines().filter(|l| l.contains("xxxxxxxx")).count();
    assert_eq!(rows, 1, "title wrapped:\n{screen}");
    let line = line_of(&screen, "#5");
    assert!(
        line.contains("xxxxxxxx") && line.contains("@bob") && line.contains("2026-03-03"),
        "{line:?}"
    );
}

#[tokio::test]
async fn the_help_shows_tab() {
    let mut app = open_pulls(state_with(Vec::new())).await;
    app.press(KeyCode::Char('?'));
    let screen = app.screen();
    assert!(screen.contains("Tab"), "{screen}");
    assert!(screen.contains("Refresh"), "{screen}");
}

#[tokio::test]
async fn selection_is_bounded_and_enter_opens_the_detail() {
    let mut app = open_pulls(state_with(vec![
        pr(1, "Alpha", "bob", PullState::Open, 3),
        pr(2, "Beta", "bob", PullState::Open, 4),
    ]))
    .await;
    let selected = |screen: &str| {
        screen
            .lines()
            .find(|l| l.contains('>') && l.contains('#'))
            .unwrap_or_else(|| panic!("no selected row:\n{screen}"))
            .to_owned()
    };
    assert!(
        selected(&app.screen()).contains("Beta"),
        "newest first and selected"
    );
    app.press(KeyCode::Up);
    app.press(KeyCode::Up);
    assert!(selected(&app.screen()).contains("Beta"));
    app.press(KeyCode::Char('j'));
    app.press(KeyCode::Down);
    assert!(selected(&app.screen()).contains("Alpha"));
    app.press(KeyCode::Enter);
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1"), "{screen}");
    assert!(!screen.contains("Pull requests (open)"), "{screen}");
    app.press(KeyCode::Esc);
    assert!(
        app.screen().contains("Pull requests (open)"),
        "{}",
        app.screen()
    );
}
