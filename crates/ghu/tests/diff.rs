//! Ticket #7: the Diff of one PR (through the harness only, against the fake GitHub). The signed-in user is
//! `alice`; the PR lives in `alice/notes`. Colours are checked by the unit tests of `ui/diff.rs`: the harness
//! gives the screen as text, not the styles of the cells.

mod support;

use gh_fake::{Failure, FailureResponse, FileStatus, Method, Pull, PullFile, Repo, State, User};
use ghu::KeyCode;
use support::Harness;

/// A file of the fake whose counters are given, not computed.
fn file(
    path: &str,
    status: FileStatus,
    additions: u64,
    deletions: u64,
    patch: Option<&str>,
) -> PullFile {
    PullFile {
        filename: path.to_owned(),
        previous_filename: None,
        status,
        additions,
        deletions,
        patch: patch.map(str::to_owned),
    }
}

/// A modified file with a patch of added lines `+<prefix> <n>` (n from 1 to `count`, three digits).
fn long_file(path: &str, prefix: &str, count: usize) -> PullFile {
    let patch = (1..=count)
        .map(|n| format!("+{prefix} {n:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    PullFile::modified(path, &patch)
}

/// A state with the single repo `alice/notes` and its PR #1 changing `files`.
fn state_with(files: Vec<PullFile>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = vec![Repo::new("alice", "notes")];
    let mut pull = Pull::new("alice", "notes", 1, "One", "bob");
    pull.files = files;
    state.pulls = vec![pull];
    state
}

/// Opens the PR list and the detail of PR #1.
async fn open_detail(state: State) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    app.settle().await;
    app
}

/// Opens the detail of PR #1 and its Diff (`d`); does not wait for the files.
async fn open_diff(state: State) -> Harness {
    let mut app = open_detail(state).await;
    app.press(KeyCode::Char('d'));
    app
}

/// The part of the first row containing `needle` that is right of the file list.
fn right_of_list<'a>(screen: &'a str, needle: &str) -> &'a str {
    line_of(screen, needle)
        .split("││")
        .nth(1)
        .unwrap_or_else(|| panic!("no right pane on the row of {needle:?}:\n{screen}"))
}

/// The first line of `screen` containing `needle`.
fn line_of<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
}

/// The screen without spaces: the harness reads the cells of a wide character as the character and a space.
fn compact(screen: &str) -> String {
    screen.chars().filter(|c| *c != ' ').collect()
}

fn press_times(app: &mut Harness, code: KeyCode, times: usize) {
    for _ in 0..times {
        app.press(code);
    }
}

#[tokio::test]
async fn the_list_shows_status_letter_counters_and_the_selected_file() {
    let state = state_with(vec![
        file(
            "src/lib.rs",
            FileStatus::Modified,
            2,
            1,
            Some("@@ -1,2 +1,3 @@\n keep\n-old\n+new\n+newer"),
        ),
        file(
            "docs/new.md",
            FileStatus::Added,
            1,
            0,
            Some("@@ -0,0 +1 @@\n+hello"),
        ),
        file(
            "old.txt",
            FileStatus::Removed,
            0,
            3,
            Some("@@ -1,3 +0,0 @@\n-a\n-b\n-c"),
        ),
        file("mode.sh", FileStatus::Changed, 0, 0, None),
        file("same.txt", FileStatus::Unchanged, 0, 0, None),
    ]);
    let mut app = open_diff(state).await;
    app.wait_for("src/lib.rs").await;
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1 · Diff"), "{screen}");
    assert!(screen.contains("> M +2 -1 src/lib.rs"), "{screen}");
    assert!(screen.contains("  A +1 -0 docs/new.md"), "{screen}");
    assert!(screen.contains("  D +0 -3 old.txt"), "{screen}");
    assert!(screen.contains("  M +0 -0 mode.sh"), "{screen}");
    assert!(screen.contains("    +0 -0 same.txt"), "{screen}");
    // The patch of the selected file is on the right, the others' are not.
    assert!(screen.contains("@@ -1,2 +1,3 @@"), "{screen}");
    assert!(screen.contains("+newer"), "{screen}");
    assert!(!screen.contains("+hello"), "{screen}");
    assert!(!screen.contains("Not available yet"), "{screen}");
    assert!(!screen.contains("Loading"), "{screen}");
}

