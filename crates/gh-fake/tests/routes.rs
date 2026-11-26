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
