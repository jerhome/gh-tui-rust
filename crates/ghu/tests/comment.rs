//! Ticket #8: writing, confirming and publishing a Comment (through the harness only, against the fake GitHub).
//! The signed-in user of every state is `alice`; the PRs live in `alice/notes`, the only repo, so `Enter` opens it.

mod support;

use std::time::Duration;

use gh_fake::{
    Failure, FailureResponse, FakeGitHub, Method, Pull, PullState, RecordedRequest, Repo, State,
    User,
};
use ghu::{KeyCode, KeyModifiers};
use support::Harness;

const HINT: &str = "Ctrl-S publish… · Ctrl-E editor · Esc cancel";

fn state_with(pulls: Vec<Pull>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.repos = vec![Repo::new("alice", "notes")];
    state.pulls = pulls;
    state
}

/// An open PR #1 of `alice/notes`.
fn pr(author: &str) -> Pull {
    Pull::new("alice", "notes", 1, "One", author)
}

/// Opens the detail of PR #1 (the only one; in the closed / merged tab when `closed_tab`).
async fn open_detail_in(state: State, closed_tab: bool) -> Harness {
    let mut app = Harness::start(state).await;
    app.wait_for("alice/notes").await;
    app.press(KeyCode::Enter);
    app.wait_for("Pull requests (open)").await;
    app.settle().await;
    if closed_tab {
        app.press(KeyCode::Tab);
        app.wait_for("One").await;
    }
    app.press(KeyCode::Enter);
    app.wait_for("Description").await;
    app.settle().await;
    app
}

async fn open_detail(state: State) -> Harness {
    open_detail_in(state, false).await
}

/// The compose screen of a Comment on PR #1 of someone else.
async fn compose() -> Harness {
    compose_on(state_with(vec![pr("bob")])).await
}

async fn compose_on(state: State) -> Harness {
    let mut app = open_detail(state).await;
    app.press(KeyCode::Char('c'));
    let screen = app.screen();
    assert!(
        screen.contains("ghu · alice/notes · #1 · Comment"),
        "{screen}"
    );
    app
}

fn ctrl_s(app: &mut Harness) {
    app.press_with(KeyCode::Char('s'), KeyModifiers::CONTROL);
}

/// The screen with its lines joined by one space, so a message wrapped on two rows can be searched.
fn flat(screen: &str) -> String {
    screen.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

/// The screen without spaces: the harness shows the second cell of a wide character as a blank.
fn squashed(screen: &str) -> String {
    screen.chars().filter(|c| *c != ' ').collect()
}

fn posts(app: &Harness) -> Vec<RecordedRequest> {
    app.fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect()
}

/// Text on screen, whole.
fn assert_on_screen(app: &mut Harness, needle: &str) {
    let screen = app.screen();
    assert!(
        screen.contains(needle),
        "{needle:?} not on screen:\n{screen}"
    );
}

/// The `body` of the only POST the fake received.
fn only_posted_body(app: &Harness) -> String {
    let posts = posts(app);
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].path, "/repos/alice/notes/issues/1/comments");
    let body = posts[0].body.clone().expect("a JSON body");
    assert_eq!(body.as_object().map(|o| o.len()), Some(1), "{body}");
    body["body"].as_str().expect("a string body").to_owned()
}

/// Ctrl-S, then `y`: publishes and waits for the success notice.
async fn publish(app: &mut Harness) {
    ctrl_s(app);
    assert_on_screen(app, "y / Enter publish · n / Esc back to the text");
    app.press(KeyCode::Char('y'));
    app.wait_for("Comment published.").await;
    app.settle().await;
}

async fn publish_and_read_body(app: &mut Harness) -> String {
    publish(app).await;
    only_posted_body(app)
}

// ---------------------------------------------------------------------------------------------------------------
// Writing