#[tokio::test]
async fn n_and_p_move_the_selection_and_stop_at_both_ends() {
    let state = state_with(vec![
        file("a.rs", FileStatus::Modified, 1, 0, Some("+in a")),
        file("b.rs", FileStatus::Modified, 1, 0, Some("+in b")),
        file("c.rs", FileStatus::Modified, 1, 0, Some("+in c")),
    ]);
    let mut app = open_diff(state).await;
    app.wait_for("+in a").await;
    app.press(KeyCode::Char('p'));
    let screen = app.screen();
    assert!(
        screen.contains("> M +1 -0 a.rs"),
        "p before the first: {screen}"
    );
    app.press(KeyCode::Char('n'));
    let screen = app.screen();
    assert!(screen.contains("> M +1 -0 b.rs"), "{screen}");
    assert!(
        screen.contains("+in b") && !screen.contains("+in a"),
        "{screen}"
    );
    press_times(&mut app, KeyCode::Char('n'), 5);
    let screen = app.screen();
    assert!(
        screen.contains("> M +1 -0 c.rs"),
        "n after the last: {screen}"
    );
    assert!(screen.contains("+in c"), "{screen}");
    press_times(&mut app, KeyCode::Char('p'), 5);
    let screen = app.screen();
    assert!(
        screen.contains("> M +1 -0 a.rs") && screen.contains("+in a"),
        "{screen}"
    );
}

#[tokio::test]
async fn changing_file_resets_the_scroll() {
    let state = state_with(vec![
        long_file("a.rs", "row", 100),
        long_file("b.rs", "other", 100),
    ]);
    let mut app = open_diff(state).await;
    app.wait_for("+row 001").await;
    press_times(&mut app, KeyCode::Char('j'), 10);
    assert!(!app.screen().contains("+row 001"));
    app.press(KeyCode::Char('n'));
    let screen = app.screen();
    assert!(screen.contains("+other 001"), "{screen}");
    app.press(KeyCode::Char('p'));
    let screen = app.screen();
    assert!(screen.contains("+row 001"), "{screen}");
}

#[tokio::test]
async fn vertical_scroll_by_line_and_page_is_bounded() {
    let mut app = open_diff(state_with(vec![long_file("a.rs", "row", 100)])).await;
    app.wait_for("+row 001").await;
    let screen = app.screen();
    assert!(!screen.contains("+row 100"), "{screen}");
    app.press(KeyCode::Char('k'));
    assert!(app.screen().contains("+row 001"), "k at the top");
    app.press(KeyCode::Char('j'));
    let screen = app.screen();
    assert!(
        !screen.contains("+row 001") && screen.contains("+row 002"),
        "{screen}"
    );
    app.press(KeyCode::Down);
    assert!(app.screen().contains("+row 003") && !app.screen().contains("+row 002"));
    app.press(KeyCode::Up);
    assert!(app.screen().contains("+row 002") && !app.screen().contains("+row 001"));
    app.press(KeyCode::PageDown);
    let screen = app.screen();
    assert!(
        !screen.contains("+row 002") && screen.contains("+row 026"),
        "{screen}"
    );
    // To the end: the last row is shown, with the screen full of rows, and one more key changes nothing.
    press_times(&mut app, KeyCode::PageDown, 10);
    let end = app.screen();
    assert!(
        end.contains("+row 100") && end.contains("+row 080"),
        "{end}"
    );
    app.press(KeyCode::Char('j'));
    assert_eq!(app.screen(), end, "j at the end");
    app.press(KeyCode::PageUp);
    let screen = app.screen();
    assert!(
        !screen.contains("+row 100") && screen.contains("+row 076"),
        "{screen}"
    );
    press_times(&mut app, KeyCode::PageUp, 10);
    assert!(app.screen().contains("+row 001"));
}

/// A patch of one long added line: `+00-01-…-29-` (91 columns).
fn wide_line() -> String {
    let mut line = String::from("+");
    for n in 0..30 {
        line.push_str(&format!("{n:02}-"));
    }
    line
}

