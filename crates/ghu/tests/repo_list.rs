//! Tickets #2 and #3: Repo list, loading indicator, help, quit, errors, affiliations, archived repos, filter,
//! selection (through the harness only).

mod support;

use std::time::Duration;

use gh_fake::{Access, Failure, FailureResponse, Method, Repo, State, User};
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

fn failing(response: FailureResponse) -> State {
    let mut state = State::demo();
    state.failures.push(Failure {
        method: Method::Get,
        path: "/user/repos".into(),
        response,
        remaining: None,
    });
    state
}

#[tokio::test]
async fn an_empty_account_shows_no_repos() {
    let mut state = State::demo();
    state.repos.clear();
    let mut app = Harness::start(state).await;
    app.wait_for("No repos.").await;
    assert!(!app.screen().contains("Loading"), "{}", app.screen());
}

#[tokio::test]
async fn a_server_error_shows_a_message_and_not_the_token_message() {
    let mut app = Harness::start(failing(FailureResponse::ServerError)).await;
    app.wait_for("Cannot load the repos").await;
    let screen = app.screen();
    assert!(!screen.contains("rejected the token"), "{screen}");
    assert!(!screen.contains("Loading"), "{screen}");
    app.press(KeyCode::Char('q'));
    assert!(app.is_finished());
}

#[tokio::test]
async fn a_rate_limit_shows_a_message() {
    let mut app = Harness::start(failing(FailureResponse::RateLimited)).await;
    app.wait_for("Cannot load the repos").await;
    assert!(
        !app.screen().contains("rejected the token"),
        "{}",
        app.screen()
    );
}

#[tokio::test]
async fn every_page_of_repos_is_loaded() {
    let mut state = State::demo();
    state.max_per_page = 1;
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    for repo in ["alice/dotfiles", "alice/notes", "alice/secret-garden"] {
        assert!(screen.contains(repo), "{repo} missing:\n{screen}");
    }
}

// ---- #3: complete list, archived repos, filter, selection ----

/// A state for user `alice` holding exactly `repos`.
fn state_with(repos: Vec<Repo>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = repos;
    state
}

fn repo(owner: &str, name: &str, change: impl FnOnce(&mut Repo)) -> Repo {
    let mut repo = Repo::new(owner, name);
    change(&mut repo);
    repo
}

/// The header line (first row of the screen).
fn header(screen: &str) -> &str {
    screen.lines().next().unwrap_or_default()
}

#[tokio::test]
async fn collaborator_and_organization_repos_are_listed() {
    let state = state_with(vec![
        Repo::new("alice", "notes"),
        repo("bob", "shared-project", |r| r.access = Access::Collaborator),
        repo("acme", "widgets", |r| r.access = Access::OrganizationMember),
        repo("carol", "public-lib", |r| r.access = Access::NoAffiliation),
    ]);
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    for name in ["alice/notes", "bob/shared-project", "acme/widgets"] {
        assert!(screen.contains(name), "{name} missing:\n{screen}");
    }
    assert!(!screen.contains("carol/public-lib"), "{screen}");
}

#[tokio::test]
async fn every_page_of_every_affiliation_is_loaded() {
    let mut state = state_with(vec![
        Repo::new("alice", "notes"),
        repo("bob", "shared-project", |r| r.access = Access::Collaborator),
        repo("acme", "widgets", |r| r.access = Access::OrganizationMember),
    ]);
    state.max_per_page = 1;
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    for name in ["alice/notes", "bob/shared-project", "acme/widgets"] {
        assert!(screen.contains(name), "{name} missing:\n{screen}");
    }
    let pages = app
        .fake
        .requests()
        .iter()
        .filter(|r| r.path == "/user/repos")
        .count();
    assert_eq!(pages, 3);
}

