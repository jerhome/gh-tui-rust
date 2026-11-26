//! Axum routes and middleware (private): latency, recording, authentication, injected failures, route.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query, Request, State as AxState};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};

use crate::state::{
    Access, Failure, FailureResponse, Method, Pull, PullState, RecordedRequest, Repo, State,
};

const DOC_URL: &str = "https://docs.github.com/rest";
const RATE_LIMIT: u32 = 5000;
const RECORDED_BODY_LIMIT: usize = 1 << 20;
const DEFAULT_PER_PAGE: usize = 30;

#[derive(Clone)]
struct App {
    state: Arc<Mutex<State>>,
    addr: SocketAddr,
}

impl App {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) fn router(state: Arc<Mutex<State>>, addr: SocketAddr) -> Router {
    let app = App { state, addr };
    Router::new()
        .route("/user/repos", get(user_repos))
        .route("/repos/{owner}/{repo}", get(repo))
        .route("/repos/{owner}/{repo}/pulls", get(pulls))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(app.clone(), pipeline))
        .with_state(app)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn rate_headers(remaining: u32) -> [(&'static str, String); 4] {
    [
        ("x-ratelimit-limit", RATE_LIMIT.to_string()),
        ("x-ratelimit-remaining", remaining.to_string()),
        ("x-ratelimit-reset", (now_secs() + 3600).to_string()),
        ("x-ratelimit-used", (RATE_LIMIT - remaining).to_string()),
    ]
}

fn error_response(status: StatusCode, remaining: u32, body: Value) -> Response {
    let mut resp = (status, axum::Json(body)).into_response();
    for (name, value) in rate_headers(remaining) {
        resp.headers_mut()
            .insert(name, HeaderValue::from_str(&value).expect("ascii digits"));
    }
    resp
}

fn message(status: StatusCode, remaining: u32, text: &str) -> Response {
    error_response(
        status,
        remaining,
        json!({"message": text, "documentation_url": DOC_URL}),
    )
}

async fn not_found() -> Response {
    not_found_with(RATE_LIMIT - 1)
}

fn not_found_with(remaining: u32) -> Response {
    message(StatusCode::NOT_FOUND, remaining, "Not Found")
}

/// What the `Authorization` header carries.
enum Credentials {
    /// No header at all.
    Missing,
    /// A header that is not `Bearer <token>` / `token <token>` (or not readable text).
    Unusable,
    Token(String),
}

impl Credentials {
    fn token(&self) -> Option<&str> {
        match self {
            Self::Token(t) => Some(t),
            _ => None,
        }
    }
}

fn parse_authorization(headers: &HeaderMap) -> Credentials {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Credentials::Missing;
    };
    let token = value.to_str().ok().and_then(|v| {
        v.strip_prefix("Bearer ")
            .or_else(|| v.strip_prefix("token "))
            .map(str::to_owned)
    });
    token.map_or(Credentials::Unusable, Credentials::Token)
}

async fn pipeline(AxState(app): AxState<App>, req: Request, next: Next) -> Response {
    // 1. Latency.
    let latency = app.lock().latency;
    if !latency.is_zero() {
        tokio::time::sleep(latency).await;
    }

    let method = match *req.method() {
        axum::http::Method::GET => Some(Method::Get),
        axum::http::Method::POST => Some(Method::Post),
        _ => None,
    };
    let Some(method) = method else {
        return not_found().await;
    };

    // 2. Recording (the body is read, bounded, then put back).
    let (parts, body) = req.into_parts();
    let bytes = to_bytes(body, RECORDED_BODY_LIMIT)
        .await
        .unwrap_or_default();
    let credentials = parse_authorization(&parts.headers);
    let path = parts.uri.path().to_owned();
    app.lock().requests.push(RecordedRequest {
        method,
        path: path.clone(),
        query: parts.uri.query().map(str::to_owned),
        token: credentials.token().map(str::to_owned),
        body: serde_json::from_slice(&bytes).ok(),
    });
    let req = Request::from_parts(parts, Body::from(bytes));

    // 3. Authentication.
    let accepted = credentials.token().is_some_and(|t| t == app.lock().token);
    if !accepted {
        let text = if matches!(credentials, Credentials::Missing) {
            "Requires authentication"
        } else {
            "Bad credentials"
        };
        return message(StatusCode::UNAUTHORIZED, RATE_LIMIT, text);
    }

    // 4. Injected failures.
    if let Some(response) = take_failure(&mut app.lock().failures, method, &path) {
        return failure_response(&response);
    }

    // 5. Route.
    let mut resp = next.run(req).await;
    for (name, value) in rate_headers(RATE_LIMIT - 1) {
        resp.headers_mut()
            .insert(name, HeaderValue::from_str(&value).expect("ascii digits"));
    }
    resp
}

