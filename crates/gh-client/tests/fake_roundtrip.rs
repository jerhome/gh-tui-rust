//! End-to-end tests of `gh_client::Client` against the real routes of `gh-fake` for the seven routes of ADR 0003
//! (#6-#10): the fake's JSON, written from GitHub's documentation, is decoded by the client; publications are seen
//! by the next GET; errors are classified and never carry the token.

use gh_client::{
    ApiBase, BodyProblem, Client, CommitSha, Error, FileStatus, Mergeable, MessageBody, PrNumber,
    PrState, RepoName, ReviewRequest, ReviewState, Token,
};
use gh_fake::{FakeGitHub, IssueComment, Method, PullFile, PullReview, State};
use gh_fake::{PullState, ReviewState as FakeReviewState};

const SECRET: &str = "ghp_TopSecretValue0123456789";

async fn start(customize: impl FnOnce(&mut State)) -> (FakeGitHub, Client) {
    let mut state = State::demo();
    state.token = SECRET.into();
    customize(&mut state);
    let fake = FakeGitHub::start(state).await.expect("fake starts");
    let base = ApiBase::parse(&fake.base_url()).expect("fake base URL is loopback http");
    let client = Client::new(base, Token::new(SECRET).expect("token")).expect("client");
    (fake, client)
}

fn repo(text: &str) -> RepoName {
    text.parse().expect("owner/name")
}

fn pr(number: u64) -> PrNumber {
    PrNumber::new(number).expect("number")
}

fn body(text: &str) -> MessageBody {
    MessageBody::new(text).expect("body")
}

fn head_sha(fake: &FakeGitHub, repo_name: &str, number: u64) -> CommitSha {
    let (owner, name) = repo_name.split_once('/').expect("owner/name");
    let sha = fake.with_state(|s| {
        s.pull_mut(owner, name, number)
            .expect("pull")
            .head_sha
            .clone()
    });
    CommitSha::parse(&sha).expect("the fake serves lowercase hex head shas")
}

fn assert_no_secret(error: &Error) {
    for text in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#?}"),
    ] {
        assert!(!text.contains(SECRET), "token leaked in {text}");
    }
}

fn unprocessable(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(error @ Error::Unprocessable { .. }) => {
            assert_no_secret(&error);
            match error {
                Error::Unprocessable { message } => message,
                _ => unreachable!(),
            }
        }
        other => panic!("expected Unprocessable, got {other:?}"),
    }
}

fn gets(fake: &FakeGitHub, suffix: &str) -> Vec<gh_fake::RecordedRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.method == Method::Get && r.path.ends_with(suffix))
        .collect()
}

fn posts(fake: &FakeGitHub) -> Vec<gh_fake::RecordedRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect()
}

#[tokio::test]
async fn current_user_decodes_the_fake_user() {
    let (_fake, client) = start(|_| {}).await;
    assert_eq!(client.current_user().await.expect("user").login, "alice");
}

#[tokio::test]
async fn pull_decodes_every_documented_field_of_the_demo_pr() {
    let (fake, client) = start(|_| {}).await;
    let detail = client
        .pull(&repo("alice/notes"), pr(1))
        .await
        .expect("pull");

    assert_eq!(detail.number, pr(1));
    assert_eq!(detail.title, "Fix typo in the README");
    assert_eq!(detail.author, "bob");
    assert_eq!(detail.state, PrState::Open { draft: false });
    let text = detail
        .body
        .as_deref()
        .expect("the demo PR has a description");
    assert!(
        text.starts_with("# Fix typo") && text.contains("notess"),
        "{text}"
    );
    assert_eq!(detail.head_label, "alice:feature");
    assert_eq!(detail.base_ref, "main");
    assert_eq!(detail.head_sha.as_str(), format!("{:040x}", 1));
    assert!(!detail.archived);
    assert_eq!(detail.mergeable, Mergeable::Yes);
    assert_eq!(detail.created_at.as_str(), "2026-02-03T09:00:00Z");
    assert_eq!(detail.updated_at.as_str(), "2026-03-03T12:30:00Z");
    // 5 files: 1+2+0+0+0 additions, 1+0+2+0+0 deletions, computed by the fake from its files.
    let (adds, dels, files) = fake.with_state(|s| {
        let p = s.pull_mut("alice", "notes", 1).expect("pull");
        (
            p.files.iter().map(|f| f.additions).sum::<u64>(),
            p.files.iter().map(|f| f.deletions).sum::<u64>(),
            p.files.len() as u64,
        )
    });
    assert_eq!(
        (detail.additions, detail.deletions, detail.changed_files),
        (adds, dels, files)
    );
    assert!(
        files == 5 && adds > 0 && dels > 0,
        "demo data changed: {files} {adds} {dels}"
    );
}