#[tokio::test]
async fn archived_repos_are_hidden_until_a_shows_them() {
    let state = state_with(vec![
        Repo::new("alice", "notes"),
        repo("alice", "old-experiments", |r| r.archived = true),
        repo("acme", "legacy-site", |r| {
            r.archived = true;
            r.access = Access::OrganizationMember;
        }),
    ]);
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(!screen.contains("alice/old-experiments"), "{screen}");
    assert!(!screen.contains("acme/legacy-site"), "{screen}");
    assert!(!header(&screen).contains("archived shown"), "{screen}");

    app.press(KeyCode::Char('a'));
    let screen = app.screen();
    assert_eq!(header(&screen).trim(), "ghu · Repo list (archived shown)");
    assert!(
        line_of(&screen, "alice/old-experiments").contains("[archived]"),
        "{screen}"
    );
    assert!(
        line_of(&screen, "acme/legacy-site").contains("[archived]"),
        "{screen}"
    );
    assert!(!line_of(&screen, "alice/notes").contains("[archived]"));

    app.press(KeyCode::Char('a'));
    let screen = app.screen();
    assert!(!screen.contains("alice/old-experiments"), "{screen}");
    assert_eq!(header(&screen).trim(), "ghu · Repo list");
}

#[tokio::test]
async fn forks_and_private_repos_are_marked_in_order() {
    let state = state_with(vec![
        repo("alice", "dotfiles", |r| {
            r.fork = true;
            r.private = true;
            r.description = Some("My config".into());
        }),
        Repo::new("alice", "notes"),
    ]);
    let mut app = Harness::start(state).await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "alice/dotfiles").contains("alice/dotfiles [fork] [private]  My config"),
        "{screen}"
    );
    assert!(!line_of(&screen, "alice/notes").contains('['), "{screen}");
}

/// Repos for the filter tests: two match `note` whatever the case, one does not.
fn filter_state() -> State {
    state_with(vec![
        Repo::new("alice", "notes"),
        repo("bob", "NOTES-backup", |r| r.access = Access::Collaborator),
        repo("acme", "widgets", |r| r.access = Access::OrganizationMember),
    ])
}

#[tokio::test]
async fn the_filter_matches_part_of_the_name_case_insensitively() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("NoTe");
    let screen = app.screen();
    assert!(screen.contains("Filter: NoTe"), "{screen}");
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(screen.contains("bob/NOTES-backup"), "{screen}");
    assert!(!screen.contains("acme/widgets"), "{screen}");
}

#[tokio::test]
async fn the_filter_also_matches_the_owner() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("acme/");
    let screen = app.screen();
    assert!(screen.contains("acme/widgets"), "{screen}");
    assert!(!screen.contains("alice/notes"), "{screen}");
}

#[tokio::test]
async fn no_match_shows_a_message_and_backspace_deletes() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("widgetx");
    let screen = app.screen();
    assert!(screen.contains("No repo matches \"widgetx\"."), "{screen}");
    assert!(!screen.contains("acme/widgets"), "{screen}");

    app.press(KeyCode::Backspace);
    let screen = app.screen();
    assert!(screen.contains("Filter: widget"), "{screen}");
    assert!(screen.contains("acme/widgets"), "{screen}");
    assert!(!screen.contains("No repo matches"), "{screen}");
}

#[tokio::test]
async fn backspace_on_an_empty_filter_does_nothing() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.press(KeyCode::Backspace);
    app.type_text("wid");
    let screen = app.screen();
    assert!(screen.contains("Filter: wid"), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn enter_keeps_the_filter_and_gives_the_keys_back() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("note");
    app.press(KeyCode::Enter);
    app.press(KeyCode::Char('a'));
    let screen = app.screen();
    assert!(screen.contains("Filter: note"), "{screen}");
    assert!(!screen.contains("Filter: notea"), "{screen}");
    assert!(header(&screen).contains("archived shown"), "{screen}");
    assert!(!screen.contains("acme/widgets"), "{screen}");
    // Not editing any more: `q` quits.
    app.press(KeyCode::Char('q'));
    assert!(app.is_finished());
}

#[tokio::test]
async fn esc_while_typing_clears_the_filter() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("wid");
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(!screen.contains("Filter:"), "{screen}");
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(screen.contains("acme/widgets"), "{screen}");
    app.press(KeyCode::Char('q'));
    assert!(app.is_finished(), "Esc must stop the editing");
}

#[tokio::test]
async fn esc_clears_a_kept_filter_without_quitting() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("wid");
    app.press(KeyCode::Enter);
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(!screen.contains("Filter:"), "{screen}");
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn q_and_question_mark_typed_in_the_filter_are_text() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("q?");
    let screen = app.screen();
    assert!(!app.is_finished());
    assert!(screen.contains("Filter: q?"), "{screen}");
    assert!(!screen.contains("Toggle this help"), "{screen}");
}

