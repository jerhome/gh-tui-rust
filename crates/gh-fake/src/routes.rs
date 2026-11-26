//! Axum routes and middleware (private): latency, recording, authentication, injected failures, route.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Query, Request, State as AxState};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};

use crate::state::{Access, Failure, FailureResponse, Method, RecordedRequest, Repo, State};

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
    message(StatusCode::NOT_FOUND, RATE_LIMIT - 1, "Not Found")
}

/// Token after `Bearer ` or `token `; `Err(())`-like `None` inside means a header we cannot read.
fn parse_authorization(headers: &HeaderMap) -> (bool, Option<String>) {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return (false, None);
    };
    let token = value.to_str().ok().and_then(|v| {
        v.strip_prefix("Bearer ")
            .or_else(|| v.strip_prefix("token "))
            .map(str::to_owned)
    });
    (true, token)
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
    let (has_header, token) = parse_authorization(&parts.headers);
    let path = parts.uri.path().to_owned();
    app.lock().requests.push(RecordedRequest {
        method,
        path: path.clone(),
        query: parts.uri.query().map(str::to_owned),
        token: token.clone(),
        body: serde_json::from_slice(&bytes).ok(),
    });
    let req = Request::from_parts(parts, Body::from(bytes));

    // 3. Authentication.
    let accepted = token.is_some_and(|t| t == app.lock().token);
    if !accepted {
        let text = if has_header {
            "Bad credentials"
        } else {
            "Requires authentication"
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
    };
    affiliation.split(',').any(|a| a.trim() == wanted)
}

async fn user_repos(
    AxState(app): AxState<App>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let (body, max_per_page) = {
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
        let max = state.max_per_page.max(1);
        let per_page = query
            .get("per_page")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_PER_PAGE)
            .min(max);
        let page = query
            .get("page")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        let last = repos.len().div_ceil(per_page).max(1);
        let start = (page - 1).saturating_mul(per_page).min(repos.len());
        let end = start.saturating_add(per_page).min(repos.len());
        let items: Vec<Value> = repos[start..end]
            .iter()
            .map(|r| repo_json(r, &state))
            .collect();
        (
            items,
            (per_page, page, last, query.get("affiliation").cloned()),
        )
    };
    let (per_page, page, last, affiliation) = max_per_page;
    let url = |p: usize| {
        let mut u = format!("http://{}/user/repos?", app.addr);
        if let Some(a) = &affiliation {
            u.push_str(&format!("affiliation={a}&"));
        }
        u.push_str(&format!("per_page={per_page}&page={p}"));
        u
    };
    let mut resp = axum::Json(body).into_response();
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