#[tokio::test]
async fn pull_maps_state_mergeable_archived_ghost_and_null_body() {
    let (_fake, client) = start(|s| {
        let p = s.pull_mut("alice", "notes", 2).expect("pull");
        p.author = None;
        p.mergeable = None;
        p.body = None;
    })
    .await;
    let ghost = client
        .pull(&repo("alice/notes"), pr(2))
        .await
        .expect("pull 2");
    assert_eq!(ghost.author, "ghost");
    assert_eq!(ghost.mergeable, Mergeable::Unknown);
    assert_eq!(ghost.body, None);

    let closed = client
        .pull(&repo("alice/notes"), pr(3))
        .await
        .expect("pull 3");
    assert_eq!(closed.state, PrState::Closed);
    let merged = client
        .pull(&repo("alice/notes"), pr(4))
        .await
        .expect("pull 4");
    assert_eq!(merged.state, PrState::Merged);
    let frozen = client
        .pull(&repo("acme/legacy-site"), pr(1))
        .await
        .expect("frozen");
    assert!(frozen.archived, "base.repo.archived must be read");
    let conflicting = client
        .pull(&repo("acme/widgets"), pr(1))
        .await
        .expect("conflicting");
    assert_eq!(conflicting.mergeable, Mergeable::Conflicts);
}

#[tokio::test]
async fn pull_and_lists_of_a_missing_pr_or_repo_are_not_found() {
    let (_fake, client) = start(|_| {}).await;
    let missing = repo("alice/notes");
    assert_eq!(
        client.pull(&missing, pr(99)).await.unwrap_err(),
        Error::NotFound
    );
    assert_eq!(
        client.reviews(&missing, pr(99)).await.unwrap_err(),
        Error::NotFound
    );
    assert_eq!(
        client.comments(&missing, pr(99)).await.unwrap_err(),
        Error::NotFound
    );
    assert_eq!(
        client.pull_files(&missing, pr(99)).await.unwrap_err(),
        Error::NotFound
    );
    let nowhere = repo("alice/nowhere");
    assert_eq!(
        client.pull(&nowhere, pr(1)).await.unwrap_err(),
        Error::NotFound
    );
    let comment = body("hello");
    assert_eq!(
        client
            .post_comment(&nowhere, pr(1), &comment)
            .await
            .unwrap_err(),
        Error::NotFound
    );
}

#[tokio::test]
async fn reviews_ignore_pending_and_decode_the_demo_reviews_in_order() {
    let (fake, client) = start(|_| {}).await;
    // The fake serves the Pending review of alice (4 entries), the client keeps 3.
    let reviews = client
        .reviews(&repo("alice/notes"), pr(1))
        .await
        .expect("reviews");
    assert_eq!(
        fake.with_state(|s| s.pull_mut("alice", "notes", 1).expect("p").reviews.len()),
        4
    );
    let ids: Vec<u64> = reviews.iter().map(|r| r.id).collect();
    assert_eq!(ids, [101, 102, 103]);
    assert_eq!(reviews[0].author, "carol");
    assert_eq!(reviews[0].state, ReviewState::Approved);
    assert_eq!(reviews[0].body, None, "an empty body is None");
    assert_eq!(reviews[1].state, ReviewState::ChangesRequested);
    assert_eq!(
        reviews[1].body.as_deref(),
        Some("Please also fix the title.")
    );
    assert_eq!(reviews[2].state, ReviewState::Commented);
    assert_eq!(
        reviews[0].submitted_at.as_ref().map(|t| t.as_str()),
        Some("2026-03-01T10:00:00Z")
    );
}