#[tokio::test]
async fn a_paste_appends_to_the_filter() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Char('/'));
    app.type_text("a");
    app.paste("cme/wid");
    let screen = app.screen();
    assert!(screen.contains("Filter: acme/wid"), "{screen}");
    assert!(screen.contains("acme/widgets"), "{screen}");
    assert!(!screen.contains("alice/notes"), "{screen}");
}

#[tokio::test]
async fn a_paste_outside_the_filter_is_ignored() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.paste("qqq");
    assert!(!app.is_finished());
    let screen = app.screen();
    assert!(!screen.contains("Filter:"), "{screen}");
    assert!(screen.contains("acme/widgets"), "{screen}");
}

/// 40 repos `alice/repo-00` … `alice/repo-39`: more than the screen shows.
fn forty_repos() -> State {
    state_with(
        (0..40)
            .map(|i| Repo::new("alice", &format!("repo-{i:02}")))
            .collect(),
    )
}

#[tokio::test]
async fn the_selection_is_bounded_and_the_list_scrolls() {
    let mut app = Harness::start(forty_repos()).await;
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains(">alice/repo-00"), "{screen}");
    assert!(!screen.contains("alice/repo-39"), "{screen}");

    app.press(KeyCode::Up);
    assert!(app.screen().contains(">alice/repo-00"), "{}", app.screen());

    for _ in 0..39 {
        app.press(KeyCode::Char('j'));
    }
    let screen = app.screen();
    assert!(screen.contains(">alice/repo-39"), "{screen}");
    assert!(!screen.contains("alice/repo-00"), "{screen}");

    for _ in 0..3 {
        app.press(KeyCode::Down);
    }
    assert!(app.screen().contains(">alice/repo-39"), "{}", app.screen());

    for _ in 0..39 {
        app.press(KeyCode::Char('k'));
    }
    let screen = app.screen();
    assert!(screen.contains(">alice/repo-00"), "{screen}");
    assert_eq!(screen.matches('>').count(), 1, "{screen}");
}

#[tokio::test]
async fn the_selection_follows_the_repo_when_the_filter_changes() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    // Order by name: acme/widgets, alice/notes, bob/NOTES-backup.
    app.press(KeyCode::Down);
    app.press(KeyCode::Down);
    assert!(
        app.screen().contains(">bob/NOTES-backup"),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Char('/'));
    app.type_text("note");
    assert!(
        app.screen().contains(">bob/NOTES-backup"),
        "{}",
        app.screen()
    );
    app.type_text("s-");
    assert!(
        app.screen().contains(">bob/NOTES-backup"),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Esc);
    app.press(KeyCode::Char('/'));
    app.type_text("wid");
    assert!(app.screen().contains(">acme/widgets"), "{}", app.screen());
}

#[tokio::test]
async fn enter_opens_the_pull_requests_of_the_selected_repo() {
    let mut app = Harness::start(filter_state()).await;
    app.settle().await;
    app.press(KeyCode::Down);
    app.press(KeyCode::Enter);
    let screen = app.screen();
    assert_eq!(
        header(&screen).trim(),
        "ghu · alice/notes · Pull requests (open)"
    );
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert_eq!(header(&screen).trim(), "ghu · Repo list");
    assert!(screen.contains(">alice/notes"), "{screen}");
}

#[tokio::test]
async fn enter_on_an_empty_list_does_nothing() {
    let mut app = Harness::start(state_with(Vec::new())).await;
    app.wait_for("No repos.").await;
    app.press(KeyCode::Enter);
    assert_eq!(header(&app.screen()).trim(), "ghu · Repo list");
}

#[tokio::test]
async fn r_reloads_the_list() {
    let mut app = Harness::start(state_with(vec![Repo::new("alice", "notes")])).await;
    app.settle().await;
    app.fake
        .with_state(|s| s.repos.push(Repo::new("alice", "fresh")));
    assert!(!app.screen().contains("alice/fresh"));
    app.press(KeyCode::Char('r'));
    app.wait_for("alice/fresh").await;
    let loads = app
        .fake
        .requests()
        .iter()
        .filter(|r| r.path == "/user/repos")
        .count();
    assert_eq!(loads, 2);
}
