//! Ticket #6: the detail of one PR (through the harness only, against the fake GitHub). The signed-in user of
//! every state is `alice`; the PRs live in `alice/notes`, the only repo, so `Enter` opens it.

mod support;

use std::time::Duration;

use gh_fake::{
    Failure, FailureResponse, IssueComment, Method, Pull, PullFile, PullReview, PullState, Repo,
    ReviewState, State, User,
};
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

/// The demo data restricted to `alice/notes` (PR #1 has the Markdown description, the reviews, the comments).
fn demo_notes() -> State {
    let mut state = State::demo();
    state
        .repos
        .retain(|r| r.owner == "alice" && r.name == "notes");
    state
        .pulls
        .retain(|p| p.owner == "alice" && p.repo == "notes");
    state
}

/// An open PR of `alice/notes`.
fn pr(number: u64, title: &str, author: &str) -> Pull {
    let mut pull = Pull::new("alice", "notes", number, title, author);
    pull.updated_at = format!("2026-03-{:02}T12:30:00Z", number.min(28));
    pull
}

fn comment(id: u64, author: &str, body: &str, created_at: &str) -> IssueComment {
    IssueComment {
        id,
        author: Some(author.to_owned()),
        body: body.to_owned(),
        created_at: created_at.to_owned(),
    }
}

fn review(id: u64, author: &str, review_state: ReviewState, body: &str, at: &str) -> PullReview {
    PullReview {
        id,
        author: Some(author.to_owned()),
        state: review_state,
        body: body.to_owned(),
        submitted_at: (review_state != ReviewState::Pending).then(|| at.to_owned()),
        commit_id: format!("{:040x}", 1),
    }
}

/// Starts the app and opens the PR list of `alice/notes`.
async fn open_list(state: State) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    app
}

/// Selects the row of PR `number` in the list and presses `Enter`; does not wait for the answer.
async fn press_enter_on(app: &mut Harness, number: u64) {
    for _ in 0..20 {
        let screen = app.screen();
        let selected = screen
            .lines()
            .find(|l| l.contains('>') && l.contains('#'))
            .unwrap_or_else(|| panic!("no selected row:\n{screen}"));
        if selected.contains(&format!("#{number} ")) {
            break;
        }
        app.press(KeyCode::Char('j'));
    }
    app.press(KeyCode::Enter);
}

/// Opens the detail of PR `number` and waits until it is loaded.
async fn open_detail(state: State, number: u64) -> Harness {
    let mut app = open_list(state).await;
    press_enter_on(&mut app, number).await;
    app.wait_for("Description").await;
    app.settle().await;
    app
}

/// The first line of `screen` containing `needle`.
fn line_of<'a>(screen: &'a str, needle: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
}

/// Index of the first screen row containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
}

fn press_times(app: &mut Harness, code: KeyCode, times: usize) {
    for _ in 0..times {
        app.press(code);
    }
}

fn failing(path: &str, response: FailureResponse, remaining: Option<u32>) -> Failure {
    Failure {
        method: Method::Get,
        path: path.into(),
        response,
        remaining,
    }
}

