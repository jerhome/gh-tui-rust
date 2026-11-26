//! Ticket #4: Pinned repos — open `owner/name`, pin, unpin, persistence (through the harness only).

mod support;

use gh_fake::{Access, Method, Repo, State, User};
use ghu::KeyCode;
use support::{Harness, TestEnv};

/// The line of `screen` that contains `needle`.
fn line_of<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not on screen:\n{screen}"))
}

/// Index of the first line of `screen` containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not on screen:\n{screen}"))
}

fn header(screen: &str) -> &str {
    screen.lines().next().unwrap_or_default()
}

/// User `alice` owning `alice/notes` and `alice/zzz`; `carol/public-lib` is public and not affiliated; the
/// archived `acme/legacy-site` is an organization repo.
fn state() -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    let mut public_lib = Repo::new("carol", "public-lib");
    public_lib.access = Access::NoAffiliation;
    public_lib.description = Some("Someone else's library".into());
    let mut legacy = Repo::new("acme", "legacy-site");
    legacy.archived = true;
    legacy.access = Access::OrganizationMember;
    state.repos = vec![
        Repo::new("alice", "notes"),
        Repo::new("alice", "zzz"),
        public_lib,
        legacy,
    ];
    state
}

/// Writes `content` as the Pinned repos file of `env`'s configuration directory.
fn write_pins(env: &TestEnv, content: &str) {
    let dir = env.config_dir.as_deref().expect("harness config dir");
    let path = ghu::pinned_repos_path(dir);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, content).expect("write pins");
}

/// Starts with `content` as the Pinned repos file.
async fn start_with_pins(content: &'static str) -> Harness {
    Harness::start_with(state(), move |env, _| write_pins(env, content)).await
}

/// Number of `GET /repos/{owner}/{repo}` requests for `path`.
fn repo_requests(app: &Harness, path: &str) -> usize {
    app.fake
        .requests()
        .iter()
        .filter(|r| r.method == Method::Get && r.path == path)
        .count()
}

/// Opens `name` with `o` and waits until no request is in flight.
async fn open(app: &mut Harness, name: &str) {
    app.press(KeyCode::Char('o'));
    app.type_text(name);
    app.press(KeyCode::Enter);
    app.settle().await;
}

fn pinned_file_text(app: &Harness) -> String {
    std::fs::read_to_string(app.pinned_file()).expect("pinned file written")
}

#[tokio::test]
async fn o_opens_a_prompt() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    app.press(KeyCode::Char('o'));
    app.type_text("carol/pub");
    assert!(
        app.screen().contains("Open repo (owner/name): carol/pub"),
        "{}",
        app.screen()
    );
}

#[tokio::test]
async fn opening_owner_name_pins_it_and_saves_the_file() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "carol/public-lib").await;
    assert!(
        pinned_file_text(&app).contains("pinned = [\"carol/public-lib\"]"),
        "{}",
        pinned_file_text(&app)
    );
    // Back to the Repo list (from the PR list if it opened).
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert_eq!(header(&screen).trim(), "ghu · Repo list");
    assert!(!screen.contains("Open repo"), "{screen}");
    assert!(
        line_of(&screen, "carol/public-lib").contains("carol/public-lib [pinned]"),
        "{screen}"
    );
    assert!(
        row_of(&screen, "carol/public-lib") < row_of(&screen, "alice/notes"),
        "pinned repos come first:\n{screen}"
    );
}

#[tokio::test]
#[ignore = "needs a way to push a screen from View::apply (ui.rs, architecte)"]
async fn opening_owner_name_opens_its_pull_requests() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "carol/public-lib").await;
    assert_eq!(
        header(&app.screen()).trim(),
        "ghu · carol/public-lib · Pull requests (open)"
    );
}

#[tokio::test]
async fn a_pinned_repo_is_kept_after_a_restart() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "carol/public-lib").await;
    let before = repo_requests(&app, "/repos/carol/public-lib");

    app.restart();
    app.wait_for("carol/public-lib").await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "carol/public-lib").contains("[pinned]"),
        "{screen}"
    );
    assert!(screen.contains("Someone else's library"), "{screen}");
    assert_eq!(repo_requests(&app, "/repos/carol/public-lib"), before + 1);
}