fn take_failure(
    failures: &mut Vec<Failure>,
    method: Method,
    path: &str,
) -> Option<FailureResponse> {
    let index = failures
        .iter()
        .position(|f| f.method == method && f.path == path)?;
    let failure = &mut failures[index];
    let response = failure.response.clone();
    match failure.remaining {
        Some(n) if n <= 1 => {
            failures.remove(index);
        }
        Some(n) => failure.remaining = Some(n - 1),
        None => {}
    }
    Some(response)
}

fn failure_response(response: &FailureResponse) -> Response {
    let ok = RATE_LIMIT - 1;
    match response {
        FailureResponse::Unauthorized => message(StatusCode::UNAUTHORIZED, ok, "Bad credentials"),
        FailureResponse::Forbidden { message: m } => message(StatusCode::FORBIDDEN, ok, m),
        FailureResponse::RateLimited => message(
            StatusCode::FORBIDDEN,
            0,
            "API rate limit exceeded for user ID 1. If you reach out to GitHub Support for help, please include the request ID.",
        ),
        FailureResponse::NotFound => message(StatusCode::NOT_FOUND, ok, "Not Found"),
        FailureResponse::Unprocessable { message: m } => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            ok,
            json!({"message": m, "errors": [m], "documentation_url": DOC_URL}),
        ),
        FailureResponse::ServerError => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, ok, json!({}))
        }
    }
}

/// FNV-1a, for stable fake ids.
fn fake_id(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    }) >> 16
}

fn repo_json(repo: &Repo, state: &State) -> Value {
    let full_name = format!("{}/{}", repo.owner, repo.name);
    let owner_id = if repo.owner == state.user.login {
        state.user.id
    } else {
        fake_id(&repo.owner)
    };
    json!({
        "id": fake_id(&full_name),
        "name": repo.name,
        "full_name": full_name,
        "owner": { "login": repo.owner, "id": owner_id, "type": "User" },
        "private": repo.private,
        "description": repo.description,
        "fork": repo.fork,
        "archived": repo.archived,
        "html_url": format!("https://github.com/{full_name}"),
    })
}

fn access_matches(access: Access, affiliation: &str) -> bool {
    let wanted = match access {
        Access::Owner => "owner",
        Access::Collaborator => "collaborator",
        Access::OrganizationMember => "organization_member",
        Access::NoAffiliation => return false,
    };
    affiliation.split(',').any(|a| a.trim() == wanted)
}

/// Percent-encodes everything but URL-unreserved characters, so any decoded query value is header-safe.
fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b',') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Positive integer query parameter, if present and valid.
fn positive(query: &HashMap<String, String>, name: &str) -> Option<usize> {
    query
        .get(name)
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
}

/// Serves the requested page of `items` (already filtered and sorted) as a JSON array, with the `Link` header
/// while a next page exists. `path` is the route path; the other query parameters are kept in the links.
fn paginated(
    app: &App,
    path: &str,
    query: &HashMap<String, String>,
    items: &[Value],
    max_per_page: usize,
) -> Response {
    let per_page = positive(query, "per_page")
        .unwrap_or(DEFAULT_PER_PAGE)
        .min(max_per_page.max(1));
    let page = positive(query, "page").unwrap_or(1);
    let last = items.len().div_ceil(per_page).max(1);
    let start = (page - 1).saturating_mul(per_page).min(items.len());
    let end = start.saturating_add(per_page).min(items.len());

    let mut kept: Vec<(&String, &String)> = query
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "per_page" | "page"))
        .collect();
    kept.sort();
    let url = |p: usize| {
        let mut u = format!("http://{}{path}?", app.addr);
        for (k, v) in &kept {
            u.push_str(&format!("{}={}&", percent_encode(k), percent_encode(v)));
        }
        u.push_str(&format!("per_page={per_page}&page={p}"));
        u
    };
    let mut resp = axum::Json(&items[start..end]).into_response();
    if page < last {
        let link = format!(
            "<{}>; rel=\"next\", <{}>; rel=\"last\"",
            url(page + 1),
            url(last)
        );
        resp.headers_mut().insert(
            header::LINK,
            HeaderValue::from_str(&link).expect("ascii url"),
        );
    }
    resp
}