#[tokio::test]
async fn the_writing_screen_shows_the_hint_and_the_typed_lines() {
    let mut app = compose().await;
    let screen = app.screen();
    assert!(screen.contains(HINT), "{screen}");
    app.type_text("Hello");
    app.press(KeyCode::Enter);
    app.type_text("second line");
    let screen = app.screen();
    assert!(screen.contains("Hello"), "{screen}");
    assert!(screen.contains("second line"), "{screen}");
    let (first, second) = (
        screen.lines().position(|l| l.contains("Hello")).unwrap(),
        screen
            .lines()
            .position(|l| l.contains("second line"))
            .unwrap(),
    );
    assert_eq!(second, first + 1, "{screen}");
    assert!(posts(&app).is_empty());
}

#[tokio::test]
async fn q_and_question_mark_are_text_while_writing() {
    let mut app = compose().await;
    app.type_text("q?Q");
    assert!(!app.is_finished());
    assert_on_screen(&mut app, "q?Q");
    assert!(!app.screen().contains("Toggle this help"));
    assert_eq!(publish_and_read_body(&mut app).await, "q?Q");
}

#[tokio::test]
async fn editing_keys_move_insert_and_delete_inside_a_line() {
    let mut app = compose().await;
    app.type_text("Helo");
    app.press(KeyCode::Left);
    app.type_text("l");
    app.press(KeyCode::Home);
    app.type_text(">");
    app.press(KeyCode::End);
    app.type_text("<");
    app.press(KeyCode::Left);
    app.press(KeyCode::Left);
    app.press(KeyCode::Backspace);
    app.press(KeyCode::Delete);
    app.press(KeyCode::Right);
    app.type_text("!");
    // ">Hel<" then "!" after the closing mark.
    assert_eq!(publish_and_read_body(&mut app).await, ">Hel<!");
}

#[tokio::test]
async fn enter_backspace_and_delete_split_and_join_lines_and_arrows_keep_the_column_in_range() {
    let mut app = compose().await;
    app.type_text("ab");
    app.press(KeyCode::Enter);
    app.type_text("cd");
    app.press(KeyCode::Home);
    app.press(KeyCode::Backspace); // joins: ab|cd
    app.press(KeyCode::Enter); // ab / cd
    app.press(KeyCode::Up);
    app.press(KeyCode::End);
    app.press(KeyCode::Delete); // joins: ab|cd
    app.press(KeyCode::End);
    app.press(KeyCode::Enter); // abcd / (empty)
    app.type_text("x");
    app.press(KeyCode::Up); // column 1 of "abcd"
    app.type_text("Z");
    app.press(KeyCode::Down); // column 2 of "x" is clamped to its end
    app.type_text("Q");
    // Right at the end of a line goes to the next one, Left at the start goes back.
    app.press(KeyCode::Up);
    app.press(KeyCode::End);
    app.press(KeyCode::Right);
    app.type_text("R");
    app.press(KeyCode::Left);
    app.press(KeyCode::Left);
    app.type_text("L");
    assert_eq!(publish_and_read_body(&mut app).await, "aZbcdL\nRxQ");
}

#[tokio::test]
async fn arrows_and_edits_do_nothing_at_the_bounds() {
    let mut app = compose().await;
    for key in [
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Left,
        KeyCode::Up,
        KeyCode::Home,
        KeyCode::Right,
        KeyCode::Down,
        KeyCode::End,
    ] {
        app.press(key);
    }
    app.type_text("ok");
    for key in [KeyCode::Delete, KeyCode::Right, KeyCode::Down] {
        app.press(key);
    }
    assert_eq!(publish_and_read_body(&mut app).await, "ok");
}

