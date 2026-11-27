//! Routes of the fake server, driven over real HTTP with reqwest (ticket #2).

use std::time::{Duration, Instant};

use gh_fake::{
    Access, DEFAULT_TOKEN, Failure, FailureResponse, FakeGitHub, Method, Pull, PullState, Repo,
    State, User,
};
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;

fn user() -> User {
    User {
        login: "alice".into(),
        id: 1,
    }
}

async fn start(repos: Vec<Repo>) -> FakeGitHub {
    let mut state = State::new(user());
    state.repos = repos;
    FakeGitHub::start(state).await.expect("start fake")
}

/// Builds a client; reqwest (`rustls-no-provider`) needs a process-wide crypto provider even for plain HTTP.
fn client() -> Client {
    static PROVIDER: std::sync::Once = std::sync::Once::new();
    PROVIDER.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("install ring provider");
    });
    Client::new()
}

async fn get(fake: &FakeGitHub, path: &str, token: Option<&str>) -> Response {
    let mut req = client().get(format!("{}{path}", fake.base_url()));
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    req.send().await.expect("send")
}

async fn full_names(resp: Response) -> Vec<String> {
    let json: Value = resp.json().await.expect("json");
    json.as_array()
        .expect("array")
        .iter()
        .map(|r| r["full_name"].as_str().expect("full_name").to_owned())
        .collect()
}

fn header(resp: &Response, name: &str) -> String {
    resp.headers()
        .get(name)
        .unwrap_or_else(|| panic!("missing header {name}"))
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn no_token_is_401_requires_authentication() {
    let fake = start(vec![]).await;
    let resp = get(&fake, "/user/repos", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Requires authentication");
    assert!(json["documentation_url"].is_string());
}

#[tokio::test]
async fn wrong_token_is_401_bad_credentials() {
    let fake = start(vec![]).await;
    let resp = get(&fake, "/user/repos", Some("nope")).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Bad credentials");
}

#[tokio::test]
async fn legacy_token_scheme_is_accepted() {
    let fake = start(vec![]).await;
    let resp = client()
        .get(format!("{}/user/repos", fake.base_url()))
        .header("Authorization", format!("token {DEFAULT_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn repos_are_sorted_case_insensitively_by_owner_then_name() {
    let fake = start(vec![
        Repo::new("bob", "zeta"),
        Repo::new("alice", "beta"),
        Repo::new("Alice", "Alpha"),
        Repo::new("alice", "alpha2"),
    ])
    .await;
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        full_names(resp).await,
        ["Alice/Alpha", "alice/alpha2", "alice/beta", "bob/zeta"]
    );
}

#[tokio::test]
async fn repo_json_has_github_shape() {
    let mut r = Repo::new("alice", "notes");
    r.description = Some("hello".into());
    r.private = true;
    r.fork = true;
    r.archived = true;
    let fake = start(vec![r, Repo::new("alice", "plain")]).await;
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json[0]["name"], "notes");
    assert_eq!(json[0]["full_name"], "alice/notes");
    assert_eq!(json[0]["owner"]["login"], "alice");
    assert_eq!(json[0]["description"], "hello");
    assert_eq!(json[0]["private"], true);
    assert_eq!(json[0]["fork"], true);
    assert_eq!(json[0]["archived"], true);
    assert_eq!(json[1]["description"], Value::Null);
    assert_eq!(json[1]["private"], false);
}

#[tokio::test]
async fn affiliation_filters_by_access() {
    let mut collab = Repo::new("bob", "shared");
    collab.access = Access::Collaborator;
    let mut org = Repo::new("acme", "tool");
    org.access = Access::OrganizationMember;
    let fake = start(vec![Repo::new("alice", "mine"), collab, org]).await;

    let all = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(
        full_names(all).await,
        ["acme/tool", "alice/mine", "bob/shared"]
    );

    let owner = get(&fake, "/user/repos?affiliation=owner", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(owner).await, ["alice/mine"]);

    let two = get(
        &fake,
        "/user/repos?affiliation=collaborator,organization_member",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(full_names(two).await, ["acme/tool", "bob/shared"]);
}

fn five_repos() -> Vec<Repo> {
    ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| Repo::new("alice", n))
        .collect()
}

#[tokio::test]
async fn pagination_uses_absolute_link_header() {
    let fake = start(five_repos()).await;
    fake.with_state(|s| s.max_per_page = 2);
    let base = fake.base_url();

    let p1 = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    let link = header(&p1, "link");
    assert_eq!(full_names(p1).await, ["alice/a", "alice/b"]);
    assert!(link.contains(&format!("<{base}/user/repos?")), "{link}");
    assert!(link.contains("page=2"), "{link}");
    assert!(link.contains("rel=\"next\""), "{link}");
    assert!(link.contains("page=3"), "{link}");
    assert!(link.contains("rel=\"last\""), "{link}");

    let p3 = get(&fake, "/user/repos?page=3", Some(DEFAULT_TOKEN)).await;
    assert!(p3.headers().get("link").is_none() || !header(&p3, "link").contains("rel=\"next\""));
    assert_eq!(full_names(p3).await, ["alice/e"]);

    let p2 = get(&fake, "/user/repos?page=2", Some(DEFAULT_TOKEN)).await;
    assert!(header(&p2, "link").contains("rel=\"next\""));
    assert_eq!(full_names(p2).await, ["alice/c", "alice/d"]);
}

#[tokio::test]
async fn affiliation_with_control_characters_is_encoded_in_the_link_header() {
    let fake = start(five_repos()).await;
    fake.with_state(|s| s.max_per_page = 2);

    let resp = get(
        &fake,
        "/user/repos?affiliation=owner,%0A%C3%A9",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let link = header(&resp, "link");
    assert!(link.contains("affiliation=owner,%0A%C3%A9&"), "{link}");
    assert!(link.is_ascii() && !link.contains('\n'), "{link:?}");
}

#[tokio::test]
async fn per_page_default_is_30_and_capped_by_max_per_page() {
    let repos: Vec<Repo> = (0..40)
        .map(|i| Repo::new("alice", &format!("r{i:02}")))
        .collect();
    let fake = start(repos).await;
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(resp).await.len(), 30);
    let resp = get(&fake, "/user/repos?per_page=10", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(resp).await.len(), 10);
    let resp = get(&fake, "/user/repos?per_page=1000", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(resp).await.len(), 40);
    fake.with_state(|s| s.max_per_page = 5);
    let resp = get(&fake, "/user/repos?per_page=10", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(resp).await.len(), 5);
}

#[tokio::test]
async fn page_beyond_the_end_is_empty() {
    let fake = start(five_repos()).await;
    let resp = get(&fake, "/user/repos?page=9", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(full_names(resp).await.is_empty());
}

#[tokio::test]
async fn latency_delays_every_request() {
    let fake = start(vec![]).await;
    fake.with_state(|s| s.latency = Duration::from_millis(300));
    let t = Instant::now();
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        t.elapsed() >= Duration::from_millis(300),
        "{:?}",
        t.elapsed()
    );
}

#[tokio::test]
async fn rate_limited_failure_is_403_with_remaining_zero() {
    let fake = start(vec![]).await;
    fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Get,
            path: "/user/repos".into(),
            response: FailureResponse::RateLimited,
            remaining: None,
        })
    });
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(header(&resp, "x-ratelimit-remaining"), "0");
    assert!(header(&resp, "x-ratelimit-reset").parse::<u64>().unwrap() > 0);
    let json: Value = resp.json().await.unwrap();
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .starts_with("API rate limit exceeded")
    );
}