#[tokio::test]
async fn reviews_decode_dismissed_and_a_null_user() {
    let (_fake, client) = start(|s| {
        let p = s.pull_mut("alice", "notes", 1).expect("pull");
        p.reviews[0].state = FakeReviewState::Dismissed;
        p.reviews[1].author = None;
    })
    .await;
    let reviews = client
        .reviews(&repo("alice/notes"), pr(1))
        .await
        .expect("reviews");
    assert_eq!(reviews[0].state, ReviewState::Dismissed);
    assert_eq!(reviews[1].author, "ghost");
}

#[tokio::test]
async fn comments_decode_and_come_back_in_id_order() {
    let (_fake, client) = start(|s| {
        // Stored out of order: the fake serves them sorted by id, the client keeps that order.
        let p = s.pull_mut("alice", "notes", 1).expect("pull");
        p.comments.reverse();
        p.comments[0].author = None;
    })
    .await;
    let comments = client
        .comments(&repo("alice/notes"), pr(1))
        .await
        .expect("comments");
    assert_eq!(
        comments.iter().map(|c| c.id).collect::<Vec<_>>(),
        [201, 202]
    );
    assert_eq!(comments[0].author, "carol");
    assert_eq!(comments[0].body, "Thanks, good catch.");
    assert_eq!(comments[0].created_at.as_str(), "2026-03-02T09:00:00Z");
    assert_eq!(comments[1].author, "ghost");
}

#[tokio::test]
async fn pull_files_decode_every_status_patch_and_previous_name() {
    let (_fake, client) = start(|_| {}).await;
    let files = client
        .pull_files(&repo("alice/notes"), pr(1))
        .await
        .expect("files");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "README.md",
            "docs/guide.md",
            "scripts/export.sh",
            "docs/tags.md",
            "assets/logo.png"
        ]
    );
    assert_eq!(files[0].status, FileStatus::Modified);
    assert_eq!((files[0].additions, files[0].deletions), (1, 1));
    assert!(
        files[0]
            .patch
            .as_deref()
            .expect("patch")
            .starts_with("@@ -1,3 +1,3 @@")
    );
    assert_eq!(files[1].status, FileStatus::Added);
    assert_eq!(files[2].status, FileStatus::Removed);
    assert_eq!(files[3].status, FileStatus::Renamed);
    assert_eq!(files[3].previous_path.as_deref(), Some("docs/labels.md"));
    assert_eq!(files[3].patch, None, "a rename without change has no patch");
    assert_eq!(files[4].patch, None, "a binary has no patch");
    assert_eq!(files[0].previous_path, None);
}

#[tokio::test]
async fn pull_files_decode_the_remaining_statuses() {
    let (_fake, client) = start(|s| {
        let p = s.pull_mut("alice", "notes", 2).expect("pull");
        for (name, status) in [
            ("a", gh_fake::FileStatus::Copied),
            ("b", gh_fake::FileStatus::Changed),
            ("c", gh_fake::FileStatus::Unchanged),
        ] {
            p.files.push(PullFile {
                filename: name.into(),
                previous_filename: None,
                status,
                additions: 0,
                deletions: 0,
                patch: None,
            });
        }
    })
    .await;
    let files = client
        .pull_files(&repo("alice/notes"), pr(2))
        .await
        .expect("files");
    let statuses: Vec<FileStatus> = files.iter().map(|f| f.status).collect();
    assert_eq!(
        statuses,
        [
            FileStatus::Copied,
            FileStatus::Changed,
            FileStatus::Unchanged
        ]
    );
}