/// A PR whose description has `count` numbered lines (`line 001`…), to scroll through.
fn long_pr(count: usize) -> Pull {
    let mut pull = pr(1, "Long one", "bob");
    pull.body = Some(
        (1..=count)
            .map(|i| format!("line {i:03}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    pull
}

/// The number of the first `line NNN` row on screen.
fn first_numbered_line(screen: &str) -> usize {
    let row = screen
        .lines()
        .find_map(|l| l.find("line ").map(|at| &l[at + 5..at + 8]))
        .unwrap_or_else(|| panic!("no numbered line:\n{screen}"));
    row.parse().unwrap_or_else(|_| panic!("{row:?}:\n{screen}"))
}

#[tokio::test]
async fn the_header_and_the_summary_show_title_author_branches_state_and_counts() {
    let mut pull = pr(7, "Fix typo in the README", "bob");
    pull.head = "fix-typo".into();
    pull.base = "develop".into();
    pull.created_at = "2026-02-03T09:00:00Z".into();
    pull.updated_at = "2026-03-04T10:00:00Z".into();
    pull.files = vec![
        PullFile::modified("a.rs", "@@ -1 +1,3 @@\n-x\n+a\n+b\n+c\n"),
        PullFile::modified("b.rs", "@@ -1 +1 @@\n x\n"),
    ];
    let mut app = open_detail(state_with(vec![pull]), 7).await;
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #7"), "{screen}");
    assert!(screen.contains("#7 Fix typo in the README"), "{screen}");
    assert!(
        screen.contains("open · @bob · alice:fix-typo → develop · mergeable"),
        "{screen}"
    );
    assert!(
        screen.contains("Created 2026-02-03 · updated 2026-03-04 · 2 files · +3 -1"),
        "{screen}"
    );
}

#[tokio::test]
async fn a_draft_pr_says_draft_and_closed_and_merged_say_so_without_mergeability() {
    let mut draft = pr(1, "Draft one", "bob");
    draft.draft = true;
    let mut closed = pr(2, "Closed one", "bob");
    closed.state = PullState::Closed;
    let mut merged = pr(3, "Merged one", "bob");
    merged.state = PullState::Merged;
    let state = state_with(vec![draft, closed, merged]);

    let mut app = open_detail(state.clone(), 1).await;
    let screen = app.screen();
    assert!(
        line_of(&screen, "@bob").starts_with("│draft · @bob · alice:feature → main · mergeable"),
        "{screen}"
    );

    // The closed / merged PRs are on the other tab of the list.
    for (number, word) in [(2, "closed"), (3, "merged")] {
        let mut app = open_list(state.clone()).await;
        app.press(KeyCode::Tab);
        app.wait_for(&format!("{word} one").replace("c", "C").replace("m", "M"))
            .await;
        press_enter_on(&mut app, number).await;
        app.wait_for("Description").await;
        let screen = app.screen();
        let line = line_of(&screen, "@bob");
        assert!(
            line.contains(&format!("{word} · @bob · alice:feature → main")),
            "{line}"
        );
        assert!(
            !line.contains("mergeable") && !line.contains("conflicts"),
            "{line}"
        );
    }
}

#[tokio::test]
async fn the_three_mergeability_states_are_told_apart() {
    for (mergeable, words) in [
        (Some(true), "→ main · mergeable"),
        (Some(false), "→ main · conflicts"),
        (None, "→ main · mergeability unknown"),
    ] {
        let mut pull = pr(1, "One", "bob");
        pull.mergeable = mergeable;
        let mut app = open_detail(state_with(vec![pull]), 1).await;
        let screen = app.screen();
        assert!(screen.contains(words), "{mergeable:?}:\n{screen}");
    }
}

#[tokio::test]
async fn a_deleted_author_is_ghost() {
    let mut pull = pr(1, "One", "bob");
    pull.author = None;
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    assert!(app.screen().contains("open · @ghost ·"), "{}", app.screen());
}

#[tokio::test]
async fn the_description_is_rendered_as_simple_markdown() {
    let mut app = open_detail(demo_notes(), 1).await;
    let screen = app.screen();
    assert!(!screen.contains("# Fix typo"), "{screen}");
    assert!(screen.contains("│Fix typo"), "{screen}");
    assert!(
        screen.contains("The README said notess. Changes:"),
        "{screen}"
    );
    assert!(!screen.contains("**"), "{screen}");
    assert!(screen.contains("• fix the typo"), "{screen}");
    assert!(screen.contains("• drop the export script"), "{screen}");
    assert!(!screen.contains("```"), "{screen}");
    assert!(screen.contains("│  grep -n notess README.md"), "{screen}");
    // Images and HTML stay as text.
    assert!(
        screen.contains("![screenshot](https://example.com/shot.png)"),
        "{screen}"
    );
    assert!(
        screen.contains("<b>Raw HTML</b> stays as text."),
        "{screen}"
    );
}

#[tokio::test]
async fn a_missing_description_says_so() {
    for body in [None, Some(String::new())] {
        let mut pull = pr(1, "One", "bob");
        pull.body = body;
        let mut app = open_detail(state_with(vec![pull]), 1).await;
        assert!(app.screen().contains("No description."), "{}", app.screen());
    }
}

#[tokio::test]
async fn verdicts_are_listed_per_reviewer_and_comments_and_pending_reviews_are_not_verdicts() {
    let mut app = open_detail(demo_notes(), 1).await;
    let screen = app.screen();
    // carol approved, dave requested changes, erin only commented, alice has a pending review.
    assert!(
        screen.contains("Verdicts: @carol Approve · @dave Request changes"),
        "{screen}"
    );
    assert!(!screen.contains("@erin Approve"), "{screen}");
    assert!(!screen.contains("Draft of my review"), "{screen}");
}

#[tokio::test]
async fn a_pr_without_reviews_has_no_verdicts() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    assert!(app.screen().contains("Verdicts: none"), "{}", app.screen());
}

#[tokio::test]
async fn a_dismissed_verdict_is_not_a_verdict_but_its_text_stays_in_the_conversation() {
    let mut pull = pr(1, "One", "bob");
    pull.reviews = vec![
        review(
            1,
            "carol",
            ReviewState::Approved,
            "",
            "2026-03-01T10:00:00Z",
        ),
        review(
            2,
            "carol",
            ReviewState::Dismissed,
            "Old approval",
            "2026-03-02T10:00:00Z",
        ),
        review(
            3,
            "dave",
            ReviewState::ChangesRequested,
            "",
            "2026-03-03T10:00:00Z",
        ),
        review(
            4,
            "dave",
            ReviewState::Dismissed,
            "",
            "2026-03-04T10:00:00Z",
        ),
    ];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    assert!(screen.contains("Verdicts: none"), "{screen}");
    assert!(screen.contains("Conversation (1)"), "{screen}");
    assert!(
        screen.contains("@carol · 2026-03-02 · dismissed"),
        "{screen}"
    );
    assert!(screen.contains("│  Old approval"), "{screen}");
}

#[tokio::test]
async fn the_conversation_mixes_comments_and_reviews_with_text_in_order() {
    let mut pull = pr(1, "One", "bob");
    pull.comments = vec![
        comment(3, "bob", "fifth comment", "2026-03-05T09:00:00Z"),
        comment(1, "carol", "first comment", "2026-03-02T09:00:00Z"),
    ];
    pull.reviews = vec![
        review(
            11,
            "dave",
            ReviewState::ChangesRequested,
            "second: please fix",
            "2026-03-03T10:00:00Z",
        ),
        review(
            12,
            "erin",
            ReviewState::Approved,
            "",
            "2026-03-04T10:00:00Z",
        ),
        review(
            13,
            "frank",
            ReviewState::Commented,
            "fourth: a question",
            "2026-03-04T11:00:00Z",
        ),
        review(
            14,
            "gina",
            ReviewState::Approved,
            "last: ship it",
            "2026-03-06T08:00:00Z",
        ),
    ];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    // Five entries: three comments... two comments and three reviews with text; erin's empty review is left out.
    assert!(screen.contains("Conversation (5)"), "{screen}");
    let rows: Vec<usize> = [
        "first comment",
        "second: please fix",
        "fourth: a question",
        "fifth comment",
        "last: ship it",
    ]
    .iter()
    .map(|text| row_of(&screen, text))
    .collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}\n{screen}");
    assert!(!screen.contains("@erin ·"), "{screen}");
    assert!(screen.contains("@carol · 2026-03-02"), "{screen}");
    assert!(
        screen.contains("@dave · 2026-03-03 · Request changes"),
        "{screen}"
    );
    assert!(screen.contains("@frank · 2026-03-04 · review"), "{screen}");
    assert!(screen.contains("@gina · 2026-03-06 · Approve"), "{screen}");
    // Text is indented by two spaces.
    assert!(screen.contains("│  first comment"), "{screen}");
}

#[tokio::test]
async fn a_pr_without_comments_says_so() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    let screen = app.screen();
    assert!(screen.contains("Conversation (0)"), "{screen}");
    assert!(screen.contains("No comments."), "{screen}");
}