#[tokio::test]
async fn normal_answers_carry_ratelimit_headers() {
    let fake = start(vec![]).await;
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert!(header(&resp, "x-ratelimit-limit").parse::<u32>().unwrap() > 0);
    assert!(
        header(&resp, "x-ratelimit-remaining")
            .parse::<u32>()
            .unwrap()
            > 0
    );
    assert!(header(&resp, "x-ratelimit-reset").parse::<u64>().is_ok());
}

#[tokio::test]
async fn failure_remaining_is_decremented_then_removed() {
    let fake = start(vec![Repo::new("alice", "a")]).await;
    fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Get,
            path: "/user/repos".into(),
            response: FailureResponse::ServerError,
            remaining: Some(2),
        })
    });
    assert_eq!(
        get(&fake, "/user/repos", Some(DEFAULT_TOKEN))
            .await
            .status(),
        500
    );
    assert_eq!(fake.with_state(|s| s.failures[0].remaining), Some(1));
    assert_eq!(
        get(&fake, "/user/repos", Some(DEFAULT_TOKEN))
            .await
            .status(),
        500
    );
    assert!(fake.with_state(|s| s.failures.is_empty()));
    assert_eq!(
        get(&fake, "/user/repos", Some(DEFAULT_TOKEN))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn failures_do_not_apply_without_authentication_and_other_variants_have_github_shape() {
    let fake = start(vec![]).await;
    let cases = [
        (FailureResponse::Unauthorized, 401, "Bad credentials"),
        (
            FailureResponse::Forbidden {
                message: "Nope".into(),
            },
            403,
            "Nope",
        ),
        (FailureResponse::NotFound, 404, "Not Found"),
        (
            FailureResponse::Unprocessable {
                message: "Bad".into(),
            },
            422,
            "Bad",
        ),
    ];
    for (response, status, message) in cases {
        fake.with_state(|s| {
            s.failures = vec![Failure {
                method: Method::Get,
                path: "/user/repos".into(),
                response,
                remaining: None,
            }]
        });
        // Authentication comes first: no token, no injected failure.
        assert_eq!(get(&fake, "/user/repos", None).await.status(), 401);
        let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
        assert_eq!(resp.status(), status);
        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["message"], message);
        if status == 422 {
            assert_eq!(json["errors"][0], message);
        }
    }
}