#[tokio::test]
async fn the_three_lists_follow_every_real_page_in_order() {
    let (fake, client) = start(|s| {
        s.max_per_page = 2;
        let p = s.pull_mut("alice", "notes", 2).expect("pull");
        for i in 0..5_u64 {
            p.reviews.push(PullReview {
                id: 500 + i,
                author: Some("carol".into()),
                state: FakeReviewState::Commented,
                body: format!("r{i}"),
                submitted_at: Some("2026-03-05T10:00:00Z".into()),
                commit_id: format!("{:040x}", 2),
            });
            p.comments.push(IssueComment {
                id: 600 + i,
                author: Some("bob".into()),
                body: format!("c{i}"),
                created_at: "2026-03-05T10:00:00Z".into(),
            });
            p.files.push(PullFile::modified(
                &format!("f{i}.txt"),
                "@@ -1 +1 @@\n-a\n+b\n",
            ));
        }
    })
    .await;
    let target = repo("alice/notes");
    let reviews = client.reviews(&target, pr(2)).await.expect("reviews");
    let comments = client.comments(&target, pr(2)).await.expect("comments");
    let files = client.pull_files(&target, pr(2)).await.expect("files");

    assert_eq!(
        reviews.iter().map(|r| r.id).collect::<Vec<_>>(),
        [500, 501, 502, 503, 504]
    );
    assert_eq!(
        comments.iter().map(|c| c.id).collect::<Vec<_>>(),
        [600, 601, 602, 603, 604]
    );
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["f0.txt", "f1.txt", "f2.txt", "f3.txt", "f4.txt"]
    );
    // 5 items at 2 per page: 3 real requests per route, the first page then the fake's own next links.
    for suffix in ["/pulls/2/reviews", "/issues/2/comments", "/pulls/2/files"] {
        let queries: Vec<String> = gets(&fake, suffix)
            .into_iter()
            .map(|r| r.query.unwrap_or_default())
            .collect();
        assert_eq!(queries.len(), 3, "{suffix}: {queries:?}");
        // The client asks for 100 per page; the fake caps it at 2 and its next links say so.
        assert!(queries[0].contains("per_page=100"), "{suffix}: {queries:?}");
        assert!(queries[1].contains("page=2"), "{suffix}: {queries:?}");
        assert!(queries[2].contains("page=3"), "{suffix}: {queries:?}");
    }
}

#[tokio::test]
async fn a_comment_is_published_then_seen_by_the_next_get() {
    let (fake, client) = start(|s| {
        s.now = "2026-05-06T07:08:09Z".into();
        s.next_id = 7_000;
    })
    .await;
    let target = repo("alice/notes");
    let before = client.comments(&target, pr(1)).await.expect("before");
    let created = client
        .post_comment(&target, pr(1), &body("  Looks good to me.\n"))
        .await
        .expect("post");
    assert_eq!(created.id, 7_000);
    assert_eq!(created.author, "alice");
    assert_eq!(created.body, "  Looks good to me.\n", "sent as is");
    assert_eq!(created.created_at.as_str(), "2026-05-06T07:08:09Z");

    let after = client.comments(&target, pr(1)).await.expect("after");
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.last(), Some(&created));
    let detail = client.pull(&target, pr(1)).await.expect("detail");
    assert_eq!(detail.updated_at.as_str(), "2026-05-06T07:08:09Z");

    let sent = posts(&fake);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].path, "/repos/alice/notes/issues/1/comments");
    assert_eq!(
        sent[0].body,
        Some(serde_json::json!({ "body": "  Looks good to me.\n" }))
    );
    assert_eq!(sent[0].token.as_deref(), Some(SECRET));
}

#[tokio::test]
async fn a_comment_is_accepted_on_a_closed_pr_as_on_github() {
    let (_fake, client) = start(|_| {}).await;
    let created = client
        .post_comment(&repo("alice/notes"), pr(3), &body("late remark"))
        .await
        .expect("closed PR accepts comments");
    assert_eq!(created.body, "late remark");
}