#[tokio::test]
async fn a_review_without_a_date_goes_last() {
    let mut pull = pr(1, "One", "bob");
    pull.comments = vec![comment(1, "carol", "a comment", "2026-03-02T09:00:00Z")];
    let mut undated = review(5, "dave", ReviewState::Commented, "undated review", "x");
    undated.submitted_at = None;
    pull.reviews = vec![undated];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    // A Pending review is dropped by the client, so the fake's undated Commented review is rendered by its
    // state: here the fake serves it as submitted without date.
    assert!(
        row_of(&screen, "a comment") < row_of(&screen, "undated review"),
        "{screen}"
    );
    assert!(screen.contains("@dave · review"), "{screen}");
}

#[tokio::test]
async fn the_cases_of_the_action_line() {
    // Open PR of someone else: everything.
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    assert!(
        app.screen()
            .contains("c Comment · a Approve · x Request changes · d Diff"),
        "{}",
        app.screen()
    );
    // Own PR: Comment only, with the reason.
    let mut app = open_detail(state_with(vec![pr(1, "Mine", "alice")]), 1).await;
    let screen = app.screen();
    assert!(
        screen.contains("c Comment · d Diff · Verdicts are not offered on your own PR."),
        "{screen}"
    );
    assert!(!screen.contains("a Approve"), "{screen}");
    // Closed and merged.
    for (pull_state, text) in [
        (
            PullState::Closed,
            "d Diff · This PR is closed: no comment or review.",
        ),
        (
            PullState::Merged,
            "d Diff · This PR is merged: no comment or review.",
        ),
    ] {
        let mut pull = pr(1, "One", "bob");
        pull.state = pull_state;
        let mut app = open_list(state_with(vec![pull])).await;
        app.press(KeyCode::Tab);
        app.wait_for("One").await;
        app.press(KeyCode::Enter);
        app.wait_for("Description").await;
        let screen = app.screen();
        assert!(screen.contains(text), "{screen}");
        assert!(!screen.contains("c Comment"), "{screen}");
    }
}