#[tokio::test]
async fn horizontal_scroll_moves_by_8_columns_and_is_bounded() {
    let patch = format!("@@ -1 +1 @@\n{}", wide_line());
    let state = state_with(vec![file("a.rs", FileStatus::Modified, 1, 0, Some(&patch))]);
    let mut app = open_diff(state).await;
    app.wait_for("+00-01-02-").await;
    app.press(KeyCode::Char('h'));
    assert!(app.screen().contains("+00-01-02-"), "h at the left end");
    app.press(KeyCode::Char('l'));
    let screen = app.screen();
    // Column 8 is the '2' of "02-" (the first column is the '+').
    assert!(
        right_of_list(&screen, "2-03-04-").starts_with("2-03-04-05-"),
        "{screen}"
    );
    app.press(KeyCode::Char('h'));
    assert!(app.screen().contains("+00-01-02-"));
    app.press(KeyCode::Right);
    app.press(KeyCode::Right);
    let screen = app.screen();
    assert!(
        right_of_list(&screen, "05-06-07-").starts_with("05-06-07-08-"),
        "{screen}"
    );
    app.press(KeyCode::Left);
    let screen = app.screen();
    assert!(
        right_of_list(&screen, "2-03-04-").starts_with("2-03-04-05-"),
        "{screen}"
    );
    press_times(&mut app, KeyCode::Char('l'), 50);
    let screen = app.screen();
    assert!(screen.contains("27-28-29-"), "end of the line: {screen}");
    // One step back from the end moves by 8 columns: the end is gone (the bound was the end, not beyond).
    app.press(KeyCode::Char('h'));
    let screen = app.screen();
    assert!(
        screen.contains("25-26-") && !screen.contains("29-"),
        "{screen}"
    );
}

#[tokio::test]
async fn a_rename_without_change_is_said_so() {
    let mut renamed = file("new/name.rs", FileStatus::Renamed, 0, 0, None);
    renamed.previous_filename = Some("old/name.rs".to_owned());
    let mut app = open_diff(state_with(vec![renamed])).await;
    app.wait_for("Renamed from old/name.rs, no content change.")
        .await;
    let screen = app.screen();
    assert!(screen.contains("old/name.rs → new/name.rs"), "{screen}");
    assert!(
        screen.contains("  R +0 -0 new/name.rs") || screen.contains("> R +0 -0 new/name.rs"),
        "{screen}"
    );
    assert!(!screen.contains("No diff from GitHub"), "{screen}");
}

#[tokio::test]
async fn a_file_without_patch_is_signalled_not_shown_empty() {
    let mut renamed_with_changes = file("b.rs", FileStatus::Renamed, 5, 2, None);
    renamed_with_changes.previous_filename = Some("a.rs".to_owned());
    let state = state_with(vec![
        file("logo.png", FileStatus::Added, 0, 0, None),
        renamed_with_changes,
        file("huge.rs", FileStatus::Modified, 9000, 10, None),
    ]);
    let mut app = open_diff(state).await;
    app.wait_for("No diff from GitHub: binary file, or too large to show.")
        .await;
    let screen = app.screen();
    assert!(screen.contains("> A +0 -0 logo.png"), "{screen}");
    app.press(KeyCode::Char('n'));
    let screen = app.screen();
    assert!(
        screen.contains("No diff from GitHub"),
        "renamed with changes: {screen}"
    );
    assert!(screen.contains("a.rs → b.rs"), "{screen}");
    assert!(!screen.contains("no content change"), "{screen}");
    app.press(KeyCode::Char('n'));
    let screen = app.screen();
    assert!(
        screen.contains("No diff from GitHub") && screen.contains("huge.rs"),
        "{screen}"
    );
}

