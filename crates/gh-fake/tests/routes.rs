//! Routes of the fake server, driven over real HTTP with reqwest (ticket #2).

use std::time::{Duration, Instant};

use gh_fake::{
    Access, DEFAULT_TOKEN, Failure, FailureResponse, FakeGitHub, Method, Repo, State, User,
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
    assert!(repos.iter().all(|r| r["owner"]["login"] == "alice"));
}

#[tokio::test]
async fn base_url_is_loopback_without_trailing_slash() {
    let fake = start(vec![]).await;
    assert!(fake.addr().ip().is_loopback());
    assert!(fake.base_url().starts_with("http://127.0.0.1:"));
    assert!(!fake.base_url().ends_with('/'));
}