#[tokio::test]
async fn a_repo_archived_after_the_load_loses_its_actions_after_r() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    assert!(
        app.screen().contains("c Comment · a Approve"),
        "{}",
        app.screen()
    );
    app.fake.with_state(|s| {
        for repo in &mut s.repos {
            repo.archived = true;
        }
    });
    // Nothing changes before the refresh...
    assert!(app.screen().contains("c Comment · a Approve"));
    app.press(KeyCode::Char('r'));
    app.wait_for("This repo is archived: read-only.").await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        screen.contains("d Diff · This repo is archived: read-only."),
        "{screen}"
    );
    assert!(
        !screen.contains("a Approve") && !screen.contains("c Comment"),
        "{screen}"
    );
    // ... and the keys of the removed actions do nothing.
    for key in ['c', 'a', 'x'] {
        app.press(KeyCode::Char(key));
        let screen = app.screen();
        assert!(
            screen.contains("ghu · alice/notes · #1"),
            "{key}:\n{screen}"
        );
        assert!(!screen.contains("Not available yet."), "{key}:\n{screen}");
    }
}

#[tokio::test]
async fn an_action_key_the_pr_does_not_allow_does_nothing_and_an_allowed_one_opens_its_screen() {
    // Own PR: c works, a and x do nothing.
    let mut app = open_detail(state_with(vec![pr(1, "Mine", "alice")]), 1).await;
    for key in ['a', 'x'] {
        app.press(KeyCode::Char(key));
        assert!(
            app.screen().contains("ghu · alice/notes · #1\n"),
            "{}",
            app.screen()
        );
        assert!(
            !app.screen().contains("Not available yet."),
            "{}",
            app.screen()
        );
    }
    app.press(KeyCode::Char('c'));
    assert!(
        app.screen().contains("ghu · alice/notes · #1 · Comment"),
        "{}",
        app.screen()
    );
    app.press(KeyCode::Esc);
    assert!(
        app.screen().contains("ghu · alice/notes · #1\n"),
        "{}",
        app.screen()
    );

    // Someone else's open PR: every action opens its screen, Esc comes back to the unchanged detail.
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    for (key, title) in [
        ('c', "· Comment"),
        ('a', "· Review: Approve"),
        ('x', "· Review: Request changes"),
        ('d', "· Diff"),
    ] {
        app.press(KeyCode::Char(key));
        let screen = app.screen();
        assert!(
            screen.contains(&format!("ghu · alice/notes · #1 {title}")),
            "{key}:\n{screen}"
        );
        app.press(KeyCode::Esc);
        let screen = app.screen();
        assert!(screen.contains("Verdicts: none"), "{key}:\n{screen}");
    }
    // Closed PR: the three publishing keys do nothing, Diff still opens.
    let mut closed = pr(1, "One", "bob");
    closed.state = PullState::Closed;
    let mut app = open_list(state_with(vec![closed])).await;
    app.press(KeyCode::Tab);
    app.wait_for("One").await;
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    for key in ['c', 'a', 'x'] {
        app.press(KeyCode::Char(key));
        assert!(
            !app.screen().contains("Not available yet."),
            "{key}: {}",
            app.screen()
        );
    }
    app.press(KeyCode::Char('d'));
    assert!(app.screen().contains("· Diff"), "{}", app.screen());
}