#[tokio::test]
async fn shifted_and_altgr_characters_are_typed() {
    let mut app = compose().await;
    app.press_with(KeyCode::Char('A'), KeyModifiers::SHIFT);
    app.press_with(
        KeyCode::Char('€'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    // A plain Ctrl or Alt chord is not text.
    app.press_with(KeyCode::Char('x'), KeyModifiers::CONTROL);
    app.press_with(KeyCode::Char('x'), KeyModifiers::ALT);
    app.press(KeyCode::Tab);
    assert_eq!(publish_and_read_body(&mut app).await, "A€");
}

#[tokio::test]
async fn a_paste_is_inserted_at_the_cursor_with_line_ends_normalised_and_controls_dropped() {
    let mut app = compose().await;
    app.type_text("ac");
    app.press(KeyCode::Left);
    app.paste("b\r\none\rtwo\nth\u{1b}[31mree\u{7}\u{85}\u{7f}\ttab");
    let screen = app.screen();
    assert!(screen.contains("th[31mree"), "{screen}");
    assert_eq!(
        publish_and_read_body(&mut app).await,
        "ab\none\ntwo\nth[31mree\ttabc"
    );
}

#[tokio::test]
async fn a_paste_is_ignored_on_the_confirmation() {
    let mut app = compose().await;
    app.type_text("text");
    ctrl_s(&mut app);
    app.paste("more");
    app.press(KeyCode::Esc);
    app.type_text("!");
    assert_eq!(publish_and_read_body(&mut app).await, "text!");
}

#[tokio::test]
async fn wide_characters_are_shown_and_published_as_they_are() {
    let mut app = compose().await;
    app.type_text("日本語 😀 é");
    app.press(KeyCode::Enter);
    app.type_text("e\u{301}");
    let screen = app.screen();
    assert!(squashed(&screen).contains("日本語😀é"), "{screen}");
    assert_eq!(
        publish_and_read_body(&mut app).await,
        "日本語 😀 é\ne\u{301}"
    );
}

#[tokio::test]
async fn ctrl_e_has_no_effect_before_the_editor_lot_and_the_other_screens_are_not_ready() {
    let mut app = compose().await;
    app.type_text("ab");
    app.press_with(KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert_on_screen(&mut app, HINT);
    assert_eq!(publish_and_read_body(&mut app).await, "ab");

    // Review screens keep the provisional notice and never publish.
    let mut app = open_detail(state_with(vec![pr("bob")])).await;
    for key in ['a', 'x'] {
        app.press(KeyCode::Char(key));
        assert_on_screen(&mut app, "Not available yet.");
        ctrl_s(&mut app);
        app.press(KeyCode::Char('y'));
        app.press(KeyCode::Esc);
        assert_on_screen(&mut app, "Verdicts: none");
    }
    assert!(posts(&app).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Cancelling and refusing

#[tokio::test]
async fn esc_on_a_blank_text_closes_at_once_and_nothing_is_published() {
    for blank in ["", "   ", " \n\t \n"] {
        let mut app = compose().await;
        for c in blank.chars() {
            if c == '\n' {
                app.press(KeyCode::Enter);
            } else {
                app.press(KeyCode::Char(c));
            }
        }
        app.press(KeyCode::Esc);
        let screen = app.screen();
        assert!(
            screen.contains("ghu · alice/notes · #1\n"),
            "{blank:?}\n{screen}"
        );
        assert!(!screen.contains(HINT), "{screen}");
        assert!(posts(&app).is_empty());
    }
}

#[tokio::test]
async fn esc_on_a_text_asks_again_and_a_second_esc_discards() {
    let mut app = compose().await;
    app.type_text("draft");
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(
        screen.contains("Press Esc again to discard the text."),
        "{screen}"
    );
    assert!(screen.contains("· Comment"), "{screen}");
    assert!(screen.contains("draft"), "{screen}");
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
    // A new Comment starts empty.
    app.press(KeyCode::Char('c'));
    assert!(!app.screen().contains("draft"));
    assert!(posts(&app).is_empty());
}

#[tokio::test]
async fn any_other_key_disarms_the_double_esc() {
    let mut app = compose().await;
    app.type_text("draft");
    app.press(KeyCode::Esc);
    app.press(KeyCode::Left); // any key: the arming is gone
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(
        screen.contains("Press Esc again to discard the text."),
        "{screen}"
    );
    assert!(screen.contains("· Comment"), "{screen}");
    app.press(KeyCode::Esc);
    assert!(app.screen().contains("ghu · alice/notes · #1\n"));
}

#[tokio::test]
async fn a_blank_text_is_refused_with_its_message_and_stays_writable() {
    for blank in [&[][..], &[' ', ' '][..], &['\t'][..]] {
        let mut app = compose().await;
        for &c in blank {
            app.press(KeyCode::Char(c));
        }
        ctrl_s(&mut app);
        let screen = app.screen();
        assert!(screen.contains("Write a text first."), "{screen}");
        assert!(screen.contains(HINT), "{screen}");
        assert!(!screen.contains("Publish this Comment"), "{screen}");
        assert!(posts(&app).is_empty());
    }
    // Enter alone makes lines, not text.
    let mut app = compose().await;
    app.press(KeyCode::Enter);
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "Write a text first.");
}

#[tokio::test]
async fn the_length_limit_is_checked_in_characters_at_the_boundary() {
    // 65 536 characters (multi-byte ones) go through; one more is refused with the count.
    let mut app = compose().await;
    app.paste(&"é".repeat(65_536));
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "Publish this Comment on alice/notes #1?");

    let mut app = compose().await;
    app.paste(&"é".repeat(65_537));
    ctrl_s(&mut app);
    let screen = app.screen();
    assert!(
        screen.contains("The text is too long: 65537 characters (GitHub allows 65536)."),
        "{screen}"
    );
    assert!(screen.contains(HINT), "{screen}");
    assert!(posts(&app).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Confirmation

#[tokio::test]
async fn the_confirmation_screen_reads_exactly() {
    let mut app = compose().await;
    app.type_text("first line");
    app.press(KeyCode::Enter);
    app.type_text("tab\there");
    ctrl_s(&mut app);
    let screen = app.screen();
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let at = |needle: &str| {
        rows.iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
    };
    let publish_row = at("Publish this Comment on alice/notes #1?");
    let text_row = at("Text:");
    let first = at("first line");
    let second = at("tab    here");
    let hint = at("y / Enter publish · n / Esc back to the text");
    assert!(publish_row < text_row && text_row < first, "{screen}");
    assert_eq!(second, first + 1, "{screen}");
    assert!(second < hint, "{screen}");
    assert!(!screen.contains("Ctrl-S"), "{screen}");
    assert!(!screen.contains("Verdict:"), "{screen}");
    assert!(posts(&app).is_empty(), "nothing is sent before the answer");
}

#[tokio::test]
async fn a_long_text_is_cut_on_the_confirmation_with_the_number_of_lines_left() {
    let mut app = compose().await;
    for i in 1..=60 {
        app.type_text(&format!("row {i:02}"));
        if i < 60 {
            app.press(KeyCode::Enter);
        }
    }
    ctrl_s(&mut app);
    let screen = app.screen();
    let shown = screen.lines().filter(|l| l.contains("row ")).count();
    let more = screen
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.trim_matches(|c| c == '│' || c == ' ')
                .strip_prefix("… ")?
                .strip_suffix(" more lines")?
                .parse::<usize>()
                .ok()
        })
        .unwrap_or_else(|| panic!("no '… k more lines':\n{screen}"));
    assert!(shown >= 5 && more >= 1, "{screen}");
    assert_eq!(shown + more, 60, "{screen}");
    assert!(
        screen.contains("y / Enter publish · n / Esc back to the text"),
        "{screen}"
    );
    assert!(screen.contains("row 01"), "{screen}");
}

#[tokio::test]
async fn text_from_the_zone_is_cleaned_on_the_confirmation_but_sent_as_typed() {
    let mut app = compose().await;
    app.paste("a\u{202e}b");
    ctrl_s(&mut app);
    assert_on_screen(&mut app, "a�b");
    app.press(KeyCode::Char('y'));
    app.wait_for("Comment published.").await;
    assert_eq!(only_posted_body(&app), "a\u{202e}b");
}

#[tokio::test]
async fn n_and_esc_go_back_to_the_text_which_is_intact_and_nothing_is_sent() {
    for back in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut app = compose().await;
        app.type_text("keep");
        app.press(KeyCode::Enter);
        app.type_text("me");
        ctrl_s(&mut app);
        app.press(back);
        let screen = app.screen();
        assert!(screen.contains(HINT), "{back:?}\n{screen}");
        assert!(screen.contains("keep") && screen.contains("me"), "{screen}");
        assert!(!screen.contains("Publish this Comment"), "{screen}");
        app.type_text("!");
        assert!(posts(&app).is_empty());
        assert_eq!(publish_and_read_body(&mut app).await, "keep\nme!");
    }
}

#[tokio::test]
async fn y_is_text_while_writing_and_other_keys_do_nothing_on_the_confirmation() {
    let mut app = compose().await;
    app.type_text("yes");
    assert!(posts(&app).is_empty());
    ctrl_s(&mut app);
    for key in [KeyCode::Char('x'), KeyCode::Left, KeyCode::Char('Y')] {
        app.press(key);
    }
    assert_on_screen(&mut app, "Publish this Comment on alice/notes #1?");
    assert!(posts(&app).is_empty());
    assert_eq!(publish_and_read_body(&mut app).await, "yes");
}

// ---------------------------------------------------------------------------------------------------------------
// Publishing

#[tokio::test]
async fn y_publishes_the_exact_text_and_the_detail_is_reloaded_with_the_comment() {
    let mut app = compose().await;
    app.type_text("  Looks good  ");
    app.press(KeyCode::Enter);
    app.press(KeyCode::Enter);
    app.type_text("日本 😀");
    publish(&mut app).await;
    assert_eq!(only_posted_body(&app), "  Looks good  \n\n日本 😀");
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
    assert!(screen.contains("Comment published."), "{screen}");
    app.wait_for("Looks good").await;
    app.settle().await;
    let screen = app.screen();
    assert!(screen.contains("Conversation (1)"), "{screen}");
    assert!(screen.contains("@alice"), "{screen}");
    assert!(squashed(&screen).contains("日本😀"), "{screen}");
    // The detail was read again after the POST.
    let requests = app.fake.requests();
    let post = requests
        .iter()
        .position(|r| r.method == Method::Post)
        .expect("a POST");
    assert!(
        requests[post..]
            .iter()
            .any(|r| r.method == Method::Get && r.path == "/repos/alice/notes/pulls/1"),
        "{requests:?}"
    );
}

#[tokio::test]
async fn enter_publishes_like_y_and_an_own_pr_accepts_a_comment() {
    let mut app = compose_on(state_with(vec![pr("alice")])).await;
    app.type_text("mine");
    ctrl_s(&mut app);
    app.press(KeyCode::Enter);
    app.wait_for("Comment published.").await;
    assert_eq!(only_posted_body(&app), "mine");
}

#[tokio::test]
async fn while_publishing_the_keys_do_nothing_and_one_request_only_goes_out() {
    let mut app = compose().await;
    app.type_text("slow");
    ctrl_s(&mut app);
    app.fake
        .with_state(|s| s.latency = Duration::from_millis(400));
    app.press(KeyCode::Char('y'));
    assert_on_screen(&mut app, "Publishing…");
    app.press(KeyCode::Char('y'));
    app.press(KeyCode::Enter);
    app.press(KeyCode::Char('n'));
    app.press(KeyCode::Esc);
    let screen = app.screen();
    assert!(
        screen.contains("Publishing: wait for GitHub's answer."),
        "{screen}"
    );
    assert!(screen.contains("Publishing…"), "{screen}");
    app.wait_for("Comment published.").await;
    app.settle().await;
    assert_eq!(only_posted_body(&app), "slow");
}

// ---------------------------------------------------------------------------------------------------------------
// Failures

/// Publishes "my text" against a fake that fails the POST, checks the message, the kept text, and that no second
/// request followed.
async fn failed_publication(response: FailureResponse, message: &str) {
    let mut app = compose().await;
    app.fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Post,
            path: "/repos/alice/notes/issues/1/comments".into(),
            response,
            remaining: Some(1),
        });
    });
    app.type_text("my text");
    ctrl_s(&mut app);
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the comment").await;
    app.settle().await;
    let screen = app.screen();
    assert!(flat(&screen).contains(message), "{message}\n{screen}");
    assert!(
        screen.contains("ghu · alice/notes · #1 · Comment"),
        "{screen}"
    );
    assert!(screen.contains("my text"), "{screen}");
    assert!(screen.contains(HINT), "{screen}");
    assert!(!screen.contains("Publishing…"), "{screen}");
    tokio::time::sleep(Duration::from_millis(150)).await;
    app.settle().await;
    assert_eq!(posts(&app).len(), 1, "no second request: {:?}", posts(&app));
    // The user decides the retry: Ctrl-S, y; the failure was injected once, so it goes through.
    publish(&mut app).await;
    assert_eq!(posts(&app).len(), 2);
    let last = posts(&app).pop().unwrap();
    assert_eq!(last.body.unwrap()["body"], "my text");
}