#[tokio::test]
async fn a_pr_without_files_says_so() {
    let mut app = open_diff(state_with(Vec::new())).await;
    app.wait_for("No changed files.").await;
    // Keys on an empty list are harmless.
    for key in ['n', 'p', 'j', 'k', 'h', 'l'] {
        app.press(KeyCode::Char(key));
    }
    app.press(KeyCode::PageDown);
    app.press(KeyCode::Right);
    let screen = app.screen();
    assert!(screen.contains("No changed files."), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn more_than_a_page_of_files_are_all_listed() {
    let files = (1..=120)
        .map(|n| {
            file(
                &format!("f{n:03}.txt"),
                FileStatus::Modified,
                1,
                0,
                Some(&format!("+body {n:03}")),
            )
        })
        .collect();
    let mut app = open_diff(state_with(files)).await;
    app.fake.with_state(|s| s.max_per_page = 40);
    app.wait_for("+body 001").await;
    app.settle().await;
    app.fake.with_state(|s| s.requests.clear());
    app.press(KeyCode::Char('r'));
    app.wait_for("+body 001").await;
    app.settle().await;
    let pages = app
        .fake
        .requests()
        .iter()
        .filter(|r| r.path == "/repos/alice/notes/pulls/1/files")
        .count();
    assert_eq!(pages, 3, "120 files at 40 per page");
    press_times(&mut app, KeyCode::Char('n'), 119);
    let screen = app.screen();
    assert!(screen.contains("> M +1 -0 f120.txt"), "{screen}");
    assert!(screen.contains("+body 120"), "{screen}");
    app.press(KeyCode::Char('n'));
    assert!(app.screen().contains("> M +1 -0 f120.txt"));
}

fn failing(path: &str, response: FailureResponse, remaining: Option<u32>) -> Failure {
    Failure {
        method: Method::Get,
        path: path.into(),
        response,
        remaining,
    }
}

#[tokio::test]
async fn an_error_shows_a_readable_message_and_r_retries() {
    let mut state = state_with(vec![file(
        "a.rs",
        FileStatus::Modified,
        1,
        0,
        Some("+fine"),
    )]);
    state.failures.push(failing(
        "/repos/alice/notes/pulls/1/files",
        FailureResponse::NotFound,
        Some(1),
    ));
    let mut app = open_diff(state).await;
    app.wait_for("Cannot load the changed files").await;
    let screen = app.screen();
    assert!(screen.contains("not found on GitHub (404)"), "{screen}");
    assert!(
        !screen.contains("Loading") && !screen.contains("+fine"),
        "{screen}"
    );
    // Keys on a failed screen are harmless.
    for key in ['n', 'p', 'j', 'l'] {
        app.press(KeyCode::Char(key));
    }
    app.press(KeyCode::Char('r'));
    app.wait_for("+fine").await;
    assert!(!app.screen().contains("Cannot load"), "{}", app.screen());
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(
        screen.contains("Description"),
        "Esc goes back to the detail: {screen}"
    );
}

#[tokio::test]
async fn a_hostile_patch_cannot_reach_the_terminal() {
    let patch = "@@ -1 +1 @@\n+red \u{1b}[31mtext\n+flip \u{202e}live\n+lone a\rb\n+bell \u{7}ring\n+tab\there";
    let state = state_with(vec![file(
        "evil\u{1b}[2J.rs",
        FileStatus::Modified,
        4,
        0,
        Some(patch),
    )]);
    let mut app = open_diff(state).await;
    app.wait_for("+red").await;
    let screen = app.screen();
    for bad in ['\u{1b}', '\u{202e}', '\r', '\u{7}', '\t'] {
        assert!(
            !screen.contains(bad),
            "{bad:?} reached the buffer:\n{screen}"
        );
    }
    assert!(screen.contains("+red \u{fffd}[31mtext"), "{screen}");
    assert!(screen.contains("+flip \u{fffd}live"), "{screen}");
    assert!(screen.contains("+lone a\u{fffd}b"), "{screen}");
    assert!(screen.contains("+tab    here"), "{screen}");
    // The path is made visible in the list (left of the right pane) and in the header.
    let left = line_of(&screen, "> M +4 -0")
        .split("││")
        .next()
        .expect("left pane");
    assert!(left.contains("evil\u{fffd}[2J.rs"), "list: {screen}");
    assert!(
        right_of_list(&screen, "evil").starts_with("evil\u{fffd}[2J.rs"),
        "header: {screen}"
    );
}

#[tokio::test]
async fn a_hostile_previous_name_is_made_visible() {
    let mut renamed = file("new.rs", FileStatus::Renamed, 0, 0, None);
    renamed.previous_filename = Some("old\u{202e}name\u{1b}.rs".to_owned());
    let mut app = open_diff(state_with(vec![renamed])).await;
    app.wait_for("no content change").await;
    let screen = app.screen();
    for bad in ['\u{1b}', '\u{202e}'] {
        assert!(!screen.contains(bad), "{bad:?}: {screen}");
    }
    assert!(
        screen.contains("old\u{fffd}name\u{fffd}.rs → new.rs"),
        "header: {screen}"
    );
    assert!(
        screen.contains("Renamed from old\u{fffd}name\u{fffd}.rs"),
        "message: {screen}"
    );
}

#[tokio::test]
async fn a_slow_answer_to_an_older_request_does_not_overwrite_a_newer_one() {
    let mut app = open_detail(state_with(vec![file(
        "a.rs",
        FileStatus::Modified,
        1,
        0,
        Some("+first content"),
    )]))
    .await;
    // The first request is slow (the fake reads its data after the latency); the user reloads at once and the
    // second request, fast, reads the data as they are now. Then the data change: the late answer of the first
    // request carries the new data, and must be ignored because the screen waits for the second one.
    app.fake
        .with_state(|s| s.latency = std::time::Duration::from_millis(500));
    app.press(KeyCode::Char('d'));
    // Let the first request reach the fake (the runtime is single-threaded) before the latency goes back to 0.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    app.fake
        .with_state(|s| s.latency = std::time::Duration::ZERO);
    app.press(KeyCode::Char('r'));
    app.wait_for("+first content").await;
    app.fake.with_state(|s| {
        s.pull_mut("alice", "notes", 1).expect("pr").files = vec![file(
            "a.rs",
            FileStatus::Modified,
            1,
            0,
            Some("+second content"),
        )];
    });
    app.settle().await;
    let screen = app.screen();
    assert!(
        screen.contains("+first content") && !screen.contains("+second content"),
        "{screen}"
    );
}

#[tokio::test]
async fn a_crlf_patch_shows_two_lines_and_no_replacement_character() {
    let state = state_with(vec![file(
        "crlf.txt",
        FileStatus::Modified,
        2,
        0,
        Some("@@ -0,0 +1,2 @@\r\n+first\r\n+second\r\n"),
    )]);
    let mut app = open_diff(state).await;
    app.wait_for("+second").await;
    let screen = app.screen();
    assert!(!screen.contains('\u{fffd}'), "{screen}");
    let first = screen
        .lines()
        .position(|l| l.contains("+first"))
        .expect("first");
    let second = screen
        .lines()
        .position(|l| l.contains("+second"))
        .expect("second");
    assert_eq!(second, first + 1, "{screen}");
}

#[tokio::test]
async fn widths_are_measured_in_columns_for_cjk_and_emoji() {
    // The list pane is 33 columns wide (31 inside): a long CJK path must be cut with a visible mark.
    let long_path = "ドキュメント/日本語のファイル名がとても長い場合.md";
    // A line of 40 wide characters (80 columns) after the '+': 65 columns are shown at a time.
    let wide: String =
        "一二三四五六七八九十百千万億兆京垓秭穣溝澗正載極恒阿僧祇那由他不可思議無量大数上下"
            .chars()
            .take(40)
            .collect();
    let last: String = wide.chars().last().into_iter().collect();
    assert_eq!(wide.chars().count(), 40);
    let patch = format!("+{wide}\n+😀😀😀 smile");
    let state = state_with(vec![
        file(long_path, FileStatus::Modified, 2, 0, Some(&patch)),
        file("next.rs", FileStatus::Modified, 0, 0, Some("+x")),
    ]);
    let mut app = open_diff(state).await;
    app.wait_until("the wide line", |s| compact(s).contains("一二三"))
        .await;
    let screen = compact(&app.screen());
    let row = line_of(&screen, ">M+2-0");
    let row = row.split("││").next().expect("left pane");
    assert!(row.contains('…'), "the cut path has its mark: {row}");
    assert!(
        !row.contains("とても長い場合.md") && !row.contains("ファイル名"),
        "{row}"
    );
    assert!(screen.contains("😀😀😀smile"), "{screen}");
    assert!(
        !screen.contains(&last),
        "the 40th character does not fit at first: {screen}"
    );
    // Scrolling right reaches it: the bound is the width in columns, not in characters.
    press_times(&mut app, KeyCode::Char('l'), 10);
    let screen = app.screen();
    assert!(
        screen.contains(&last),
        "the end of the line is unreachable: {screen}"
    );
    // The rest of the screen is intact after the wide cells.
    assert!(screen.contains("next.rs"), "{screen}");
}

#[tokio::test]
async fn the_help_lists_the_diff_keys() {
    let mut app = open_diff(state_with(vec![file(
        "a.rs",
        FileStatus::Modified,
        1,
        0,
        Some("+x"),
    )]))
    .await;
    app.wait_for("+x").await;
    app.press(KeyCode::Char('?'));
    let screen = app.screen();
    for text in ["Next file", "Previous file", "Reload"] {
        assert!(screen.contains(text), "{text}: {screen}");
    }
}