#[tokio::test]
async fn opening_a_pr_reads_it_and_never_publishes() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    let requests = app.fake.requests();
    for path in [
        "/repos/alice/notes/pulls/1",
        "/repos/alice/notes/pulls/1/reviews",
        "/repos/alice/notes/issues/1/comments",
        "/user",
    ] {
        assert!(
            requests
                .iter()
                .any(|r| r.method == Method::Get && r.path == path),
            "{path} not requested: {requests:?}"
        );
    }
    assert!(
        requests.iter().all(|r| r.method == Method::Get),
        "{requests:?}"
    );
    let _ = app.screen();
}

#[tokio::test]
async fn esc_returns_to_the_unchanged_pr_list() {
    let mut app = open_detail(
        state_with(vec![pr(1, "First", "bob"), pr(2, "Second", "bob")]),
        1,
    )
    .await;
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(
        screen.contains("ghu · alice/notes · Pull requests (open)"),
        "{screen}"
    );
    assert!(
        screen.contains("First") && screen.contains("Second"),
        "{screen}"
    );
    // The selection of the list is kept: the PR opened (#1, second row) is still the selected one.
    assert!(line_of(&screen, ">").contains("#1"), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn a_slow_load_shows_loading_and_keeps_the_keys_alive() {
    let mut state = state_with(vec![pr(1, "Slow one", "bob")]);
    let mut app = open_list(state.clone()).await;
    app.fake
        .with_state(|s| s.latency = Duration::from_millis(200));
    state.latency = Duration::from_millis(200);
    app.press(KeyCode::Enter);
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1"), "{screen}");
    assert!(screen.contains("Loading"), "{screen}");
    assert!(!screen.contains("Description"), "{screen}");
    app.wait_for("Description").await;
    app.settle().await;
    assert!(!app.screen().contains("Loading"), "{}", app.screen());

    // Leaving before the answer arrives: no panic, no stale content on the list.
    app.press(KeyCode::Char('r'));
    app.press(KeyCode::Esc);
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("Pull requests (open)"), "{screen}");
    assert!(!screen.contains("Description"), "{screen}");
    assert!(!app.is_finished());
}

#[tokio::test]
async fn a_404_and_a_500_show_a_readable_message_and_r_retries() {
    let mut state = state_with(vec![pr(1, "One", "bob")]);
    state.failures.push(failing(
        "/repos/alice/notes/pulls/1",
        FailureResponse::NotFound,
        Some(1),
    ));
    let mut app = open_list(state).await;
    app.press(KeyCode::Enter);
    app.wait_for("Cannot load the pull request").await;
    let screen = app.screen();
    assert!(screen.contains("not found on GitHub (404)"), "{screen}");
    assert!(!screen.contains("Loading"), "{screen}");
    assert!(!screen.contains("Description"), "{screen}");
    app.press(KeyCode::Char('r'));
    app.wait_for("Description").await;
    assert!(!app.screen().contains("Cannot load"), "{}", app.screen());

    let mut state = state_with(vec![pr(1, "One", "bob")]);
    state.failures.push(failing(
        "/repos/alice/notes/issues/1/comments",
        FailureResponse::ServerError,
        Some(1),
    ));
    let mut app = open_list(state).await;
    app.press(KeyCode::Enter);
    app.wait_for("Cannot load the pull request").await;
    let screen = app.screen();
    assert!(
        screen.contains("GitHub answered with an error (HTTP 500)"),
        "{screen}"
    );
    app.press(KeyCode::Char('r'));
    app.wait_for("Description").await;
    assert!(!app.screen().contains("Cannot load"), "{}", app.screen());
}

#[tokio::test]
async fn r_keeps_the_content_until_the_answer_and_shows_the_news() {
    let mut app = open_detail(state_with(vec![pr(1, "Old title", "bob")]), 1).await;
    app.fake.with_state(|s| {
        s.latency = Duration::from_millis(200);
        s.pull_mut("alice", "notes", 1).expect("pr").title = "New title".into();
    });
    app.press(KeyCode::Char('r'));
    let screen = app.screen();
    assert!(
        screen.contains("#1 Old title"),
        "content dropped:\n{screen}"
    );
    assert!(screen.contains("Loading"), "{screen}");
    app.wait_for("#1 New title").await;
    app.settle().await;
    assert!(!app.screen().contains("Old title"), "{}", app.screen());
}

#[tokio::test]
async fn only_the_answer_to_the_latest_request_counts() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    // Request A is slow and would succeed; request B is fast and fails. B is the latest: its error stays.
    app.fake
        .with_state(|s| s.latency = Duration::from_millis(300));
    app.press(KeyCode::Char('r'));
    tokio::time::sleep(Duration::from_millis(100)).await;
    app.fake.with_state(|s| {
        s.latency = Duration::ZERO;
        s.failures.push(failing(
            "/repos/alice/notes/pulls/1",
            FailureResponse::ServerError,
            Some(1),
        ));
    });
    app.press(KeyCode::Char('r'));
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("Cannot load the pull request"), "{screen}");
    assert!(!screen.contains("Description"), "{screen}");
}