#[tokio::test]
async fn unknown_route_is_404_not_found() {
    let fake = start(vec![]).await;
    let resp = get(&fake, "/nope", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Not Found");
}

#[tokio::test]
async fn unsupported_method_is_404_and_not_recorded() {
    let fake = start(vec![]).await;
    let resp = client()
        .delete(format!("{}/user/repos", fake.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn requests_and_token_are_recorded_including_refused_ones() {
    let fake = start(vec![]).await;
    get(&fake, "/user/repos?per_page=5", Some(DEFAULT_TOKEN)).await;
    get(&fake, "/user/repos", Some("bad")).await;
    get(&fake, "/missing", None).await;
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[0].method, Method::Get);
    assert_eq!(reqs[0].path, "/user/repos");
    assert_eq!(reqs[0].query.as_deref(), Some("per_page=5"));
    assert_eq!(reqs[0].token.as_deref(), Some(DEFAULT_TOKEN));
    assert_eq!(reqs[1].token.as_deref(), Some("bad"));
    assert_eq!(reqs[1].query, None);
    assert_eq!(reqs[2].token, None);
    assert_eq!(reqs[2].path, "/missing");
}

#[tokio::test]
async fn post_body_is_recorded_when_valid_json() {
    let fake = start(vec![]).await;
    client()
        .post(format!("{}/anything", fake.base_url()))
        .header("Authorization", format!("Bearer {DEFAULT_TOKEN}"))
        .json(&serde_json::json!({"a": 1}))
        .send()
        .await
        .unwrap();
    let reqs = fake.requests();
    assert_eq!(reqs[0].method, Method::Post);
    assert_eq!(reqs[0].body, Some(serde_json::json!({"a": 1})));
}

#[tokio::test]
async fn demo_state_serves_repos_with_fork_archived_and_private() {
    let fake = FakeGitHub::start(State::demo()).await.unwrap();
    let resp = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    let json: Value = resp.json().await.unwrap();
    let repos = json.as_array().unwrap();
    assert!(repos.len() >= 3);
    assert!(repos.iter().any(|r| r["fork"] == true));
    assert!(repos.iter().any(|r| r["archived"] == true));
    assert!(
        repos
            .iter()
            .any(|r| r["private"] == true && r["description"].is_string())
    );
    // Collaborator and organization repos of other owners are listed too (#3).
    assert!(repos.iter().any(|r| r["owner"]["login"] != "alice"));
}

#[tokio::test]
async fn base_url_is_loopback_without_trailing_slash() {
    let fake = start(vec![]).await;
    assert!(fake.addr().ip().is_loopback());
    assert!(fake.base_url().starts_with("http://127.0.0.1:"));
    assert!(!fake.base_url().ends_with('/'));
}

// ---- #4 / #5: GET /repos/{owner}/{repo} and GET /repos/{owner}/{repo}/pulls ----

fn pull(repo: &str, number: u64, state: PullState, created: &str, updated: &str) -> Pull {
    let mut p = Pull::new("alice", repo, number, &format!("PR {number}"), "bob");
    p.state = state;
    p.created_at = created.to_owned();
    p.updated_at = updated.to_owned();
    p
}

async fn numbers(resp: Response) -> Vec<u64> {
    let json: Value = resp.json().await.expect("json");
    json.as_array()
        .expect("array")
        .iter()
        .map(|p| p["number"].as_u64().expect("number"))
        .collect()
}

/// A fake with `alice/notes` and PRs 1 (open), 2 (closed), 3 (merged), 4 (open), plus a PR of another repo.
async fn with_pulls() -> FakeGitHub {
    let fake = start(vec![
        Repo::new("alice", "notes"),
        Repo::new("alice", "other"),
    ])
    .await;
    fake.with_state(|s| {
        s.pulls = vec![
            pull(
                "notes",
                1,
                PullState::Open,
                "2026-01-01T00:00:00Z",
                "2026-03-01T00:00:00Z",
            ),
            pull(
                "notes",
                2,
                PullState::Closed,
                "2026-01-02T00:00:00Z",
                "2026-02-01T00:00:00Z",
            ),
            pull(
                "notes",
                3,
                PullState::Merged,
                "2026-01-03T00:00:00Z",
                "2026-04-01T00:00:00Z",
            ),
            pull(
                "notes",
                4,
                PullState::Open,
                "2026-01-04T00:00:00Z",
                "2026-01-15T00:00:00Z",
            ),
            pull(
                "other",
                9,
                PullState::Open,
                "2026-01-09T00:00:00Z",
                "2026-01-09T00:00:00Z",
            ),
        ];
    });
    fake
}

#[tokio::test]
async fn repo_is_found_with_canonical_name_whatever_the_path_case() {
    let fake = start(vec![Repo::new("Alice", "Notes")]).await;
    let resp = get(&fake, "/repos/alice/notes", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["full_name"], "Alice/Notes");
    assert_eq!(json["name"], "Notes");
    assert_eq!(json["owner"]["login"], "Alice");
    let resp = get(&fake, "/repos/ALICE/NOTES", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn no_affiliation_repo_is_served_by_path_but_never_listed() {
    let mut other = Repo::new("carol", "public-lib");
    other.access = Access::NoAffiliation;
    let fake = start(vec![Repo::new("alice", "mine"), other]).await;
    let resp = get(&fake, "/repos/carol/public-lib", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["full_name"], "carol/public-lib");
    let listed = get(&fake, "/user/repos", Some(DEFAULT_TOKEN)).await;
    assert_eq!(full_names(listed).await, ["alice/mine"]);
    let all = get(
        &fake,
        "/user/repos?affiliation=owner,collaborator,organization_member",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(full_names(all).await, ["alice/mine"]);
}

#[tokio::test]
async fn unknown_repo_is_404_not_found_on_both_routes() {
    let fake = start(vec![Repo::new("alice", "notes")]).await;
    for path in [
        "/repos/alice/nope",
        "/repos/alice/nope/pulls",
        "/repos/zed/notes/pulls",
    ] {
        let resp = get(&fake, path, Some(DEFAULT_TOKEN)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["message"], "Not Found", "{path}");
        assert!(json["documentation_url"].is_string());
    }
}

#[tokio::test]
async fn new_routes_require_authentication() {
    let fake = with_pulls().await;
    assert_eq!(get(&fake, "/repos/alice/notes", None).await.status(), 401);
    assert_eq!(
        get(&fake, "/repos/alice/notes/pulls", Some("bad"))
            .await
            .status(),
        401
    );
}

#[tokio::test]
async fn pulls_default_to_open_only_of_that_repo() {
    let fake = with_pulls().await;
    let resp = get(&fake, "/repos/alice/notes/pulls", Some(DEFAULT_TOKEN)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // created desc by default: 4 (Jan 4) then 1 (Jan 1).
    assert_eq!(numbers(resp).await, [4, 1]);
    let open = get(
        &fake,
        "/repos/alice/notes/pulls?state=open",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(numbers(open).await, [4, 1]);
}

#[tokio::test]
async fn pulls_state_closed_includes_merged_and_all_includes_everything() {
    let fake = with_pulls().await;
    let closed = get(
        &fake,
        "/repos/alice/notes/pulls?state=closed",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(numbers(closed).await, [3, 2]);
    let all = get(
        &fake,
        "/repos/alice/notes/pulls?state=all",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(numbers(all).await, [4, 3, 2, 1]);
}

#[tokio::test]
async fn pulls_repo_path_is_case_insensitive() {
    let fake = with_pulls().await;
    let resp = get(&fake, "/repos/ALICE/Notes/pulls", Some(DEFAULT_TOKEN)).await;
    assert_eq!(numbers(resp).await, [4, 1]);
}

#[tokio::test]
async fn pulls_unknown_state_is_422_validation_failed() {
    let fake = with_pulls().await;
    for q in ["state=bogus", "state=", "state=OPEN"] {
        let resp = get(
            &fake,
            &format!("/repos/alice/notes/pulls?{q}"),
            Some(DEFAULT_TOKEN),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{q}");
        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["message"], "Validation Failed", "{q}");
        assert!(json["documentation_url"].is_string());
    }
}

#[tokio::test]
async fn pulls_sort_by_created_or_updated_asc_or_desc() {
    let fake = with_pulls().await;
    let cases = [
        ("sort=created&direction=desc", vec![4, 3, 2, 1]),
        ("sort=created&direction=asc", vec![1, 2, 3, 4]),
        ("sort=updated&direction=desc", vec![3, 1, 2, 4]),
        ("sort=updated&direction=asc", vec![4, 2, 1, 3]),
        ("sort=updated", vec![3, 1, 2, 4]),
        ("direction=asc", vec![1, 2, 3, 4]),
    ];
    for (q, expected) in cases {
        let resp = get(
            &fake,
            &format!("/repos/alice/notes/pulls?state=all&{q}"),
            Some(DEFAULT_TOKEN),
        )
        .await;
        assert_eq!(numbers(resp).await, expected, "{q}");
    }
}

#[tokio::test]
async fn pulls_ties_are_broken_by_number_descending_in_both_directions() {
    let fake = start(vec![Repo::new("alice", "notes")]).await;
    fake.with_state(|s| {
        s.pulls = (1..=3)
            .map(|n| {
                pull(
                    "notes",
                    n,
                    PullState::Open,
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                )
            })
            .collect();
    });
    for dir in ["asc", "desc"] {
        let resp = get(
            &fake,
            &format!("/repos/alice/notes/pulls?direction={dir}"),
            Some(DEFAULT_TOKEN),
        )
        .await;
        assert_eq!(numbers(resp).await, [3, 2, 1], "{dir}");
    }
}

#[tokio::test]
async fn pulls_are_paginated_with_link_keeping_the_query() {
    let fake = start(vec![Repo::new("alice", "notes")]).await;
    fake.with_state(|s| {
        s.max_per_page = 2;
        s.pulls = (1..=5)
            .map(|n| {
                pull(
                    "notes",
                    n,
                    PullState::Open,
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                )
            })
            .collect();
    });
    let base = fake.base_url();
    let p1 = get(
        &fake,
        "/repos/alice/notes/pulls?state=all&sort=updated&direction=asc",
        Some(DEFAULT_TOKEN),
    )
    .await;
    let link = header(&p1, "link");
    assert_eq!(numbers(p1).await, [5, 4]);
    assert!(
        link.contains(&format!("<{base}/repos/alice/notes/pulls?")),
        "{link}"
    );
    assert!(link.contains("state=all"), "{link}");
    assert!(link.contains("sort=updated"), "{link}");
    assert!(link.contains("direction=asc"), "{link}");
    assert!(link.contains("page=2>; rel=\"next\""), "{link}");
    assert!(link.contains("page=3>; rel=\"last\""), "{link}");

    let p3 = get(
        &fake,
        "/repos/alice/notes/pulls?page=3",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert!(p3.headers().get("link").is_none());
    assert_eq!(numbers(p3).await, [1]);
    let beyond = get(
        &fake,
        "/repos/alice/notes/pulls?page=9",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(beyond.status(), StatusCode::OK);
    assert!(numbers(beyond).await.is_empty());
    let one = get(
        &fake,
        "/repos/alice/notes/pulls?per_page=1&page=2",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(numbers(one).await, [4]);
}

#[tokio::test]
async fn pulls_query_with_control_characters_is_encoded_in_the_link_header() {
    let fake = start(vec![Repo::new("alice", "notes")]).await;
    fake.with_state(|s| {
        s.max_per_page = 1;
        s.pulls = (1..=2)
            .map(|n| {
                pull(
                    "notes",
                    n,
                    PullState::Open,
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                )
            })
            .collect();
    });
    let resp = get(
        &fake,
        "/repos/alice/notes/pulls?sort=created,%0A%C3%A9",
        Some(DEFAULT_TOKEN),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let link = header(&resp, "link");
    assert!(link.contains("sort=created,%0A%C3%A9"), "{link}");
    assert!(link.is_ascii() && !link.contains('\n'), "{link:?}");
}

#[tokio::test]
async fn pull_json_has_github_shape() {
    let fake = start(vec![Repo::new("alice", "notes")]).await;
    fake.with_state(|s| {
        let mut open = pull(
            "notes",
            1,
            PullState::Open,
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        );
        open.draft = true;
        open.body = Some("text".into());
        let mut merged = pull(
            "notes",
            2,
            PullState::Merged,
            "2026-01-03T00:00:00Z",
            "2026-01-04T00:00:00Z",
        );
        merged.author = None;
        merged.head = "fix".into();
        let closed = pull(
            "notes",
            3,
            PullState::Closed,
            "2026-01-05T00:00:00Z",
            "2026-01-06T00:00:00Z",
        );
        s.pulls = vec![open, merged, closed];
    });
    let resp = get(
        &fake,
        "/repos/alice/notes/pulls?state=all&direction=asc",
        Some(DEFAULT_TOKEN),
    )
    .await;
    let json: Value = resp.json().await.unwrap();
    let (open, merged, closed) = (&json[0], &json[1], &json[2]);

    assert_eq!(open["number"], 1);
    assert_eq!(open["state"], "open");
    assert_eq!(open["title"], "PR 1");
    assert_eq!(open["draft"], true);
    assert_eq!(open["body"], "text");
    assert_eq!(open["user"]["login"], "bob");
    assert_eq!(open["user"]["type"], "User");
    assert!(open["user"]["id"].is_u64());
    assert!(open["id"].is_u64());
    assert_eq!(open["created_at"], "2026-01-01T00:00:00Z");
    assert_eq!(open["updated_at"], "2026-01-02T00:00:00Z");
    assert_eq!(open["closed_at"], Value::Null);
    assert_eq!(open["merged_at"], Value::Null);
    assert_eq!(open["head"]["ref"], "feature");
    assert_eq!(open["head"]["label"], "alice:feature");
    assert_eq!(open["base"]["ref"], "main");
    assert_eq!(open["base"]["label"], "alice:main");
    assert_eq!(open["html_url"], "https://github.com/alice/notes/pull/1");

    assert_eq!(merged["state"], "closed");
    assert_eq!(merged["user"], Value::Null);
    assert_eq!(merged["draft"], false);
    assert_eq!(merged["body"], Value::Null);
    assert_eq!(merged["merged_at"], "2026-01-04T00:00:00Z");
    assert_eq!(merged["closed_at"], "2026-01-04T00:00:00Z");
    assert_eq!(merged["head"]["ref"], "fix");

    assert_eq!(closed["state"], "closed");
    assert_eq!(closed["merged_at"], Value::Null);
    assert_eq!(closed["closed_at"], "2026-01-06T00:00:00Z");
}

#[tokio::test]
async fn injected_failures_apply_to_the_new_paths() {
    let fake = with_pulls().await;
    for path in ["/repos/alice/notes", "/repos/alice/notes/pulls"] {
        fake.with_state(|s| {
            s.failures = vec![Failure {
                method: Method::Get,
                path: path.into(),
                response: FailureResponse::ServerError,
                remaining: Some(1),
            }]
        });
        assert_eq!(
            get(&fake, path, Some(DEFAULT_TOKEN)).await.status(),
            500,
            "{path}"
        );
        assert_eq!(
            get(&fake, path, Some(DEFAULT_TOKEN)).await.status(),
            200,
            "{path}"
        );
    }
    // A failure on one path leaves the other alone.
    fake.with_state(|s| {
        s.failures = vec![Failure {
            method: Method::Get,
            path: "/repos/alice/notes/pulls".into(),
            response: FailureResponse::RateLimited,
            remaining: None,
        }]
    });
    assert_eq!(
        get(&fake, "/repos/alice/notes", Some(DEFAULT_TOKEN))
            .await
            .status(),
        200
    );
    assert_eq!(
        get(&fake, "/repos/alice/notes/pulls", Some(DEFAULT_TOKEN))
            .await
            .status(),
        403
    );
}

#[tokio::test]
async fn demo_state_has_the_data_the_later_tickets_need() {
    let fake = FakeGitHub::start(State::demo()).await.unwrap();
    let (collab, org, none) = fake.with_state(|s| {
        let count = |a: Access| {
            s.repos
                .iter()
                .filter(|r| r.access == a && r.owner != "alice")
                .count()
        };
        (
            count(Access::Collaborator),
            s.repos
                .iter()
                .filter(|r| r.access == Access::OrganizationMember)
                .collect::<Vec<_>>()
                .len(),
            count(Access::NoAffiliation),
        )
    });
    assert!(
        collab >= 1 && org >= 2 && none >= 1,
        "{collab} {org} {none}"
    );
    let org_archived = fake.with_state(|s| {
        s.repos
            .iter()
            .any(|r| r.access == Access::OrganizationMember && r.archived)
    });
    assert!(org_archived);
    let org_owner = fake.with_state(|s| {
        let owners: std::collections::HashSet<_> = s
            .repos
            .iter()
            .filter(|r| r.access == Access::OrganizationMember)
            .map(|r| r.owner.clone())
            .collect();
        owners.len()
    });
    assert_eq!(
        org_owner, 1,
        "both organization repos belong to one organization"
    );

    // The no-affiliation repo opens by path but is not listed.
    let path = fake.with_state(|s| {
        let r = s
            .repos
            .iter()
            .find(|r| r.access == Access::NoAffiliation)
            .unwrap();
        format!("/repos/{}/{}", r.owner, r.name)
    });
    assert_eq!(get(&fake, &path, Some(DEFAULT_TOKEN)).await.status(), 200);

    let all = get(
        &fake,
        "/repos/alice/notes/pulls?state=all&per_page=100",
        Some(DEFAULT_TOKEN),
    )
    .await;
    let json: Value = all.json().await.unwrap();
    let prs = json.as_array().unwrap();
    let open: Vec<_> = prs.iter().filter(|p| p["state"] == "open").collect();
    assert!(open.len() >= 2);
    assert!(open.iter().any(|p| p["draft"] == true));
    assert!(
        prs.iter()
            .any(|p| p["state"] == "closed" && p["merged_at"].is_null())
    );
    assert!(prs.iter().any(|p| !p["merged_at"].is_null()));
    let updated: std::collections::HashSet<_> = prs
        .iter()
        .map(|p| p["updated_at"].as_str().unwrap())
        .collect();
    assert_eq!(updated.len(), prs.len(), "distinct updated_at");
    let authors: std::collections::HashSet<_> = prs
        .iter()
        .filter_map(|p| p["user"]["login"].as_str())
        .collect();
    assert!(authors.len() >= 2);
}

// ---- ADR 0003 (#6-#10): detail, files, reviews, comments, and the two POST routes ----

use gh_fake::{FileStatus, IssueComment, PullFile, PullReview, ReviewState};
use serde_json::json;

const NOW: &str = "2026-05-05T05:05:05Z";
const SHA_OK: &str = "0123456789abcdef0123456789abcdef01234567";

async fn post(fake: &FakeGitHub, path: &str, body: &Value) -> Response {
    client()
        .post(format!("{}{path}", fake.base_url()))
        .header("Authorization", format!("Bearer {DEFAULT_TOKEN}"))
        .json(body)
        .send()
        .await
        .expect("send")
}

async fn authed(fake: &FakeGitHub, path: &str) -> Response {
    get(fake, path, Some(DEFAULT_TOKEN)).await
}

fn review(id: u64, author: Option<&str>, state: ReviewState, body: &str) -> PullReview {
    PullReview {
        id,
        author: author.map(str::to_owned),
        state,
        body: body.to_owned(),
        submitted_at: (state != ReviewState::Pending).then(|| "2026-04-01T08:00:00Z".to_owned()),
        commit_id: SHA_OK.to_owned(),
    }
}

/// alice/notes: #1 (bob, open, files, reviews, comments), #2 (alice, open), #3 (bob, closed); acme/frozen
/// (archived): #1 (bob, open); bob/tools: nothing. "now" and "next_id" are the ADR defaults except "now".
async fn adr_fake() -> FakeGitHub {
    let mut frozen = Repo::new("acme", "frozen");
    frozen.archived = true;
    let fake = start(vec![
        Repo::new("alice", "notes"),
        frozen,
        Repo::new("bob", "tools"),
    ])
    .await;
    fake.with_state(|s| {
        s.now = NOW.to_owned();
        let mut one = Pull::new("alice", "notes", 1, "One", "bob");
        one.head_sha = SHA_OK.to_owned();
        one.files = vec![
            PullFile::modified("src/a.rs", "@@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n"),
            PullFile {
                filename: "new.png".into(),
                previous_filename: None,
                status: FileStatus::Added,
                additions: 0,
                deletions: 0,
                patch: None,
            },
            PullFile {
                filename: "moved.txt".into(),
                previous_filename: Some("old.txt".into()),
                status: FileStatus::Renamed,
                additions: 0,
                deletions: 0,
                patch: None,
            },
        ];
        one.reviews = vec![
            review(10, Some("carol"), ReviewState::Approved, "LGTM"),
            review(11, None, ReviewState::Commented, ""),
            review(12, Some("alice"), ReviewState::Pending, "draft"),
        ];
        one.comments = vec![
            IssueComment {
                id: 22,
                author: Some("dave".into()),
                body: "second".into(),
                created_at: "2026-04-01T09:00:00Z".into(),
            },
            IssueComment {
                id: 21,
                author: None,
                body: "first".into(),
                created_at: "2026-04-01T08:00:00Z".into(),
            },
        ];
        let own = Pull::new("alice", "notes", 2, "Mine", "alice");
        let mut closed = Pull::new("alice", "notes", 3, "Closed", "bob");
        closed.state = PullState::Closed;
        let frozen_pr = Pull::new("acme", "frozen", 1, "Frozen", "bob");
        s.pulls = vec![one, own, closed, frozen_pr];
    });
    fake
}

const PR1: &str = "/repos/alice/notes/pulls/1";

#[tokio::test]
async fn adr_routes_require_authentication() {
    let fake = adr_fake().await;
    for path in [
        "/user",
        PR1,
        "/repos/alice/notes/pulls/1/files",
        "/repos/alice/notes/pulls/1/reviews",
        "/repos/alice/notes/issues/1/comments",
    ] {
        assert_eq!(get(&fake, path, None).await.status(), 401, "{path}");
    }
}

#[tokio::test]
async fn user_route_serves_the_signed_in_user() {
    let fake = adr_fake().await;
    let resp = authed(&fake, "/user").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["login"], "alice");
    assert_eq!(json["id"], 1);
    assert_eq!(json["type"], "User");
}

#[tokio::test]
async fn pull_detail_has_github_shape_and_computed_counts() {
    let fake = adr_fake().await;
    let resp = authed(&fake, "/repos/Alice/NOTES/pulls/1").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["number"], 1);
    assert_eq!(json["title"], "One");
    assert_eq!(json["user"]["login"], "bob");
    assert_eq!(json["head"]["sha"], SHA_OK);
    assert_eq!(json["head"]["label"], "alice:feature");
    assert_eq!(json["base"]["ref"], "main");
    assert_eq!(json["mergeable"], true);
    assert_eq!(json["mergeable_state"], "clean");
    assert_eq!(json["merged"], false);
    assert_eq!(json["additions"], 2);
    assert_eq!(json["deletions"], 1);
    assert_eq!(json["changed_files"], 3);
    assert_eq!(json["comments"], 2);
    assert_eq!(json["base"]["repo"]["full_name"], "alice/notes");
    assert_eq!(json["base"]["repo"]["archived"], false);
}

#[tokio::test]
async fn pull_detail_mergeable_states_merged_flag_and_archived_repo() {
    let fake = adr_fake().await;
    fake.with_state(|s| {
        s.pull_mut("alice", "notes", 2).unwrap().mergeable = None;
        s.pull_mut("alice", "notes", 1).unwrap().mergeable = Some(false);
        s.pull_mut("alice", "notes", 3).unwrap().state = PullState::Merged;
    });
    let unknown: Value = authed(&fake, "/repos/alice/notes/pulls/2")
        .await
        .json()
        .await
        .unwrap();
    assert!(unknown["mergeable"].is_null());
    assert_eq!(unknown["mergeable_state"], "unknown");
    let dirty: Value = authed(&fake, PR1).await.json().await.unwrap();
    assert_eq!(dirty["mergeable"], false);
    assert_eq!(dirty["mergeable_state"], "dirty");
    let merged: Value = authed(&fake, "/repos/alice/notes/pulls/3")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(merged["merged"], true);
    assert!(!merged["merged_at"].is_null());
    let frozen: Value = authed(&fake, "/repos/acme/frozen/pulls/1")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(frozen["base"]["repo"]["archived"], true);
}

#[tokio::test]
async fn the_seven_routes_answer_404_for_a_missing_repo_or_pull() {
    let fake = adr_fake().await;
    let body = json!({"body": "hi", "event": "APPROVE"});
    for prefix in ["/repos/alice/nope", "/repos/nobody/notes"] {
        for tail in [
            "/pulls/1",
            "/pulls/1/files",
            "/pulls/1/reviews",
            "/issues/1/comments",
        ] {
            let path = format!("{prefix}{tail}");
            let resp = authed(&fake, &path).await;
            assert_eq!(resp.status(), 404, "GET {path}");
            let json: Value = resp.json().await.unwrap();
            assert_eq!(json["message"], "Not Found");
        }
        assert_eq!(
            post(&fake, &format!("{prefix}/issues/1/comments"), &body)
                .await
                .status(),
            404
        );
        assert_eq!(
            post(&fake, &format!("{prefix}/pulls/1/reviews"), &body)
                .await
                .status(),
            404
        );
    }
    for number in ["99", "0", "x"] {
        for tail in ["", "/files", "/reviews"] {
            let path = format!("/repos/alice/notes/pulls/{number}{tail}");
            assert_eq!(authed(&fake, &path).await.status(), 404, "GET {path}");
        }
        let comments = format!("/repos/alice/notes/issues/{number}/comments");
        assert_eq!(
            authed(&fake, &comments).await.status(),
            404,
            "GET {comments}"
        );
        assert_eq!(post(&fake, &comments, &body).await.status(), 404);
        let reviews = format!("/repos/alice/notes/pulls/{number}/reviews");
        assert_eq!(post(&fake, &reviews, &body).await.status(), 404);
    }
    assert!(fake.with_state(|s| s.pulls.iter().all(|p| p.comments.len() <= 2)));
}

#[tokio::test]
async fn files_are_served_in_order_with_optional_keys() {
    let fake = adr_fake().await;
    let json: Value = authed(&fake, "/repos/alice/notes/pulls/1/files")
        .await
        .json()
        .await
        .unwrap();
    let files = json.as_array().unwrap();
    let names: Vec<_> = files
        .iter()
        .map(|f| f["filename"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["src/a.rs", "new.png", "moved.txt"]);
    assert_eq!(files[0]["status"], "modified");
    assert_eq!(files[0]["additions"], 2);
    assert_eq!(files[0]["deletions"], 1);
    assert_eq!(files[0]["changes"], 3);
    assert!(files[0]["patch"].as_str().unwrap().starts_with("@@ -1,2"));
    assert!(files[0].get("previous_filename").is_none());
    assert_eq!(files[1]["status"], "added");
    assert!(files[1].get("patch").is_none(), "patch key absent");
    assert_eq!(files[2]["status"], "renamed");
    assert_eq!(files[2]["previous_filename"], "old.txt");
}

#[tokio::test]
async fn reviews_are_served_chronologically_with_github_states() {
    let fake = adr_fake().await;
    let json: Value = authed(&fake, "/repos/alice/notes/pulls/1/reviews")
        .await
        .json()
        .await
        .unwrap();
    let list = json.as_array().unwrap();
    let ids: Vec<_> = list.iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [10, 11, 12]);
    assert_eq!(list[0]["state"], "APPROVED");
    assert_eq!(list[0]["user"]["login"], "carol");
    assert_eq!(list[0]["body"], "LGTM");
    assert_eq!(list[0]["commit_id"], SHA_OK);
    assert_eq!(list[0]["submitted_at"], "2026-04-01T08:00:00Z");
    assert!(list[1]["user"].is_null());
    assert_eq!(list[1]["state"], "COMMENTED");
    assert_eq!(list[2]["state"], "PENDING");
    assert!(
        list[2].get("submitted_at").is_none(),
        "Pending has no submitted_at"
    );
}

#[tokio::test]
async fn comments_are_served_sorted_by_id() {
    let fake = adr_fake().await;
    let json: Value = authed(&fake, "/repos/alice/notes/issues/1/comments")
        .await
        .json()
        .await
        .unwrap();
    let list = json.as_array().unwrap();
    assert_eq!(list[0]["id"], 21);
    assert_eq!(list[1]["id"], 22);
    assert!(list[0]["user"].is_null());
    assert_eq!(list[1]["user"]["login"], "dave");
    assert_eq!(list[1]["body"], "second");
    assert_eq!(list[1]["created_at"], "2026-04-01T09:00:00Z");
}

#[tokio::test]
async fn the_three_lists_are_paginated_with_link_headers() {
    let fake = adr_fake().await;
    fake.with_state(|s| {
        s.max_per_page = 2;
        let p = s.pull_mut("alice", "notes", 1).unwrap();
        for n in 0..3 {
            p.files
                .push(PullFile::modified(&format!("extra{n}.rs"), "+x"));
            p.reviews
                .push(review(100 + n, Some("carol"), ReviewState::Commented, "x"));
            p.comments.push(IssueComment {
                id: 100 + n,
                author: Some("dave".into()),
                body: "x".into(),
                created_at: "2026-04-02T00:00:00Z".into(),
            });
        }
    });
    for (path, total, key) in [
        ("/repos/alice/notes/pulls/1/files", 6usize, "filename"),
        ("/repos/alice/notes/pulls/1/reviews", 6, "id"),
        ("/repos/alice/notes/issues/1/comments", 5, "id"),
    ] {
        let first = authed(&fake, &format!("{path}?per_page=100")).await;
        let link = header(&first, "link");
        assert!(
            link.contains(&format!(
                "http://{}{path}?per_page=2&page=2>; rel=\"next\"",
                fake.addr()
            )),
            "{link}"
        );
        let mut seen = Vec::new();
        let mut page = 1;
        loop {
            let resp = authed(&fake, &format!("{path}?page={page}")).await;
            let more = resp.headers().contains_key("link");
            let json: Value = resp.json().await.unwrap();
            let items = json.as_array().unwrap();
            assert!(items.len() <= 2);
            seen.extend(items.iter().map(|i| i[key].to_string()));
            if !more {
                break;
            }
            page += 1;
        }
        assert_eq!(seen.len(), total, "{path}");
        let unique: std::collections::HashSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), total, "{path}: no duplicate");
    }
    let default = authed(&fake, "/repos/alice/notes/pulls/1/files").await;
    assert!(
        header(&default, "link").contains("per_page=2"),
        "capped by max_per_page"
    );
}

// -- POST comments --

#[tokio::test]
async fn posting_a_comment_adds_it_and_the_next_get_shows_it() {
    let fake = adr_fake().await;
    fake.with_state(|s| s.next_id = 500);
    let path = "/repos/alice/notes/issues/1/comments";
    let resp = post(&fake, path, &json!({"body": "  hello **you**\n"})).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = resp.json().await.unwrap();
    assert_eq!(created["id"], 500);
    assert_eq!(created["user"]["login"], "alice");
    assert_eq!(created["body"], "  hello **you**\n", "sent as is");
    assert_eq!(created["created_at"], NOW);

    let list: Value = authed(&fake, path).await.json().await.unwrap();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[2]["id"], 500);
    let second = post(&fake, path, &json!({"body": "again"})).await;
    let second: Value = second.json().await.unwrap();
    assert_eq!(second["id"], 501, "next_id incremented");
    assert_eq!(fake.with_state(|s| s.next_id), 502);
    let updated = fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().updated_at.clone());
    assert_eq!(updated, NOW);
    let detail: Value = authed(&fake, PR1).await.json().await.unwrap();
    assert_eq!(detail["comments"], 4);

    let posts: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect();
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].path, path);
    assert_eq!(posts[0].body, Some(json!({"body": "  hello **you**\n"})));
}

#[tokio::test]
async fn a_comment_is_accepted_on_a_closed_pull() {
    let fake = adr_fake().await;
    let resp = post(
        &fake,
        "/repos/alice/notes/issues/3/comments",
        &json!({"body": "late"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn a_missing_non_string_or_blank_comment_body_is_422() {
    let fake = adr_fake().await;
    let path = "/repos/alice/notes/issues/1/comments";
    for body in [
        json!({}),
        json!({"body": 3}),
        json!({"body": null}),
        json!({"body": ""}),
        json!({"body": " \n\t "}),
    ] {
        let resp = post(&fake, path, &body).await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["message"], "Validation Failed", "{body}");
        assert!(json["errors"].is_array());
    }
    let raw = client()
        .post(format!("{}{path}", fake.base_url()))
        .header("Authorization", format!("Bearer {DEFAULT_TOKEN}"))
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(raw.status(), StatusCode::BAD_REQUEST);
    let json: Value = raw.json().await.unwrap();
    assert_eq!(json["message"], "Problems parsing JSON");
    assert!(json["documentation_url"].is_string());
    let array = post(&fake, path, &json!([1])).await;
    assert_eq!(array.status(), StatusCode::BAD_REQUEST, "not an object");
    assert_eq!(
        fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().comments.len()),
        2
    );
    assert_eq!(fake.with_state(|s| s.next_id), 1_000_000, "no id consumed");
}

#[tokio::test]
async fn a_review_body_that_is_not_a_json_object_is_a_400_and_creates_nothing() {
    let fake = adr_fake().await;
    let before = fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().reviews.len());
    let raw = client()
        .post(format!("{}{R1}", fake.base_url()))
        .header("Authorization", format!("Bearer {DEFAULT_TOKEN}"))
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(raw.status(), StatusCode::BAD_REQUEST);
    let json: Value = raw.json().await.unwrap();
    assert_eq!(json["message"], "Problems parsing JSON");
    assert!(json["documentation_url"].is_string());
    let array = post(&fake, R1, &json!(["APPROVE"])).await;
    assert_eq!(array.status(), StatusCode::BAD_REQUEST, "not an object");
    assert_eq!(
        fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().reviews.len()),
        before
    );
    assert_eq!(fake.with_state(|s| s.next_id), 1_000_000, "no id consumed");
}

#[tokio::test]
async fn posting_to_an_archived_repo_is_403_and_changes_nothing() {
    let fake = adr_fake().await;
    let resp = post(
        &fake,
        "/repos/acme/frozen/issues/1/comments",
        &json!({"body": "x"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Repository was archived so is read-only.");
    let resp = post(
        &fake,
        "/repos/acme/frozen/pulls/1/reviews",
        &json!({"event": "APPROVE"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Repository was archived so is read-only.");
    assert_eq!(
        fake.with_state(|s| {
            let p = s.pull_mut("acme", "frozen", 1).unwrap();
            p.comments.len() + p.reviews.len()
        }),
        0
    );
    assert_eq!(fake.with_state(|s| s.next_id), 1_000_000);
}

// -- POST reviews --

async fn review_errors(fake: &FakeGitHub, path: &str, body: Value) -> Vec<String> {
    let resp = post(fake, path, &body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["message"], "Unprocessable Entity");
    assert!(json["documentation_url"].is_string());
    json["errors"]
        .as_array()
        .expect("errors array")
        .iter()
        .map(|e| e.as_str().expect("string error").to_owned())
        .collect()
}

const R1: &str = "/repos/alice/notes/pulls/1/reviews";
const R_OWN: &str = "/repos/alice/notes/pulls/2/reviews";
const R_CLOSED: &str = "/repos/alice/notes/pulls/3/reviews";

fn assert_nothing_added(fake: &FakeGitHub) {
    let total = fake.with_state(|s| s.pulls.iter().map(|p| p.reviews.len()).sum::<usize>());
    assert_eq!(total, 3, "only the three seeded reviews");
    assert_eq!(fake.with_state(|s| s.next_id), 1_000_000);
}

#[tokio::test]
async fn review_on_a_pull_that_is_not_open_is_422_first() {
    let fake = adr_fake().await;
    // Also an unknown event and a bad commit: the closed pull is reported alone.
    let errors = review_errors(&fake, R_CLOSED, json!({"event": "NOPE", "commit_id": "zz"})).await;
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("not open"), "{errors:?}");
    fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().state = PullState::Merged);
    let errors = review_errors(&fake, R1, json!({"event": "APPROVE"})).await;
    assert!(errors[0].contains("not open"), "{errors:?}");
    assert_nothing_added(&fake);
}

#[tokio::test]
async fn approve_or_request_changes_on_an_own_pull_is_422() {
    let fake = adr_fake().await;
    let approve = review_errors(&fake, R_OWN, json!({"event": "APPROVE"})).await;
    assert_eq!(approve, ["Can not approve your own pull request"]);
    let request = review_errors(&fake, R_OWN, json!({"event": "REQUEST_CHANGES"})).await;
    assert_eq!(
        request,
        ["Can not request changes on your own pull request"],
        "own comes before the body rule"
    );
    // Case-insensitive author match.
    fake.with_state(|s| s.pull_mut("alice", "notes", 2).unwrap().author = Some("ALICE".into()));
    let approve = review_errors(&fake, R_OWN, json!({"event": "APPROVE"})).await;
    assert_eq!(approve, ["Can not approve your own pull request"]);
    assert_nothing_added(&fake);
}

#[tokio::test]
async fn comment_on_an_own_pull_is_accepted() {
    let fake = adr_fake().await;
    let resp = post(
        &fake,
        R_OWN,
        &json!({"event": "COMMENT", "body": "note to self"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["state"], "COMMENTED");
}

#[tokio::test]
async fn request_changes_or_comment_without_a_non_blank_body_is_422() {
    let fake = adr_fake().await;
    for event in ["REQUEST_CHANGES", "COMMENT"] {
        for body in [
            json!({"event": event}),
            json!({"event": event, "body": ""}),
            json!({"event": event, "body": " \n "}),
            json!({"event": event, "body": 5}),
        ] {
            let errors = review_errors(&fake, R1, body.clone()).await;
            assert_eq!(errors.len(), 1, "{body}");
            assert!(errors[0].contains("body"), "{errors:?}");
        }
    }
    // Body rule comes before the pending-review rule.
    // PR 1 holds a Pending review of alice: the body rule is still reported first.
    let errors = review_errors(&fake, R1, json!({"event": "COMMENT"})).await;
    assert!(errors[0].contains("body"));
    assert_nothing_added(&fake);
}

#[tokio::test]
async fn a_second_pending_review_of_the_user_is_422() {
    let fake = adr_fake().await;
    // PR 1 already holds a Pending review of alice (seeded).
    for body in [
        json!({}),
        json!({"event": "APPROVE"}),
        json!({"event": "NOPE"}),
    ] {
        let errors = review_errors(&fake, R1, body).await;
        assert_eq!(
            errors,
            ["User can only have one pending review per pull request"]
        );
    }
    assert_nothing_added(&fake);
}

#[tokio::test]
async fn a_pending_review_of_someone_else_does_not_block() {
    let fake = adr_fake().await;
    fake.with_state(|s| {
        let p = s.pull_mut("alice", "notes", 1).unwrap();
        p.reviews.retain(|r| r.state != ReviewState::Pending);
        p.reviews
            .push(review(13, Some("carol"), ReviewState::Pending, ""));
    });
    let resp = post(&fake, R1, &json!({"event": "APPROVE"})).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_unknown_event_is_422_and_precedes_the_commit_check() {
    let fake = adr_fake().await;
    fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().reviews.clear());
    for event in [json!("NOPE"), json!("approve"), json!(7), json!("PENDING")] {
        let errors = review_errors(&fake, R1, json!({"event": event, "commit_id": "zz"})).await;
        assert_eq!(errors.len(), 1, "{event}");
        assert!(errors[0].contains("event"), "{errors:?}");
    }
}

#[tokio::test]
async fn a_commit_id_that_is_not_40_or_64_hex_digits_is_422() {
    let fake = adr_fake().await;
    fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().reviews.clear());
    let sha64 = "a".repeat(64);
    for bad in [
        json!("abc"),
        json!(&SHA_OK[..39]),
        json!(format!("{SHA_OK}0")),
        json!("g".repeat(40)),
        json!(&sha64[..63]),
        json!(12),
        json!(""),
    ] {
        let errors = review_errors(&fake, R1, json!({"event": "APPROVE", "commit_id": bad})).await;
        assert_eq!(errors.len(), 1, "{bad}");
        assert!(errors[0].contains("commit_id"), "{errors:?}");
    }
    for good in [SHA_OK, sha64.as_str()] {
        let resp = post(
            &fake,
            R1,
            &json!({"event": "COMMENT", "body": "ok", "commit_id": good}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "{good}");
    }
}

#[tokio::test]
async fn publishing_reviews_records_state_commit_body_and_time() {
    let fake = adr_fake().await;
    fake.with_state(|s| {
        s.pull_mut("alice", "notes", 1).unwrap().reviews.clear();
        s.next_id = 700;
    });
    let sha64 = "b".repeat(64);
    let cases = [
        (json!({"event": "APPROVE"}), "APPROVED", "", SHA_OK),
        (
            json!({"event": "REQUEST_CHANGES", "body": "fix it", "commit_id": sha64}),
            "CHANGES_REQUESTED",
            "fix it",
            sha64.as_str(),
        ),
        (
            json!({"event": "COMMENT", "body": " remark "}),
            "COMMENTED",
            " remark ",
            SHA_OK,
        ),
    ];
    for (i, (body, state, text, commit)) in cases.iter().enumerate() {
        let resp = post(&fake, R1, body).await;
        assert_eq!(resp.status(), StatusCode::OK, "{body}");
        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["id"], 700 + i as u64);
        assert_eq!(json["state"], *state);
        assert_eq!(json["body"], *text);
        assert_eq!(json["commit_id"], *commit);
        assert_eq!(json["user"]["login"], "alice");
        assert_eq!(json["submitted_at"], NOW);
        assert_eq!(
            fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().updated_at.clone()),
            NOW,
            "a submitted review touches the pull"
        );
        fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().updated_at = "old".into());
    }
    let list: Value = authed(&fake, R1).await.json().await.unwrap();
    let ids: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [700, 701, 702], "visible at the next GET, in order");
    assert_eq!(fake.with_state(|s| s.next_id), 703);
    let posts: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts[1].body.as_ref().unwrap()["body"], "fix it");
    assert_eq!(posts[1].body.as_ref().unwrap()["event"], "REQUEST_CHANGES");
}

#[tokio::test]
async fn a_review_without_event_is_created_pending_and_blocks_the_next() {
    let fake = adr_fake().await;
    fake.with_state(|s| s.pull_mut("alice", "notes", 1).unwrap().reviews.clear());
    let resp = post(&fake, R1, &json!({"body": "wip"})).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["state"], "PENDING");
    assert_eq!(json["id"], 1_000_000);
    assert_eq!(json["body"], "wip");
    assert!(json.get("submitted_at").is_none());
    let list: Value = authed(&fake, R1).await.json().await.unwrap();
    assert_eq!(list[0]["state"], "PENDING");
    let errors = review_errors(&fake, R1, json!({"event": "APPROVE"})).await;
    assert_eq!(
        errors,
        ["User can only have one pending review per pull request"]
    );
}

// -- state helpers and demo --

#[test]
fn pull_mut_finds_a_pull_case_insensitively() {
    let mut state = State::new(user());
    state.pulls = vec![Pull::new("Alice", "Notes", 4, "t", "bob")];
    assert_eq!(state.pull_mut("alice", "notes", 4).unwrap().title, "t");
    state.pull_mut("ALICE", "NOTES", 4).unwrap().title = "edited".into();
    assert_eq!(state.pulls[0].title, "edited");
    assert!(state.pull_mut("alice", "notes", 5).is_none());
    assert!(state.pull_mut("alice", "other", 4).is_none());
}

#[test]
fn pull_file_modified_counts_plus_and_minus_lines() {
    let file = PullFile::modified(
        "a.rs",
        "@@ -1,3 +1,4 @@\n ctx\n-old1\n-old2\n+new1\n+new2\n+new3\n ctx",
    );
    assert_eq!(file.filename, "a.rs");
    assert_eq!(file.status, FileStatus::Modified);
    assert_eq!((file.additions, file.deletions), (3, 2));
    assert!(file.previous_filename.is_none());
    assert!(file.patch.as_deref().unwrap().starts_with("@@"));
    let empty = PullFile::modified("b.rs", "");
    assert_eq!((empty.additions, empty.deletions), (0, 0));
}

#[tokio::test]
async fn demo_state_has_the_pr_detail_data() {
    let fake = FakeGitHub::start(State::demo()).await.unwrap();
    let notes: Value = authed(&fake, "/repos/alice/notes/pulls?state=all&per_page=100")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        notes.as_array().unwrap().len(),
        4,
        "alice/notes keeps its four PRs"
    );

    let detail: Value = authed(&fake, PR1).await.json().await.unwrap();
    let body = detail["body"].as_str().expect("markdown body");
    for needle in ["# ", "**", "- ", "```", "![", "<"] {
        assert!(body.contains(needle), "body lacks {needle:?}");
    }
    assert_eq!(detail["comments"], 2);
    assert_eq!(detail["changed_files"], 5);

    let reviews: Value = authed(&fake, "/repos/alice/notes/pulls/1/reviews")
        .await
        .json()
        .await
        .unwrap();
    let reviews = reviews.as_array().unwrap();
    let find = |state: &str| {
        reviews
            .iter()
            .filter(|r| r["state"] == state)
            .collect::<Vec<_>>()
    };
    assert_eq!(find("APPROVED")[0]["user"]["login"], "carol");
    assert_eq!(find("CHANGES_REQUESTED")[0]["user"]["login"], "dave");
    assert!(!find("COMMENTED")[0]["body"].as_str().unwrap().is_empty());
    assert_eq!(find("PENDING")[0]["user"]["login"], "alice");

    let files: Value = authed(&fake, "/repos/alice/notes/pulls/1/files")
        .await
        .json()
        .await
        .unwrap();
    let files = files.as_array().unwrap();
    let status = |s: &str| files.iter().filter(|f| f["status"] == s).count();
    assert!(status("modified") >= 1 && status("added") >= 1 && status("removed") >= 1);
    assert!(
        files
            .iter()
            .any(|f| f["status"] == "renamed" && f.get("patch").is_none() && f["changes"] == 0)
    );
    assert!(
        files
            .iter()
            .any(|f| f.get("patch").is_none() && f["status"] != "renamed"),
        "binary"
    );
    assert!(
        files
            .iter()
            .any(|f| f["status"] == "modified" && f.get("patch").is_some())
    );

    let comments: Value = authed(&fake, "/repos/alice/notes/issues/1/comments")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(comments.as_array().unwrap().len(), 2);

    // Own PR, archived repo, conflicting PR.
    let own: Value = authed(&fake, "/repos/bob/shared-tools/pulls?per_page=100")
        .await
        .json()
        .await
        .unwrap();
    assert!(
        own.as_array()
            .unwrap()
            .iter()
            .any(|p| p["user"]["login"] == "alice" && p["state"] == "open")
    );
    let legacy: Value = authed(&fake, "/repos/acme/legacy-site/pulls?per_page=100")
        .await
        .json()
        .await
        .unwrap();
    let legacy_pr = legacy
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["state"] == "open")
        .expect("open PR");
    let number = legacy_pr["number"].as_u64().unwrap();
    let detail: Value = authed(&fake, &format!("/repos/acme/legacy-site/pulls/{number}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(detail["base"]["repo"]["archived"], true);
    let conflicts = fake.with_state(|s| {
        s.pulls
            .iter()
            .any(|p| p.mergeable == Some(false) && p.state == PullState::Open)
    });
    assert!(conflicts);
}