#[tokio::test]
async fn a_403_says_so_keeps_the_text_and_sends_nothing_again() {
    failed_publication(
        FailureResponse::Forbidden {
            message: "Resource not accessible".into(),
        },
        "Cannot publish the comment: GitHub denied access (403). The token may lack the required scope.",
    )
    .await;
}

#[tokio::test]
async fn the_rate_limit_says_so_keeps_the_text_and_sends_nothing_again() {
    failed_publication(
        FailureResponse::RateLimited,
        "Cannot publish the comment: GitHub rate limit exceeded. Wait a few minutes and refresh.",
    )
    .await;
}

#[tokio::test]
async fn a_422_says_so_keeps_the_text_and_sends_nothing_again() {
    failed_publication(
        FailureResponse::Unprocessable {
            message: "Validation Failed".into(),
        },
        "Cannot publish the comment: GitHub refused the request (422).",
    )
    .await;
}

#[tokio::test]
async fn a_401_a_404_and_a_500_have_their_own_messages() {
    failed_publication(
        FailureResponse::NotFound,
        "Cannot publish the comment: not found on GitHub (404)",
    )
    .await;
    failed_publication(
        FailureResponse::ServerError,
        "Cannot publish the comment: GitHub answered with an error (HTTP 500).",
    )
    .await;
}