#[tokio::test]
async fn approve_and_request_changes_are_published_and_seen_by_the_next_get() {
    let (fake, client) = start(|s| s.now = "2026-05-06T07:08:09Z".into()).await;
    let target = repo("alice/notes");
    let sha = head_sha(&fake, "alice/notes", 2);

    let approved = client
        .post_review(&target, pr(2), &sha, &ReviewRequest::Approve { body: None })
        .await
        .expect("approve");
    assert_eq!(approved.state, ReviewState::Approved);
    assert_eq!(approved.author, "alice");
    assert_eq!(approved.body, None);
    assert_eq!(
        approved.submitted_at.as_ref().map(|t| t.as_str()),
        Some("2026-05-06T07:08:09Z")
    );

    let changes = client
        .post_review(
            &target,
            pr(2),
            &sha,
            &ReviewRequest::RequestChanges {
                body: body("Needs tests"),
            },
        )
        .await
        .expect("request changes");
    assert_eq!(changes.state, ReviewState::ChangesRequested);
    assert_eq!(changes.body.as_deref(), Some("Needs tests"));
    assert_eq!(changes.id, approved.id + 1);

    let seen = client.reviews(&target, pr(2)).await.expect("reviews");
    assert_eq!(seen, [approved, changes]);
    let detail = client.pull(&target, pr(2)).await.expect("detail");
    assert_eq!(
        detail.updated_at.as_str(),
        "2026-05-06T07:08:09Z",
        "a submitted review touches the PR"
    );

    let sent = posts(&fake);
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[0].body,
        Some(serde_json::json!({ "event": "APPROVE", "commit_id": sha.as_str() }))
    );
    assert_eq!(
        sent[1].body,
        Some(serde_json::json!({
            "event": "REQUEST_CHANGES", "body": "Needs tests", "commit_id": sha.as_str()
        }))
    );
    // The fake stored the commit the user saw.
    let stored = fake.with_state(|s| s.pull_mut("alice", "notes", 2).expect("p").reviews.clone());
    assert!(stored.iter().all(|r| r.commit_id == sha.as_str()));
}

#[tokio::test]
async fn an_approval_with_text_keeps_the_text() {
    let (fake, client) = start(|_| {}).await;
    let sha = head_sha(&fake, "alice/notes", 2);
    let review = client
        .post_review(
            &repo("alice/notes"),
            pr(2),
            &sha,
            &ReviewRequest::Approve {
                body: Some(body("LGTM")),
            },
        )
        .await
        .expect("approve");
    assert_eq!(review.body.as_deref(), Some("LGTM"));
}

#[tokio::test]
async fn a_pending_review_of_the_user_is_never_returned_even_after_publishing_elsewhere() {
    // alice has a Pending review on PR 1 of alice/notes: reviews() hides it and a new Review is refused (422).
    let (_fake, client) = start(|_| {}).await;
    let target = repo("alice/notes");
    let listed = client.reviews(&target, pr(1)).await.expect("reviews");
    assert!(listed.iter().all(|r| r.author != "alice"), "{listed:?}");
    let sha = CommitSha::parse(&format!("{:040x}", 1)).expect("sha");
    let message = unprocessable(
        client
            .post_review(&target, pr(1), &sha, &ReviewRequest::Approve { body: None })
            .await,
    );
    assert!(message.contains("one pending review"), "{message}");
}