#[tokio::test]
async fn scrolling_is_bounded_and_up_reacts_at_the_first_press_after_the_stop() {
    let mut app = open_detail(state_with(vec![long_pr(100)]), 1).await;
    let top = app.screen();
    assert!(
        top.contains("line 001") && !top.contains("line 100"),
        "{top}"
    );
    press_times(&mut app, KeyCode::Up, 3);
    assert_eq!(app.screen(), top, "Up at the top moved the screen");

    press_times(&mut app, KeyCode::Down, 400);
    let end = app.screen();
    assert!(end.contains("No comments."), "{end}");
    assert!(!end.contains("line 001"), "{end}");
    let last = row_of(&end, "line 100");
    let bottom = row_of(&end, "No comments.");
    assert!(
        end.lines().nth(bottom + 1).is_some_and(|l| l.contains('└')),
        "blank rows under the end:\n{end}"
    );
    // More presses of Down and of j change nothing.
    press_times(&mut app, KeyCode::Down, 5);
    press_times(&mut app, KeyCode::Char('j'), 5);
    assert_eq!(app.screen(), end);
    // The first Up moves by exactly one line.
    app.press(KeyCode::Up);
    let up = app.screen();
    assert_eq!(row_of(&up, "line 100"), last + 1, "{up}");
    app.press(KeyCode::Char('j'));
    assert_eq!(app.screen(), end);
    app.press(KeyCode::Char('k'));
    assert_eq!(row_of(&app.screen(), "line 100"), last + 1);
}

#[tokio::test]
async fn page_keys_move_by_about_a_page_and_stay_bounded() {
    let mut app = open_detail(state_with(vec![long_pr(100)]), 1).await;
    app.press(KeyCode::PageDown);
    let page = app.screen();
    let first = first_numbered_line(&page);
    assert!(
        (15..=30).contains(&first),
        "PgDn moved to line {first}:\n{page}"
    );
    app.press(KeyCode::PageUp);
    let back = app.screen();
    assert!(
        back.contains("line 001") && back.contains("Description"),
        "{back}"
    );
    press_times(&mut app, KeyCode::PageDown, 30);
    let end = app.screen();
    assert!(end.contains("No comments."), "{end}");
    press_times(&mut app, KeyCode::PageDown, 3);
    assert_eq!(app.screen(), end);
    press_times(&mut app, KeyCode::PageUp, 30);
    assert_eq!(app.screen(), back);
}

#[tokio::test]
async fn long_lines_wrap_and_nothing_is_lost() {
    let words: Vec<String> = (1..=60).map(|i| format!("word{i:02}")).collect();
    let mut pull = pr(1, "T", "bob");
    pull.title = words.join(" ");
    pull.body = Some(words.join(" "));
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    // Every word appears (title and description: twice each), whole, on a row of the right width.
    let screen = app.screen();
    assert!(screen.contains("word60"), "{screen}");
    assert_eq!(screen.matches("word01").count(), 2, "{screen}");
    assert!(screen.lines().all(|l| l.chars().count() <= 100), "{screen}");
    // A word longer than the row is cut, not lost.
    let mut pull = pr(1, "T", "bob");
    pull.body = Some("z".repeat(250));
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    let total: usize = screen
        .lines()
        .map(|l| l.chars().filter(|&c| c == 'z').count())
        .sum();
    assert_eq!(total, 250, "{screen}");
}