#[tokio::test]
async fn the_canonical_case_of_github_is_saved() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "CAROL/Public-Lib").await;
    assert!(
        pinned_file_text(&app).contains("pinned = [\"carol/public-lib\"]"),
        "{}",
        pinned_file_text(&app)
    );
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(screen.contains("carol/public-lib [pinned]"), "{screen}");
    assert!(!screen.contains("CAROL"), "{screen}");
}

#[tokio::test]
async fn a_pinned_repo_that_is_also_affiliated_is_listed_once_among_the_pinned() {
    let mut app = start_with_pins("pinned = [\"alice/zzz\"]\n").await;
    app.settle().await;
    let screen = app.screen();
    assert_eq!(screen.matches("alice/zzz").count(), 1, "{screen}");
    assert!(
        line_of(&screen, "alice/zzz").contains("[pinned]"),
        "{screen}"
    );
    assert!(
        row_of(&screen, "alice/zzz") < row_of(&screen, "alice/notes"),
        "{screen}"
    );
}

#[tokio::test]
async fn opening_an_affiliated_repo_moves_it_to_the_pinned_group() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "alice/zzz").await;
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert_eq!(screen.matches("alice/zzz").count(), 1, "{screen}");
    assert!(
        row_of(&screen, "alice/zzz") < row_of(&screen, "alice/notes"),
        "{screen}"
    );
}

#[tokio::test]
async fn u_unpins_and_removes_from_the_list_and_the_file() {
    let mut app = start_with_pins("pinned = [\"carol/public-lib\", \"alice/notes\"]\n").await;
    app.settle().await;
    // First row: carol/public-lib.
    app.press(KeyCode::Char('u'));
    let screen = app.screen();
    assert!(screen.contains("Unpinned carol/public-lib."), "{screen}");
    assert!(!screen.contains("carol/public-lib ["), "{screen}");
    assert!(
        pinned_file_text(&app).contains("pinned = [\"alice/notes\"]"),
        "{}",
        pinned_file_text(&app)
    );

    // alice/notes is affiliated: it stays, without [pinned], in GitHub's order.
    let screen = app.screen();
    assert!(screen.contains(">alice/notes [pinned]"), "{screen}");
    app.press(KeyCode::Char('u'));
    let screen = app.screen();
    assert!(screen.contains("Unpinned alice/notes."), "{screen}");
    assert!(screen.contains("alice/notes"), "{screen}");
    assert!(!screen.contains("[pinned]"), "{screen}");
    assert!(
        row_of(&screen, "alice/notes") < row_of(&screen, "alice/zzz"),
        "{screen}"
    );
    assert!(
        pinned_file_text(&app).contains("pinned = []"),
        "{}",
        pinned_file_text(&app)
    );
}

#[tokio::test]
async fn u_on_a_repo_that_is_not_pinned_says_so() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    app.press(KeyCode::Char('u'));
    let screen = app.screen();
    assert!(screen.contains("alice/notes is not pinned."), "{screen}");
    assert!(!app.pinned_file().exists());
}

#[tokio::test]
async fn a_missing_repo_shows_a_message_keeps_the_input_and_pins_nothing() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    open(&mut app, "nobody/nothing").await;
    let screen = app.screen();
    assert!(
        screen.contains("Cannot open nobody/nothing: not found on GitHub (404)"),
        "{screen}"
    );
    assert!(
        screen.contains("Open repo (owner/name): nobody/nothing"),
        "{screen}"
    );
    assert!(!app.pinned_file().exists());
    // The prompt is still editing: Backspace edits it.
    app.press(KeyCode::Backspace);
    assert!(
        app.screen()
            .contains("Open repo (owner/name): nobody/nothin"),
        "{}",
        app.screen()
    );
}