async fn user_repos(
    AxState(app): AxState<App>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let (items, max) = {
        let state = app.lock();
        let affiliation = query
            .get("affiliation")
            .map_or("owner,collaborator,organization_member", String::as_str);
        let mut repos: Vec<&Repo> = state
            .repos
            .iter()
            .filter(|r| access_matches(r.access, affiliation))
            .collect();
        repos.sort_by_key(|r| (r.owner.to_lowercase(), r.name.to_lowercase()));
        let items: Vec<Value> = repos.iter().map(|r| repo_json(r, &state)).collect();
        (items, state.max_per_page)
    };
    paginated(&app, "/user/repos", &query, &items, max)
}

/// The repo of `state.repos` whose owner and name match case-insensitively.
fn find_repo<'a>(state: &'a State, owner: &str, name: &str) -> Option<&'a Repo> {
    state
        .repos
        .iter()
        .find(|r| r.owner.eq_ignore_ascii_case(owner) && r.name.eq_ignore_ascii_case(name))
}

async fn repo(AxState(app): AxState<App>, Path((owner, name)): Path<(String, String)>) -> Response {
    let state = app.lock();
    match find_repo(&state, &owner, &name) {
        Some(r) => axum::Json(repo_json(r, &state)).into_response(),
        None => not_found_with(RATE_LIMIT - 1),
    }
}

fn user_json(login: &str, state: &State) -> Value {
    let id = if login == state.user.login {
        state.user.id
    } else {
        fake_id(login)
    };
    json!({ "login": login, "id": id, "type": "User" })
}

fn pull_json(pull: &Pull, repo: &Repo, state: &State) -> Value {
    let full_name = format!("{}/{}", repo.owner, repo.name);
    let closed_at = match pull.state {
        PullState::Open => Value::Null,
        PullState::Closed | PullState::Merged => json!(pull.updated_at),
    };
    let merged_at = match pull.state {
        PullState::Merged => json!(pull.updated_at),
        PullState::Open | PullState::Closed => Value::Null,
    };
    json!({
        "id": fake_id(&format!("{full_name}#{}", pull.number)),
        "number": pull.number,
        "state": if pull.state == PullState::Open { "open" } else { "closed" },
        "title": pull.title,
        "user": pull.author.as_deref().map_or(Value::Null, |a| user_json(a, state)),
        "body": pull.body,
        "draft": pull.draft,
        "created_at": pull.created_at,
        "updated_at": pull.updated_at,
        "closed_at": closed_at,
        "merged_at": merged_at,
        "head": { "ref": pull.head, "label": format!("{}:{}", repo.owner, pull.head) },
        "base": { "ref": pull.base, "label": format!("{}:{}", repo.owner, pull.base) },
        "html_url": format!("https://github.com/{full_name}/pull/{}", pull.number),
    })
}

async fn pulls(
    AxState(app): AxState<App>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let (items, max) = {
        let state = app.lock();
        let Some(repo) = find_repo(&state, &owner, &name) else {
            return not_found_with(RATE_LIMIT - 1);
        };
        let wanted = query.get("state").map_or("open", String::as_str);
        if !matches!(wanted, "open" | "closed" | "all") {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                RATE_LIMIT - 1,
                json!({
                    "message": "Validation Failed",
                    "errors": [{
                        "resource": "Issue",
                        "code": "invalid",
                        "field": "state",
                    }],
                    "documentation_url": DOC_URL,
                }),
            );
        }
        let mut found: Vec<&Pull> = state
            .pulls
            .iter()
            .filter(|p| {
                p.owner.eq_ignore_ascii_case(&repo.owner) && p.repo.eq_ignore_ascii_case(&repo.name)
            })
            .filter(|p| match wanted {
                "open" => p.state == PullState::Open,
                "closed" => p.state != PullState::Open,
                _ => true,
            })
            .collect();
        let by_updated = query.get("sort").is_some_and(|s| s == "updated");
        let ascending = query.get("direction").is_some_and(|d| d == "asc");
        found.sort_by(|a, b| {
            let (ka, kb) = if by_updated {
                (&a.updated_at, &b.updated_at)
            } else {
                (&a.created_at, &b.created_at)
            };
            let order = if ascending { ka.cmp(kb) } else { kb.cmp(ka) };
            order.then(b.number.cmp(&a.number))
        });
        let items: Vec<Value> = found.iter().map(|p| pull_json(p, repo, &state)).collect();
        (items, state.max_per_page)
    };
    let path = format!("/repos/{owner}/{name}/pulls");
    paginated(&app, &path, &query, &items, max)
}