#[tokio::test]
async fn publishing_on_an_archived_repo_is_forbidden_with_githubs_message() {
    let (fake, client) = start(|_| {}).await;
    let target = repo("acme/legacy-site");
    let detail = client.pull(&target, pr(1)).await.expect("detail");
    assert!(detail.archived);

    let comment = client.post_comment(&target, pr(1), &body("hi")).await;
    let review = client
        .post_review(
            &target,
            pr(1),
            &detail.head_sha,
            &ReviewRequest::Approve { body: None },
        )
        .await;
    for result in [comment.map(|_| ()), review.map(|_| ())] {
        match result {
            Err(error @ Error::Forbidden { .. }) => {
                assert_no_secret(&error);
                let Error::Forbidden { message } = error else {
                    unreachable!()
                };
                assert_eq!(message, "Repository was archived so is read-only.");
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }
    // Nothing was added.
    fake.with_state(|s| {
        let p = s.pull_mut("acme", "legacy-site", 1).expect("pull");
        assert!(p.comments.is_empty() && p.reviews.is_empty());
    });
}

#[tokio::test]
async fn own_pr_is_refused_for_both_verdicts_with_distinct_messages() {
    let (fake, client) = start(|_| {}).await;
    let target = repo("bob/shared-tools");
    let sha = head_sha(&fake, "bob/shared-tools", 1);

    let approve = unprocessable(
        client
            .post_review(&target, pr(1), &sha, &ReviewRequest::Approve { body: None })
            .await,
    );
    assert!(
        approve.contains("Can not approve your own pull request"),
        "{approve}"
    );
    let changes = unprocessable(
        client
            .post_review(
                &target,
                pr(1),
                &sha,
                &ReviewRequest::RequestChanges { body: body("no") },
            )
            .await,
    );
    assert!(
        changes.contains("Can not request changes on your own pull request"),
        "{changes}"
    );
    fake.with_state(|s| {
        assert!(
            s.pull_mut("bob", "shared-tools", 1)
                .expect("p")
                .reviews
                .is_empty()
        );
    });
}

#[tokio::test]
async fn a_review_on_a_closed_or_merged_pr_is_refused() {
    let (fake, client) = start(|_| {}).await;
    let target = repo("alice/notes");
    for number in [3, 4] {
        let sha = head_sha(&fake, "alice/notes", number);
        let message = unprocessable(
            client
                .post_review(
                    &target,
                    pr(number),
                    &sha,
                    &ReviewRequest::Approve { body: None },
                )
                .await,
        );
        assert!(message.contains("not open"), "#{number}: {message}");
    }
    let still_closed = fake.with_state(|s| s.pull_mut("alice", "notes", 3).expect("p").state);
    assert_eq!(still_closed, PullState::Closed);
}

#[tokio::test]
async fn blank_texts_are_refused_before_any_request() {
    // The client cannot build a blank body, so the fake's "body required" 422 is never reached through it.
    for text in ["", "   \n\t"] {
        assert_eq!(
            MessageBody::new(text).unwrap_err(),
            Error::InvalidBody {
                problem: BodyProblem::Blank
            }
        );
    }
    let (fake, _client) = start(|_| {}).await;
    assert!(posts(&fake).is_empty());
}

#[tokio::test]
async fn an_injected_string_form_422_is_classified_with_its_message() {
    // The object-form 422 of the fake's own validation (blank body) cannot be reached through the client, which
    // refuses a blank body before sending; the string form is what the client can see.
    let (fake, client) = start(|_| {}).await;
    fake.with_state(|s| {
        s.failures.push(gh_fake::Failure {
            method: Method::Post,
            path: "/repos/alice/notes/issues/1/comments".into(),
            response: gh_fake::FailureResponse::Unprocessable {
                message: "Body is too short".into(),
            },
            remaining: Some(1),
        });
    });
    let message = unprocessable(
        client
            .post_comment(&repo("alice/notes"), pr(1), &body("x"))
            .await,
    );
    assert!(message.contains("Body is too short"), "{message}");
}

#[tokio::test]
async fn every_error_of_the_seven_routes_hides_the_token() {
    let (fake, client) = start(|_| {}).await;
    let target = repo("alice/notes");
    let sha = head_sha(&fake, "alice/notes", 2);
    let other_token = Client::new(
        ApiBase::parse(&fake.base_url()).expect("base"),
        Token::new("ghp_WrongTokenValue999").expect("token"),
    )
    .expect("client");
    // Wrong token: 401 on every route, none of the two tokens appears.
    let errors = [
        other_token.current_user().await.map(|_| ()),
        other_token.pull(&target, pr(1)).await.map(|_| ()),
        other_token.reviews(&target, pr(1)).await.map(|_| ()),
        other_token.comments(&target, pr(1)).await.map(|_| ()),
        other_token.pull_files(&target, pr(1)).await.map(|_| ()),
        other_token
            .post_comment(&target, pr(1), &body("x"))
            .await
            .map(|_| ()),
        other_token
            .post_review(&target, pr(2), &sha, &ReviewRequest::Approve { body: None })
            .await
            .map(|_| ()),
    ];
    for result in errors {
        let error = result.expect_err("401");
        assert_eq!(error, Error::Unauthorized);
        assert_no_secret(&error);
        assert!(!format!("{error:?}").contains("WrongTokenValue"));
    }
    assert!(fake.with_state(|s| {
        s.pull_mut("alice", "notes", 2)
            .expect("p")
            .reviews
            .is_empty()
    }));
    let _ = client;
}
