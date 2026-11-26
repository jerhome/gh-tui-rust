//! Fake GitHub REST v3 server for `ghu` tests and demos (ADR 0001).
//!
//! Contract:
//! - Real HTTP on `127.0.0.1` only (never another interface), on a port chosen by the OS unless told otherwise.
//! - Serves GitHub's JSON shapes (field names, status codes, `Link` pagination, error bodies
//!   `{"message": …, "documentation_url": …}`) as documented at <https://docs.github.com/en/rest>, from an
//!   in-memory [`State`] that tests read and modify while the server runs ([`FakeGitHub::with_state`]).
//! - Every request is checked and recorded, in this order: latency ([`State::latency`]), recording
//!   ([`State::requests`]), authentication (`Authorization: Bearer <State::token>` or `token <…>`, else 401
//!   `Bad credentials`, or `Requires authentication` without the header), injected [`Failure`]s, then the route.
//!   An unknown route answers 404 `{"message": "Not Found"}`.
//! - 422 bodies take both shapes real GitHub sends, and clients must accept both: a route's own validation error
//!   uses the object form `{"message": "Validation Failed", "errors": [{"resource", "code", "field"}], …}` (entries
//!   without `message`), an injected [`FailureResponse::Unprocessable`] the string form
//!   `{"message": m, "errors": [m], …}`. `gh-client` keeps `message` plus the `errors[]` entries that are strings
//!   or carry a `message`, so the first form reads `Validation Failed` alone.
//! - Routes (later tickets add routes and `State` fields; existing ones keep their meaning):
//!   - `GET /user/repos` (#2): query `affiliation` (default all three), `per_page`, `page`; sorted by
//!     `owner/name` case-insensitively; [`Access::NoAffiliation`] repos never listed.
//!   - `GET /repos/{owner}/{repo}` (#4): the repo of [`State::repos`] whose owner and name match
//!     case-insensitively, whatever its [`Access`], with its canonical `full_name`; else 404 `Not Found`.
//!   - `GET /repos/{owner}/{repo}/pulls` (#5): 404 `Not Found` if the repo is not in [`State::repos`]; else its
//!     [`State::pulls`] filtered by `state` (`open` default, `closed` = closed and merged, `all`; anything else:
//!     422 `Validation Failed`), sorted by `sort` (`created` default, `updated`) and `direction` (`desc`
//!     default, `asc`), ties broken by `number` descending.
//!   - Every list route paginates the same way: `per_page` (default 30, capped by [`State::max_per_page`]),
//!     `page` (default 1), and a `Link` header with `next` and `last` (absolute URLs on the fake's address,
//!     keeping the other query parameters) while a next page exists.
//!
//! Threads: the server runs as a task on the tokio runtime that called [`FakeGitHub::start`]; works on a
//! current-thread runtime (`#[tokio::test]`). [`State`] is behind a `std::sync::Mutex`, never held across an
//! `.await`.

mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub use state::{
    Access, Failure, FailureResponse, Method, Pull, PullState, RecordedRequest, Repo, State, User,
};

/// The token [`State::new`] and [`State::demo`] accept.
pub const DEFAULT_TOKEN: &str = "ghu-fake-token";

/// A running fake server. Dropping it stops the server (the task is aborted; in-flight requests are cut).
pub struct FakeGitHub {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeGitHub {
    /// Starts the server on `127.0.0.1` with a port chosen by the OS. Must be called inside a tokio runtime.
    ///
    /// Errors: the `std::io::Error` of binding the port.
    pub async fn start(state: State) -> std::io::Result<Self> {
        Self::start_on(0, state).await
    }

    /// Like [`FakeGitHub::start`] on a given port (`0` = chosen by the OS); used by the `gh-fake` binary.
    pub async fn start_on(port: u16, state: State) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(state));
        let app = routes::router(Arc::clone(&state), addr);
        let task = tokio::spawn(async move {
            // The task ends when aborted by `Drop`; a serve error has nobody to report to.
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { addr, state, task })
    }

    /// The address the server listens on (always loopback).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The URL to give `ghu` as `GHU_API_URL`: `http://127.0.0.1:<port>`, without trailing `/`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Runs `f` on the live state (lock held during `f` only; do not `.await` inside). Changes apply to the next
    /// request, for example a repo added, a failure injected, or the latency changed.
    ///
    /// Panics: never on a poisoned lock (the poison is ignored).
    pub fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }

    /// A copy of the requests received so far, oldest first (same as `with_state(|s| s.requests.clone())`).
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.with_state(|s| s.requests.clone())
    }
}

impl Drop for FakeGitHub {
    /// Aborts the server task.
    fn drop(&mut self) {
        self.task.abort();
    }
}