#[tokio::test]
async fn network_text_never_reaches_the_terminal_raw() {
    let hostile = "bad \u{1b}[31mred\u{202e}x a\rb";
    let mut pull = pr(1, "T\u{1b}[2Jitle \u{202e}", "bob");
    pull.body = Some(hostile.to_owned());
    pull.head = "br\u{1b}anch".into();
    pull.comments = vec![comment(1, "ca\u{1b}rol", hostile, "2026-03-02T09:00:00Z")];
    pull.reviews = vec![review(
        2,
        "dave",
        ReviewState::Commented,
        hostile,
        "2026-03-03T09:00:00Z",
    )];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    assert!(!screen.contains('\u{1b}'), "raw ESC on screen: {screen:?}");
    assert!(
        !screen.contains('\u{202e}'),
        "raw bidi mark on screen: {screen:?}"
    );
    // Description, Comment and Review text: the escape, the mark and the lone CR are made visible.
    assert_eq!(
        screen
            .matches("bad \u{fffd}[31mred\u{fffd}x a\u{fffd}b")
            .count(),
        3,
        "{screen}"
    );
    assert!(screen.contains("#1 T\u{fffd}[2Jitle \u{fffd}"), "{screen}");
    assert!(screen.contains("alice:br\u{fffd}anch"), "{screen}");
    assert!(screen.contains("@ca\u{fffd}rol"), "{screen}");
}

#[tokio::test]
async fn crlf_ends_a_line_without_a_replacement_character_but_a_lone_cr_does_not() {
    let mut pull = pr(1, "T", "bob");
    pull.body = Some("body one\r\nbody two\r\n\r\nbody four".to_owned());
    pull.comments = vec![comment(
        1,
        "carol",
        "first\r\nsecond",
        "2026-03-02T09:00:00Z",
    )];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    assert!(!screen.contains('\u{fffd}'), "{screen}");
    assert_eq!(
        row_of(&screen, "body two"),
        row_of(&screen, "body one") + 1,
        "{screen}"
    );
    assert_eq!(
        row_of(&screen, "body four"),
        row_of(&screen, "body one") + 3,
        "{screen}"
    );
    assert!(screen.contains("│  first"), "{screen}");
    assert_eq!(
        row_of(&screen, "second"),
        row_of(&screen, "first") + 1,
        "{screen}"
    );

    let mut pull = pr(1, "T", "bob");
    pull.comments = vec![comment(1, "carol", "a\rb", "2026-03-02T09:00:00Z")];
    let mut app = open_detail(state_with(vec![pull]), 1).await;
    let screen = app.screen();
    assert!(screen.contains("│  a\u{fffd}b"), "{screen}");
}

#[tokio::test]
async fn the_help_lists_the_keys_of_the_screen() {
    let mut app = open_detail(state_with(vec![pr(1, "One", "bob")]), 1).await;
    app.press(KeyCode::Char('?'));
    let screen = app.screen();
    for text in [
        "Up/k",
        "Down/j",
        "PgUp",
        "PgDn",
        "Refresh",
        "Diff",
        "Comment",
        "Approve",
        "Request changes",
    ] {
        assert!(screen.contains(text), "{text} missing:\n{screen}");
    }
}

#[tokio::test]
async fn the_demo_conversation_reads_in_order_with_reviews_that_have_text() {
    let mut app = open_detail(demo_notes(), 1).await;
    press_times(&mut app, KeyCode::Down, 200);
    let screen = app.screen();
    assert!(screen.contains("Conversation (4)"), "{screen}");
    let rows: Vec<usize> = [
        "Thanks, good catch.",
        "Please also fix the title.",
        "Fixed in the last commit.",
        "Looks fine, one question about the guide.",
    ]
    .iter()
    .map(|text| row_of(&screen, text))
    .collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}\n{screen}");
    assert!(
        screen.contains("@dave · 2026-03-02 · Request changes"),
        "{screen}"
    );
    assert!(screen.contains("@erin · 2026-03-03 · review"), "{screen}");
    assert!(screen.contains("@carol · 2026-03-02"), "{screen}");
}