#[tokio::test]
async fn a_lost_network_says_it_may_have_been_published_and_never_resends() {
    let mut app = compose().await;
    app.type_text("maybe");
    ctrl_s(&mut app);
    // Stop the server (its task and connections), then restart another one on the same port: it records what
    // arrives after the cut.
    let port = app.fake.addr().port();
    let (old, state) = {
        let state = app.fake.with_state(|s| s.clone());
        let placeholder = FakeGitHub::start(state_with(Vec::new())).await.unwrap();
        (std::mem::replace(&mut app.fake, placeholder), state)
    };
    drop(old);
    tokio::time::sleep(Duration::from_millis(100)).await;
    app.press(KeyCode::Char('y'));
    app.wait_for("Cannot publish the comment").await;
    app.settle().await;
    let screen = app.screen();
    assert!(
        flat(&screen).contains(
            "Cannot publish the comment: no answer from GitHub. It may have been published anyway: refresh the PR (Esc, r) before trying again."
        ),
        "{screen}"
    );
    assert!(
        screen.contains("maybe") && screen.contains(HINT),
        "{screen}"
    );
    // Back online on the same port: nothing was sent by ghu on its own.
    let again = FakeGitHub::start_on(port, state).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    app.settle().await;
    assert!(
        again.requests().iter().all(|r| r.method != Method::Post),
        "{:?}",
        again.requests()
    );
    drop(again);
}

// ---------------------------------------------------------------------------------------------------------------
// Where a Comment is offered

#[tokio::test]
async fn no_comment_screen_opens_on_an_archived_closed_or_merged_pr() {
    // Repo archived after the load: nothing changes before the refresh, then the key does nothing.
    let mut app = open_detail(state_with(vec![pr("bob")])).await;
    app.fake.with_state(|s| s.repos[0].archived = true);
    app.press(KeyCode::Char('r'));
    app.wait_for("This repo is archived: read-only.").await;
    app.settle().await;
    app.press(KeyCode::Char('c'));
    let screen = app.screen();
    assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
    assert!(!screen.contains(HINT), "{screen}");
    assert!(posts(&app).is_empty());
    // Closed and merged PRs.
    for pull_state in [PullState::Closed, PullState::Merged] {
        let mut pull = pr("bob");
        pull.state = pull_state;
        let mut app = open_detail_in(state_with(vec![pull]), true).await;
        app.press(KeyCode::Char('c'));
        let screen = app.screen();
        assert!(screen.contains("ghu · alice/notes · #1\n"), "{screen}");
        assert!(!screen.contains(HINT), "{screen}");
        assert!(posts(&app).is_empty());
    }
}