#[tokio::test]
async fn a_malformed_name_is_refused_without_a_request() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    let before = app.fake.requests().len();
    app.press(KeyCode::Char('o'));
    app.type_text("not a repo");
    app.press(KeyCode::Enter);
    let screen = app.screen();
    assert!(
        screen.contains("Invalid repo name: type owner/name."),
        "{screen}"
    );
    assert!(
        screen.contains("Open repo (owner/name): not a repo"),
        "{screen}"
    );
    assert_eq!(app.fake.requests().len(), before);
}

#[tokio::test]
async fn esc_cancels_the_prompt() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    app.press(KeyCode::Char('o'));
    app.type_text("q?");
    assert!(!app.is_finished());
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(!screen.contains("Open repo"), "{screen}");
    assert!(!app.is_finished());
    assert!(!app.pinned_file().exists());
}

#[tokio::test]
async fn a_pasted_name_is_trimmed_to_its_first_line() {
    let mut app = Harness::start(state()).await;
    app.settle().await;
    app.press(KeyCode::Char('o'));
    app.paste("  carol/public-lib \nsecond line");
    assert!(
        app.screen()
            .contains("Open repo (owner/name): carol/public-lib"),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Enter);
    app.settle().await;
    assert!(
        pinned_file_text(&app).contains("\"carol/public-lib\""),
        "{}",
        pinned_file_text(&app)
    );
}

#[tokio::test]
async fn an_invalid_file_at_start_up_shows_a_notice_and_refuses_pinning() {
    let mut app = start_with_pins("pinned = [\n").await;
    let screen = app.screen();
    assert!(screen.contains("is invalid"), "{screen}");
    app.settle().await;
    open(&mut app, "carol/public-lib").await;
    let screen = app.screen();
    assert!(
        screen.contains("Pinning is disabled"),
        "the refusal must be shown:\n{screen}"
    );
    assert_eq!(pinned_file_text(&app), "pinned = [\n");
    app.press(KeyCode::Esc);
    assert!(!app.screen().contains("[pinned]"), "{}", app.screen());
}

#[tokio::test]
async fn without_a_config_directory_pins_live_in_memory() {
    let mut app = Harness::start_with(state(), |env, _| env.config_dir = None).await;
    app.settle().await;
    open(&mut app, "carol/public-lib").await;
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(screen.contains("carol/public-lib [pinned]"), "{screen}");
    assert!(!app.pinned_file().exists());
    assert_eq!(std::fs::read_dir(app.config_dir()).expect("dir").count(), 0);
}

#[tokio::test]
async fn a_pinned_repo_missing_at_start_up_is_marked_and_can_be_unpinned() {
    let mut app = start_with_pins("pinned = [\"ghost/gone\"]\n").await;
    app.settle().await;
    let screen = app.screen();
    let line = line_of(&screen, "ghost/gone");
    assert!(line.contains("ghost/gone [pinned]"), "{screen}");
    assert!(line.contains("not found"), "{screen}");

    app.press(KeyCode::Enter);
    let screen = app.screen();
    assert!(
        screen.contains("Cannot open ghost/gone: not found on GitHub"),
        "{screen}"
    );
    assert_eq!(header(&screen).trim(), "ghu · Repo list");

    app.press(KeyCode::Char('u'));
    let screen = app.screen();
    assert!(screen.contains("Unpinned ghost/gone."), "{screen}");
    assert!(!screen.contains("ghost/gone ["), "{screen}");
    assert!(pinned_file_text(&app).contains("pinned = []"));
}

#[tokio::test]
async fn a_pinned_archived_repo_is_shown() {
    let mut app = start_with_pins("pinned = [\"acme/legacy-site\"]\n").await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "acme/legacy-site").contains("acme/legacy-site [pinned] [archived]"),
        "{screen}"
    );
    assert_eq!(header(&screen).trim(), "ghu · Repo list");
}

#[tokio::test]
async fn r_reloads_the_pinned_repos_too() {
    let mut app = start_with_pins("pinned = [\"carol/public-lib\"]\n").await;
    app.settle().await;
    app.press(KeyCode::Char('r'));
    app.settle().await;
    assert_eq!(repo_requests(&app, "/repos/carol/public-lib"), 2);
}
