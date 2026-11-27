//! Integration tests of `gh_client::Client` over real HTTP: against the fake GitHub server of `gh-fake`, and, where
//! the fake cannot show what the client sent or must misbehave on purpose, against a tiny scripted server.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gh_client::{
    Affiliation, ApiBase, BodyProblem, Client, CommitSha, Error, FileStatus, MAX_BODY_CHARS,
    Mergeable, MessageBody, PrNumber, PrState, PullFilter, RepoName, ReviewRequest, ReviewState,
    Token, Verdict,
};
use gh_fake::{
    Access, Failure, FailureResponse, FakeGitHub, Method, Pull, PullState, Repo, State, User,
};

const SECRET: &str = "ghp_TopSecretValue0123456789";

fn state_with_token(repos: Vec<Repo>) -> State {
    let mut state = State::new(User {
        login: "alice".into(),
        id: 1,
    });
    state.token = SECRET.into();
    state.repos = repos;
    state
}

fn client_for(fake: &FakeGitHub, token: &str) -> Client {
    let base = ApiBase::parse(&fake.base_url()).expect("fake base URL is loopback http");
    Client::new(base, Token::new(token).expect("token")).expect("client")
}

fn repo(name: &str) -> Repo {
    Repo::new("alice", name)
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

/// A scripted HTTP server on loopback: answers each connection with `respond(connection index, request head,
/// port)` and reports every request head on the returned channel. Serves at most `max_connections`.
fn scripted_server(
    max_connections: usize,
    respond: impl Fn(usize, &str, u16) -> String + Send + 'static,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for index in 0..max_connections {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                head.push(byte[0]);
            }
            // A POST announces its body: read it all before answering (an unread body can turn the close into a
            // RST on the client side). The reported text is the head followed by the body.
            let announced = String::from_utf8_lossy(&head)
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            let mut body = vec![0u8; announced];
            if stream.read_exact(&mut body).is_err() {
                body.clear();
            }
            head.extend_from_slice(&body);
            let head = String::from_utf8_lossy(&head).into_owned();
            let answer = respond(index, &head, port);
            tx.send(head).ok();
            stream.write_all(answer.as_bytes()).ok();
        }
    });
    (format!("http://127.0.0.1:{port}"), rx)
}

fn http_answer(status: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    )
}

fn client_at(url: &str) -> Client {
    Client::new(
        ApiBase::parse(url).expect("base"),
        Token::new(SECRET).expect("token"),
    )
    .expect("client")
}

#[tokio::test]
async fn follows_the_link_header_over_three_pages() {
    let names = ["a", "b", "c", "d", "e"];
    let mut state = state_with_token(names.iter().map(|n| repo(n)).collect());
    state.max_per_page = 2;
    let fake = FakeGitHub::start(state).await.expect("fake");

    let repos = client_for(&fake, SECRET)
        .user_repos(&[])
        .await
        .expect("repos");

    let got: Vec<String> = repos.iter().map(|r| r.name.to_string()).collect();
    assert_eq!(got, ["alice/a", "alice/b", "alice/c", "alice/d", "alice/e"]);
    let requests = fake.requests();
    assert_eq!(requests.len(), 3, "one request per page: {requests:?}");
    let queries: Vec<String> = requests
        .iter()
        .map(|r| r.query.clone().unwrap_or_default())
        .collect();
    assert!(
        !queries[0].contains("page=") || queries[0].contains("page=1"),
        "{queries:?}"
    );
    assert!(queries[1].contains("page=2"), "{queries:?}");
    assert!(queries[2].contains("page=3"), "{queries:?}");
    assert!(
        queries.iter().all(|q| q.contains("per_page=")),
        "{queries:?}"
    );
}

#[tokio::test]
async fn decodes_repo_fields_from_the_fake() {
    let mut fork = repo("forked");
    fork.fork = true;
    let mut old = repo("old");
    old.archived = true;
    old.private = true;
    old.description = Some("kept for history".into());
    let fake = FakeGitHub::start(state_with_token(vec![old, fork]))
        .await
        .expect("fake");

    let repos = client_for(&fake, SECRET)
        .user_repos(&[])
        .await
        .expect("repos");

    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0].name.to_string(), "alice/forked");
    assert!(
        repos[0].fork && !repos[0].archived && !repos[0].private && repos[0].description.is_none()
    );
    assert_eq!(repos[1].name.to_string(), "alice/old");
    assert!(repos[1].archived && repos[1].private && !repos[1].fork);
    assert_eq!(repos[1].description.as_deref(), Some("kept for history"));
}

#[tokio::test]
async fn affiliations_are_sent_as_a_filter() {
    let mut collab = Repo::new("bob", "shared");
    collab.access = Access::Collaborator;
    let fake = FakeGitHub::start(state_with_token(vec![repo("mine"), collab]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);

    let owned = client
        .user_repos(&[Affiliation::Owner])
        .await
        .expect("owned");
    assert_eq!(
        owned.iter().map(|r| r.name.to_string()).collect::<Vec<_>>(),
        ["alice/mine"]
    );

    let both = client
        .user_repos(&[
            Affiliation::Owner,
            Affiliation::Collaborator,
            Affiliation::Owner,
        ])
        .await;
    assert_eq!(both.expect("both").len(), 2);
    let query = fake
        .requests()
        .last()
        .and_then(|r| r.query.clone())
        .unwrap_or_default();
    assert!(
        query.contains("affiliation=owner%2Ccollaborator&")
            || query.contains("affiliation=owner,collaborator&"),
        "{query}"
    );

    assert_eq!(client.user_repos(&[]).await.expect("all").len(), 2);
}

#[tokio::test]
async fn sends_bearer_accept_api_version_and_user_agent() {
    let (url, heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", "[]"));

    let repos = client_at(&url).user_repos(&[]).await.expect("repos");

    assert!(repos.is_empty());
    let head = heads
        .recv_timeout(Duration::from_secs(5))
        .expect("request head");
    let lower = head.to_ascii_lowercase();
    assert!(head.starts_with("GET /user/repos?"), "{head}");
    assert!(
        lower.contains(&format!(
            "authorization: bearer {}\r\n",
            SECRET.to_ascii_lowercase()
        )),
        "{head}"
    );
    assert!(
        lower.contains("accept: application/vnd.github+json\r\n"),
        "{head}"
    );
    assert!(
        lower.contains("x-github-api-version: 2022-11-28\r\n"),
        "{head}"
    );
    assert!(
        lower.contains(&format!(
            "user-agent: ghu/{}\r\n",
            env!("CARGO_PKG_VERSION")
        )),
        "{head}"
    );
}

#[tokio::test]
async fn the_fake_receives_the_token_as_bearer() {
    let fake = FakeGitHub::start(state_with_token(vec![repo("a")]))
        .await
        .expect("fake");
    client_for(&fake, SECRET)
        .user_repos(&[])
        .await
        .expect("repos");
    assert_eq!(fake.requests()[0].token.as_deref(), Some(SECRET));
}

#[tokio::test]
async fn a_rejected_token_is_unauthorized() {
    let fake = FakeGitHub::start(state_with_token(vec![repo("a")]))
        .await
        .expect("fake");
    let error = client_for(&fake, "ghp_some_other_token")
        .user_repos(&[])
        .await
        .expect_err("401");
    assert_eq!(error, Error::Unauthorized);
}

#[tokio::test]
async fn injected_401_on_a_later_page_fails_the_whole_call() {
    let mut state = state_with_token(vec![repo("a"), repo("b"), repo("c")]);
    state.max_per_page = 1;
    let fake = FakeGitHub::start(state).await.expect("fake");
    let client = client_for(&fake, SECRET);
    fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Get,
            path: "/user/repos".into(),
            response: FailureResponse::Unauthorized,
            remaining: None,
        });
    });
    assert_eq!(client.user_repos(&[]).await, Err(Error::Unauthorized));
}

#[tokio::test]
async fn rate_limit_reports_the_reset_time() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Get,
            path: "/user/repos".into(),
            response: FailureResponse::RateLimited,
            remaining: None,
        });
    });
    let error = client_for(&fake, SECRET)
        .user_repos(&[])
        .await
        .expect_err("rate limited");
    match &error {
        Error::RateLimited {
            reset_at: Some(reset),
        } => assert!(*reset > 1_700_000_000, "{reset}"),
        other => panic!("expected RateLimited with a reset time, got {other:?}"),
    }
    assert_no_secret(&error);
}

#[tokio::test]
async fn forbidden_keeps_githubs_message() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    fake.with_state(|s| {
        s.failures.push(Failure {
            method: Method::Get,
            path: "/user/repos".into(),
            response: FailureResponse::Forbidden {
                message: "Resource not accessible by personal access token".into(),
            },
            remaining: None,
        });
    });
    let error = client_for(&fake, SECRET)
        .user_repos(&[])
        .await
        .expect_err("403");
    assert_eq!(
        error,
        Error::Forbidden {
            message: "Resource not accessible by personal access token".into()
        }
    );
}

#[tokio::test]
async fn other_statuses_map_to_their_variants() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);
    for (response, check) in [
        (
            FailureResponse::NotFound,
            (|e: &Error| *e == Error::NotFound) as fn(&Error) -> bool,
        ),
        (
            FailureResponse::Unprocessable {
                message: "Nope".into(),
            },
            |e| matches!(e, Error::Unprocessable { message } if message.starts_with("Nope")),
        ),
        (FailureResponse::ServerError, |e| {
            matches!(e, Error::Api { status: 500, .. })
        }),
    ] {
        fake.with_state(|s| {
            s.failures = vec![Failure {
                method: Method::Get,
                path: "/user/repos".into(),
                response,
                remaining: Some(1),
            }];
        });
        let error = client.user_repos(&[]).await.expect_err("failure");
        assert!(check(&error), "{error:?}");
    }
}

#[tokio::test]
async fn no_error_message_contains_the_token() {
    let fake = FakeGitHub::start(state_with_token(vec![repo("a")]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);
    let wrong = client_for(&fake, "ghp_some_other_token");
    assert_no_secret(&wrong.user_repos(&[]).await.expect_err("401"));
    for response in [
        FailureResponse::Unauthorized,
        FailureResponse::RateLimited,
        FailureResponse::Forbidden {
            message: "denied".into(),
        },
        FailureResponse::NotFound,
        FailureResponse::Unprocessable {
            message: "refused".into(),
        },
        FailureResponse::ServerError,
    ] {
        fake.with_state(|s| {
            s.failures = vec![Failure {
                method: Method::Get,
                path: "/user/repos".into(),
                response,
                remaining: Some(1),
            }];
        });
        assert_no_secret(&client.user_repos(&[]).await.expect_err("failure"));
    }
    assert!(
        !format!("{client:?}").contains(SECRET),
        "Debug of the client leaks the token"
    );
    assert!(!format!("{:?}", client.base()).contains(SECRET));
}

#[tokio::test]
async fn an_echoed_token_is_masked_in_error_messages() {
    let body = format!(r#"{{"message":"token {SECRET} is not allowed"}}"#);
    let (url, _heads) = scripted_server(1, move |_, _, _| http_answer("403 Forbidden", "", &body));
    let error = client_at(&url).user_repos(&[]).await.expect_err("403");
    assert_eq!(
        error,
        Error::Forbidden {
            message: "token *** is not allowed".into()
        }
    );
}

#[tokio::test]
async fn a_network_failure_is_reported_without_the_token() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    // The listener is dropped: nothing listens on the port any more.
    let error = client_at(&format!("http://127.0.0.1:{port}"))
        .user_repos(&[])
        .await
        .expect_err("refused");
    assert!(matches!(error, Error::Network { .. }), "{error:?}");
    assert_no_secret(&error);
}

#[tokio::test]
async fn a_next_link_outside_the_base_origin_is_refused_and_never_followed() {
    let (url, heads) = scripted_server(2, |_, _, _| {
        http_answer(
            "200 OK",
            "Link: <https://evil.example.com/user/repos?page=2>; rel=\"next\"\r\n",
            "[]",
        )
    });
    let error = client_at(&url)
        .user_repos(&[])
        .await
        .expect_err("foreign link");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
    assert_no_secret(&error);
    heads
        .recv_timeout(Duration::from_secs(5))
        .expect("first request");
    assert!(
        heads.recv_timeout(Duration::from_millis(300)).is_err(),
        "a second request was sent"
    );
}

#[tokio::test]
async fn a_next_link_on_another_port_of_the_same_host_is_refused() {
    let (url, _heads) = scripted_server(1, |_, _, port| {
        http_answer(
            "200 OK",
            &format!(
                "Link: <http://127.0.0.1:{}/user/repos?page=2>; rel=\"next\"\r\n",
                port.wrapping_add(1)
            ),
            "[]",
        )
    });
    let error = client_at(&url)
        .user_repos(&[])
        .await
        .expect_err("other origin");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_next_link_equal_to_the_current_page_is_refused() {
    let (url, _heads) = scripted_server(5, |_, head, port| {
        let target = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
        http_answer(
            "200 OK",
            &format!("Link: <http://127.0.0.1:{port}{target}>; rel=\"next\"\r\n"),
            "[]",
        )
    });
    let error = client_at(&url).user_repos(&[]).await.expect_err("loop");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_relative_next_link_is_resolved_against_the_current_page() {
    let (url, heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            "Link: </user/repos?page=2&per_page=100>; rel=\"next\"\r\n",
            r#"[{"full_name":"alice/a","description":null,"private":false,"fork":false,"archived":false}]"#,
        ),
        _ => http_answer(
            "200 OK",
            "",
            r#"[{"full_name":"alice/b","description":null,"private":false,"fork":false,"archived":false}]"#,
        ),
    });
    let repos = client_at(&url).user_repos(&[]).await.expect("repos");
    assert_eq!(repos.len(), 2);
    heads.recv_timeout(Duration::from_secs(5)).expect("page 1");
    let second = heads.recv_timeout(Duration::from_secs(5)).expect("page 2");
    assert!(
        second.starts_with("GET /user/repos?page=2&per_page=100 "),
        "{second}"
    );
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (url, heads) = scripted_server(2, |_, _, _| {
        http_answer(
            "302 Found",
            "Location: https://evil.example.com/steal\r\n",
            "",
        )
    });
    let error = client_at(&url).user_repos(&[]).await.expect_err("302");
    assert!(matches!(error, Error::Api { status: 302, .. }), "{error:?}");
    heads
        .recv_timeout(Duration::from_secs(5))
        .expect("first request");
    assert!(
        heads.recv_timeout(Duration::from_millis(300)).is_err(),
        "the redirect was followed"
    );
}

#[tokio::test]
async fn a_2xx_with_invalid_json_is_an_unexpected_response() {
    let (url, _heads) = scripted_server(1, |_, _, _| {
        http_answer("200 OK", "", "<html>not json</html>")
    });
    let error = client_at(&url).user_repos(&[]).await.expect_err("bad body");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[test]
fn a_token_that_cannot_be_a_header_is_a_setup_error_without_the_value() {
    let token = Token::new("ghp_bad\nvalue").expect("token");
    let error = Client::new(ApiBase::default(), token).expect_err("invalid header value");
    assert!(matches!(error, Error::ClientSetup { .. }), "{error:?}");
    assert!(!error.to_string().contains("ghp_bad"));
}

// ---------------------------------------------------------------------------------------------------------------
// #4 / #5 : Client::repo and Client::pulls

fn name(text: &str) -> RepoName {
    text.parse().expect("repo name")
}

fn pull_json(number: u64) -> String {
    format!(
        r#"{{"number":{number},"title":"PR {number}","user":{{"login":"bob"}},"state":"open","draft":false,"updated_at":"2026-01-01T00:00:00Z"}}"#
    )
}

fn repo_json(full_name: &str) -> String {
    format!(
        r#"{{"full_name":"{full_name}","description":null,"private":false,"fork":false,"archived":false}}"#
    )
}

fn request_line(head: &str) -> &str {
    head.lines().next().unwrap_or_default()
}

#[tokio::test]
async fn pulls_sends_state_sort_direction_and_per_page() {
    let (url, heads) = scripted_server(2, |_, _, _| http_answer("200 OK", "", "[]"));
    let client = client_at(&url);

    assert!(
        client
            .pulls(&name("Alice/Notes"), PullFilter::Open)
            .await
            .expect("open")
            .is_empty()
    );
    assert!(
        client
            .pulls(&name("alice/notes"), PullFilter::Closed)
            .await
            .expect("closed")
            .is_empty()
    );

    let open = heads.recv_timeout(Duration::from_secs(5)).expect("first");
    assert_eq!(
        request_line(&open),
        "GET /repos/Alice/Notes/pulls?state=open&sort=updated&direction=desc&per_page=100 HTTP/1.1"
    );
    let closed = heads.recv_timeout(Duration::from_secs(5)).expect("second");
    assert_eq!(
        request_line(&closed),
        "GET /repos/alice/notes/pulls?state=closed&sort=updated&direction=desc&per_page=100 HTTP/1.1"
    );
}

#[tokio::test]
async fn pulls_seen_on_two_pages_are_kept_once_at_their_first_position() {
    let (url, _heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            "Link: </repos/alice/notes/pulls?page=2>; rel=\"next\"\r\n",
            &format!("[{},{}]", pull_json(5), pull_json(4)),
        ),
        _ => http_answer(
            "200 OK",
            "",
            &format!("[{},{}]", pull_json(4), pull_json(3)),
        ),
    });

    let pulls = client_at(&url)
        .pulls(&name("alice/notes"), PullFilter::Open)
        .await
        .expect("pulls");

    let numbers: Vec<u64> = pulls.iter().map(|p| p.number.get()).collect();
    assert_eq!(numbers, [5, 4, 3]);
}

#[tokio::test]
async fn user_repos_seen_twice_are_kept_once_even_if_the_case_differs() {
    let (url, _heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            "Link: </user/repos?page=2>; rel=\"next\"\r\n",
            &format!("[{},{}]", repo_json("alice/a"), repo_json("alice/b")),
        ),
        _ => http_answer(
            "200 OK",
            "",
            &format!("[{},{}]", repo_json("Alice/B"), repo_json("alice/c")),
        ),
    });

    let repos = client_at(&url).user_repos(&[]).await.expect("repos");

    let got: Vec<String> = repos.iter().map(|r| r.name.to_string()).collect();
    assert_eq!(got, ["alice/a", "alice/b", "alice/c"]);
}

#[tokio::test]
async fn repo_requests_the_repo_path_and_returns_the_canonical_name() {
    let (url, heads) = scripted_server(1, |_, _, _| {
        http_answer("200 OK", "", &repo_json("Alice/Notes"))
    });

    let repo = client_at(&url)
        .repo(&name("alice/notes"))
        .await
        .expect("repo");

    assert_eq!(repo.name.to_string(), "Alice/Notes");
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert_eq!(request_line(&head), "GET /repos/alice/notes HTTP/1.1");
}

#[tokio::test]
async fn repo_redirect_is_an_api_error_with_its_status_and_is_not_followed() {
    let (url, heads) = scripted_server(2, |_, _, port| {
        http_answer(
            "301 Moved Permanently",
            &format!("Location: http://127.0.0.1:{port}/repositories/1\r\n"),
            r#"{"message":"Moved Permanently"}"#,
        )
    });

    let result = client_at(&url).repo(&name("alice/old")).await;

    assert!(
        matches!(result, Err(Error::Api { status: 301, .. })),
        "{result:?}"
    );
    heads.recv_timeout(Duration::from_secs(5)).expect("first");
    assert!(heads.recv_timeout(Duration::from_millis(300)).is_err());
}

#[tokio::test]
async fn repo_and_pulls_with_an_undecodable_answer_are_unexpected_and_show_no_token() {
    let body = format!("not json {SECRET}");
    let (url, _heads) = scripted_server(2, move |_, _, _| http_answer("200 OK", "", &body));
    let client = client_at(&url);

    let repo = client.repo(&name("alice/notes")).await.expect_err("repo");
    let pulls = client
        .pulls(&name("alice/notes"), PullFilter::Open)
        .await
        .expect_err("pulls");

    for error in [repo, pulls] {
        assert!(
            matches!(error, Error::UnexpectedResponse { .. }),
            "{error:?}"
        );
        assert_no_secret(&error);
    }
}

#[tokio::test]
async fn repo_and_pulls_errors_of_the_server_never_contain_the_token() {
    let (url, _heads) = scripted_server(2, |_, _, _| {
        http_answer(
            "403 Forbidden",
            "",
            &format!(r#"{{"message":"nope {SECRET}"}}"#),
        )
    });
    let client = client_at(&url);

    let repo = client.repo(&name("alice/notes")).await.expect_err("repo");
    let pulls = client
        .pulls(&name("alice/notes"), PullFilter::Closed)
        .await
        .expect_err("pulls");

    for error in [repo, pulls] {
        assert!(matches!(error, Error::Forbidden { .. }), "{error:?}");
        assert_no_secret(&error);
    }
}

// --- Against gh-fake. These need the repo and pulls routes of dev[fake]: they pass only once it is merged. ---

fn pull(number: u64, state: PullState, updated_at: &str) -> Pull {
    let mut pull = Pull::new("alice", "notes", number, &format!("PR {number}"), "bob");
    pull.state = state;
    pull.updated_at = updated_at.to_owned();
    pull
}

#[tokio::test]
async fn fake_repo_returns_the_repo_with_its_canonical_name() {
    let mut mine = repo("Notes");
    mine.description = Some("my notes".into());
    let fake = FakeGitHub::start(state_with_token(vec![mine]))
        .await
        .expect("fake");

    let found = client_for(&fake, SECRET)
        .repo(&name("ALICE/notes"))
        .await
        .expect("repo");

    assert_eq!(found.name.to_string(), "alice/Notes");
    assert_eq!(found.description.as_deref(), Some("my notes"));
}

#[tokio::test]
async fn fake_repo_of_an_unlisted_public_repo_is_found_and_a_missing_one_is_not_found() {
    let mut other = Repo::new("bob", "public");
    other.access = Access::NoAffiliation;
    let fake = FakeGitHub::start(state_with_token(vec![other]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);

    assert_eq!(
        client
            .repo(&name("bob/public"))
            .await
            .expect("found")
            .name
            .to_string(),
        "bob/public"
    );
    let missing = client.repo(&name("bob/missing")).await;
    assert_eq!(missing, Err(Error::NotFound));
}

#[tokio::test]
async fn fake_pulls_follow_every_page_and_map_states_draft_and_ghost() {
    let mut state = state_with_token(vec![repo("notes")]);
    state.max_per_page = 2;
    let mut draft = pull(3, PullState::Open, "2026-01-03T00:00:00Z");
    draft.draft = true;
    let mut ghost = pull(2, PullState::Open, "2026-01-02T00:00:00Z");
    ghost.author = None;
    state.pulls = vec![
        pull(1, PullState::Open, "2026-01-01T00:00:00Z"),
        ghost,
        draft,
        pull(4, PullState::Open, "2026-01-04T00:00:00Z"),
        pull(5, PullState::Merged, "2026-01-05T00:00:00Z"),
        pull(6, PullState::Closed, "2026-01-06T00:00:00Z"),
    ];
    let fake = FakeGitHub::start(state).await.expect("fake");
    let client = client_for(&fake, SECRET);

    let open = client
        .pulls(&name("alice/notes"), PullFilter::Open)
        .await
        .expect("open");
    let numbers: Vec<u64> = open.iter().map(|p| p.number.get()).collect();
    assert_eq!(numbers, [4, 3, 2, 1], "most recently updated first");
    assert_eq!(open[1].state, PrState::Open { draft: true });
    assert_eq!(open[2].author, "ghost");
    assert_eq!(open[0].author, "bob");
    assert_eq!(open[0].updated_at.date(), "2026-01-04");
    assert_eq!(fake.requests().len(), 2, "two pages of two");

    let closed = client
        .pulls(&name("alice/notes"), PullFilter::Closed)
        .await
        .expect("closed");
    let states: Vec<(u64, PrState)> = closed.iter().map(|p| (p.number.get(), p.state)).collect();
    assert_eq!(states, [(6, PrState::Closed), (5, PrState::Merged)]);
}

#[tokio::test]
async fn fake_pulls_of_a_missing_repo_are_not_found() {
    let fake = FakeGitHub::start(state_with_token(vec![repo("notes")]))
        .await
        .expect("fake");

    let result = client_for(&fake, SECRET)
        .pulls(&name("alice/other"), PullFilter::Open)
        .await;

    assert_eq!(result, Err(Error::NotFound));
}

#[tokio::test]
async fn fake_repo_and_pulls_with_a_bad_token_are_unauthorized_without_the_token() {
    let fake = FakeGitHub::start(state_with_token(vec![repo("notes")]))
        .await
        .expect("fake");
    let client = client_for(&fake, "wrong-token");

    let repo = client.repo(&name("alice/notes")).await.expect_err("repo");
    let pulls = client
        .pulls(&name("alice/notes"), PullFilter::Open)
        .await
        .expect_err("pulls");

    assert_eq!(repo, Error::Unauthorized);
    assert_eq!(pulls, Error::Unauthorized);
}

// ---- ADR 0003 (#6-#10): detail, reviews, comments, files, user, and the two publishing routes ----
//
// Successes are served by the scripted server from the documented examples (the fake has no routes for them in
// this wave); failures come from the fake's injected `Failure`, which is applied before routing; the JSON bodies
// of the POSTs are read from the fake's `requests()`, recorded before routing.
//
// Fixtures are copies of the OpenAPI examples (components.examples, rest-api-description): pull.json = pull-request,
// reviews.json = pull-request-review-items, review_created.json = pull-request-review, issue_comments.json =
// issue-comment-items, comment_created.json = issue-comment, files.json = diff-entry-items, user.json =
// private-user-response-with-public-and-private-profile-information.

const PULL_EXAMPLE: &str = include_str!("fixtures/pull.json");
const REVIEWS_EXAMPLE: &str = include_str!("fixtures/reviews.json");
const REVIEW_CREATED_EXAMPLE: &str = include_str!("fixtures/review_created.json");
const COMMENTS_EXAMPLE: &str = include_str!("fixtures/issue_comments.json");
const COMMENT_CREATED_EXAMPLE: &str = include_str!("fixtures/comment_created.json");
const FILES_EXAMPLE: &str = include_str!("fixtures/files.json");
const USER_EXAMPLE: &str = include_str!("fixtures/user.json");

const HEAD_SHA: &str = "6dcb09b5b57875f334f61aebed695e2e4193db5e";

fn pr(number: u64) -> PrNumber {
    PrNumber::new(number).expect("number")
}

fn sha() -> CommitSha {
    CommitSha::parse(HEAD_SHA).expect("sha")
}

fn text(chars: usize) -> MessageBody {
    MessageBody::new(&"é".repeat(chars)).expect("body")
}

fn review_json(id: u64, state: &str) -> String {
    format!(
        r#"{{"id":{id},"user":{{"login":"carol"}},"body":"b{id}","state":"{state}","submitted_at":"2026-03-04T05:06:07Z"}}"#
    )
}

fn comment_json(id: u64) -> String {
    format!(
        r#"{{"id":{id},"user":{{"login":"carol"}},"body":"c{id}","created_at":"2026-03-04T05:06:07Z"}}"#
    )
}

fn file_json(filename: &str) -> String {
    format!(r#"{{"filename":"{filename}","status":"modified","additions":1,"deletions":0}}"#)
}

fn next_page(path: &str) -> String {
    format!("Link: <{path}?page=2>; rel=\"next\"\r\n")
}

#[test]
fn commit_sha_accepts_only_lowercase_hex_of_40_or_64_digits() {
    let sha64 = "ab".repeat(32);
    for ok in [HEAD_SHA, sha64.as_str(), &"0".repeat(40)] {
        let sha = CommitSha::parse(ok).expect(ok);
        assert_eq!(sha.as_str(), ok);
        assert_eq!(sha.short(), &ok[..7]);
    }
    let upper = HEAD_SHA.to_ascii_uppercase();
    let spaced = format!(" {HEAD_SHA}");
    let non_hex = format!("g{}", &HEAD_SHA[1..]);
    let accented = format!("é{}", &HEAD_SHA[2..]);
    for bad in [
        "",
        "6dcb09b",
        &HEAD_SHA[..39],
        &format!("{HEAD_SHA}0"),
        &"a".repeat(63),
        &"a".repeat(65),
        upper.as_str(),
        spaced.as_str(),
        non_hex.as_str(),
        accented.as_str(),
    ] {
        assert_eq!(CommitSha::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn message_body_refuses_blank_and_more_than_the_limit() {
    assert_eq!(MAX_BODY_CHARS, 65_536);
    let blank = Error::InvalidBody {
        problem: BodyProblem::Blank,
    };
    for text in ["", " ", "\n\t \r\n", "\u{a0}\u{2003}"] {
        assert_eq!(MessageBody::new(text).expect_err(text), blank, "{text:?}");
    }
    // Blank wins over length: a huge run of spaces is blank, not too long.
    assert_eq!(
        MessageBody::new(&" ".repeat(70_000)).expect_err("blank"),
        blank
    );

    assert_eq!(MessageBody::new("a").expect("1").as_str(), "a");
    // The text is kept as given, never trimmed.
    assert_eq!(
        MessageBody::new(" a\r\n").expect("untrimmed").as_str(),
        " a\r\n"
    );
    // The limit counts characters, not bytes: 65 536 two-byte characters are accepted.
    assert_eq!(text(65_536).as_str().chars().count(), 65_536);
    assert_eq!(
        MessageBody::new(&"é".repeat(65_537)).expect_err("too long"),
        Error::InvalidBody {
            problem: BodyProblem::TooLong { chars: 65_537 }
        }
    );
    assert!(MessageBody::new(&"a".repeat(65_536)).is_ok());
}

#[test]
fn message_body_debug_shows_the_length_never_the_text() {
    let body = MessageBody::new("très secret é").expect("body");
    let debug = format!("{body:?}");
    let pretty = format!("{body:#?}");
    for shown in [&debug, &pretty] {
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("13"), "{shown}");
    }
    let request = ReviewRequest::RequestChanges { body };
    assert!(!format!("{request:?}").contains("secret"));
}

#[test]
fn a_review_request_knows_its_verdict() {
    let body = MessageBody::new("x").expect("body");
    assert_eq!(
        ReviewRequest::Approve { body: None }.verdict(),
        Verdict::Approve
    );
    assert_eq!(
        ReviewRequest::Approve {
            body: Some(body.clone())
        }
        .verdict(),
        Verdict::Approve
    );
    assert_eq!(
        ReviewRequest::RequestChanges { body }.verdict(),
        Verdict::RequestChanges
    );
}

#[tokio::test]
async fn current_user_requests_user_and_decodes_the_example() {
    let (url, heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", USER_EXAMPLE));
    let user = client_at(&url).current_user().await.expect("user");
    assert_eq!(user.login, "octocat");
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(head.starts_with("GET /user HTTP/1.1\r\n"), "{head}");
}

#[tokio::test]
async fn pull_requests_its_path_and_decodes_the_example() {
    let (url, heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", PULL_EXAMPLE));
    let pull = client_at(&url)
        .pull(&name("octocat/Hello-World"), pr(1347))
        .await
        .expect("pull");
    assert_eq!(pull.number.get(), 1347);
    assert_eq!(pull.head_sha.as_str(), HEAD_SHA);
    assert_eq!(pull.mergeable, Mergeable::Yes);
    assert_eq!(pull.state, PrState::Open { draft: false });
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(
        head.starts_with("GET /repos/octocat/Hello-World/pulls/1347 HTTP/1.1\r\n"),
        "{head}"
    );
}

/// The documented pull request with one field edited, as a body.
fn pull_edited(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut value: serde_json::Value = serde_json::from_str(PULL_EXAMPLE).expect("example is JSON");
    edit(&mut value);
    value.to_string()
}

#[tokio::test]
async fn pull_with_a_bad_sha_or_unknown_state_is_unexpected_and_shows_no_token() {
    for bad in [
        pull_edited(|pull| pull["head"]["sha"] = "nothex".into()),
        pull_edited(|pull| pull["state"] = "weird".into()),
        "[]".to_owned(),
    ] {
        let (url, _heads) = scripted_server(1, move |_, _, _| http_answer("200 OK", "", &bad));
        let error = client_at(&url)
            .pull(&name("a/b"), pr(1))
            .await
            .expect_err("bad");
        assert!(
            matches!(error, Error::UnexpectedResponse { .. }),
            "{error:?}"
        );
        assert_no_secret(&error);
    }
}

#[tokio::test]
async fn reviews_follow_pages_drop_pending_and_keep_the_first_of_a_repeated_id() {
    let (url, heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            &next_page("/repos/alice/notes/pulls/7/reviews"),
            &format!(
                "[{},{}]",
                review_json(1, "APPROVED"),
                review_json(2, "PENDING")
            ),
        ),
        _ => http_answer(
            "200 OK",
            "",
            &format!(
                "[{},{},{}]",
                review_json(1, "CHANGES_REQUESTED"),
                review_json(3, "COMMENTED"),
                review_json(4, "DISMISSED")
            ),
        ),
    });
    let reviews = client_at(&url)
        .reviews(&name("alice/notes"), pr(7))
        .await
        .expect("reviews");
    let got: Vec<(u64, ReviewState)> = reviews.iter().map(|r| (r.id, r.state)).collect();
    assert_eq!(
        got,
        [
            (1, ReviewState::Approved),
            (3, ReviewState::Commented),
            (4, ReviewState::Dismissed)
        ]
    );
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(
        request_line(&head).starts_with("GET /repos/alice/notes/pulls/7/reviews?per_page=100 "),
        "{head}"
    );
}

#[tokio::test]
async fn reviews_decode_the_example_and_an_unknown_state_is_unexpected() {
    let (url, _heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", REVIEWS_EXAMPLE));
    let reviews = client_at(&url)
        .reviews(&name("a/b"), pr(1))
        .await
        .expect("reviews");
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].state, ReviewState::Approved);

    let weird = REVIEWS_EXAMPLE.replace("APPROVED", "WEIRD");
    let (url, _heads) = scripted_server(1, move |_, _, _| http_answer("200 OK", "", &weird));
    let error = client_at(&url)
        .reviews(&name("a/b"), pr(1))
        .await
        .expect_err("unknown state");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn comments_follow_pages_and_keep_the_first_of_a_repeated_id() {
    let (url, heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            &next_page("/repos/alice/notes/issues/7/comments"),
            &format!("[{},{}]", comment_json(1), comment_json(2)),
        ),
        _ => http_answer(
            "200 OK",
            "",
            &format!("[{},{}]", comment_json(2), comment_json(3)),
        ),
    });
    let comments = client_at(&url)
        .comments(&name("alice/notes"), pr(7))
        .await
        .expect("comments");
    let ids: Vec<u64> = comments.iter().map(|c| c.id).collect();
    assert_eq!(ids, [1, 2, 3]);
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(
        request_line(&head).starts_with("GET /repos/alice/notes/issues/7/comments?per_page=100 "),
        "{head}"
    );
}

#[tokio::test]
async fn comments_decode_the_documented_example() {
    let (url, _heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", COMMENTS_EXAMPLE));
    let comments = client_at(&url)
        .comments(&name("a/b"), pr(1))
        .await
        .expect("comments");
    assert_eq!(comments.len(), 1);
    assert_eq!(
        (comments[0].author.as_str(), comments[0].body.as_str()),
        ("octocat", "Me too")
    );
}

#[tokio::test]
async fn pull_files_follow_pages_and_keep_the_first_of_a_repeated_filename() {
    let (url, heads) = scripted_server(2, |index, _, _| match index {
        0 => http_answer(
            "200 OK",
            &next_page("/repos/alice/notes/pulls/7/files"),
            &format!("[{},{}]", file_json("a.rs"), file_json("b.rs")),
        ),
        _ => http_answer(
            "200 OK",
            "",
            &format!("[{},{}]", file_json("b.rs"), file_json("c.rs")),
        ),
    });
    let files = client_at(&url)
        .pull_files(&name("alice/notes"), pr(7))
        .await
        .expect("files");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.rs", "b.rs", "c.rs"]);
    assert!(
        files
            .iter()
            .all(|f| f.status == FileStatus::Modified && f.patch.is_none())
    );
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(
        request_line(&head).starts_with("GET /repos/alice/notes/pulls/7/files?per_page=100 "),
        "{head}"
    );
}

#[tokio::test]
async fn pull_files_decode_the_example_and_an_unknown_status_is_unexpected() {
    let (url, _heads) = scripted_server(1, |_, _, _| http_answer("200 OK", "", FILES_EXAMPLE));
    let files = client_at(&url)
        .pull_files(&name("a/b"), pr(1))
        .await
        .expect("files");
    assert_eq!(
        (files[0].path.as_str(), files[0].status),
        ("file1.txt", FileStatus::Added)
    );
    assert!(files[0].patch.is_some());

    let weird = FILES_EXAMPLE.replace(r#""added""#, r#""weird""#);
    let (url, _heads) = scripted_server(1, move |_, _, _| http_answer("200 OK", "", &weird));
    let error = client_at(&url)
        .pull_files(&name("a/b"), pr(1))
        .await
        .expect_err("unknown status");
    assert!(
        matches!(error, Error::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn files_with_an_empty_patch_or_none_have_no_patch() {
    let body = r#"[{"filename":"bin.png","status":"added","additions":0,"deletions":0},
        {"filename":"r.txt","previous_filename":"q.txt","status":"renamed","additions":0,"deletions":0,"patch":""}]"#;
    let (url, _heads) = scripted_server(1, move |_, _, _| http_answer("200 OK", "", body));
    let files = client_at(&url)
        .pull_files(&name("a/b"), pr(1))
        .await
        .expect("files");
    assert_eq!(files[0].patch, None);
    assert_eq!(files[1].patch, None);
    assert_eq!(files[1].previous_path.as_deref(), Some("q.txt"));
}

#[tokio::test]
async fn post_comment_sends_the_body_and_decodes_a_201() {
    let (url, heads) = scripted_server(1, |_, _, _| {
        http_answer("201 Created", "", COMMENT_CREATED_EXAMPLE)
    });
    let body = MessageBody::new("Me too\nline é \"quoted\"").expect("body");
    let comment = client_at(&url)
        .post_comment(&name("alice/notes"), pr(7), &body)
        .await
        .expect("comment");
    assert_eq!((comment.id, comment.body.as_str()), (1, "Me too"));
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    let (line, rest) = head.split_once("\r\n").expect("line");
    assert_eq!(line, "POST /repos/alice/notes/issues/7/comments HTTP/1.1");
    let (headers, sent) = rest.split_once("\r\n\r\n").expect("body");
    let lower = headers.to_ascii_lowercase();
    assert!(
        lower.contains("content-type: application/json"),
        "{headers}"
    );
    assert!(lower.contains("authorization: bearer "), "{headers}");
    let json: serde_json::Value = serde_json::from_str(sent).expect("json body");
    assert_eq!(
        json,
        serde_json::json!({"body": "Me too\nline é \"quoted\""})
    );
}

#[tokio::test]
async fn post_comment_expects_201_and_a_decodable_comment() {
    for (status, body) in [
        ("200 OK", COMMENT_CREATED_EXAMPLE),
        ("204 No Content", ""),
        ("201 Created", "{}"),
        ("201 Created", "not json"),
    ] {
        let (url, _heads) = scripted_server(1, move |_, _, _| http_answer(status, "", body));
        let error = client_at(&url)
            .post_comment(&name("a/b"), pr(1), &text(3))
            .await
            .expect_err(status);
        assert!(
            matches!(error, Error::UnexpectedResponse { .. }),
            "{status}: {error:?}"
        );
        assert_no_secret(&error);
    }
}

#[tokio::test]
async fn post_review_decodes_a_200_and_sends_the_body_of_each_request() {
    let (url, heads) = scripted_server(1, |_, _, _| {
        http_answer("200 OK", "", REVIEW_CREATED_EXAMPLE)
    });
    let request = ReviewRequest::RequestChanges {
        body: MessageBody::new("fix it").expect("body"),
    };
    let review = client_at(&url)
        .post_review(&name("alice/notes"), pr(7), &sha(), &request)
        .await
        .expect("review");
    assert_eq!(
        (review.id, review.state),
        (80, ReviewState::ChangesRequested)
    );
    let head = heads.recv_timeout(Duration::from_secs(5)).expect("head");
    assert!(
        head.starts_with("POST /repos/alice/notes/pulls/7/reviews HTTP/1.1\r\n"),
        "{head}"
    );
    let sent = head.split_once("\r\n\r\n").expect("body").1;
    let json: serde_json::Value = serde_json::from_str(sent).expect("json");
    assert_eq!(
        json,
        serde_json::json!({"event": "REQUEST_CHANGES", "body": "fix it", "commit_id": HEAD_SHA})
    );
}

#[tokio::test]
async fn post_review_expects_200_and_a_submitted_review() {
    let pending = review_json(5, "PENDING");
    for (status, body) in [
        ("201 Created", REVIEW_CREATED_EXAMPLE.to_owned()),
        ("200 OK", pending),
        ("200 OK", "{}".to_owned()),
    ] {
        let label = format!("{status} {body}");
        let (url, _heads) = scripted_server(1, move |_, _, _| http_answer(status, "", &body));
        let error = client_at(&url)
            .post_review(
                &name("a/b"),
                pr(1),
                &sha(),
                &ReviewRequest::Approve { body: None },
            )
            .await
            .expect_err(&label);
        assert!(
            matches!(error, Error::UnexpectedResponse { .. }),
            "{label}: {error:?}"
        );
        assert_no_secret(&error);
    }
}

fn post_requests(fake: &FakeGitHub) -> Vec<gh_fake::RecordedRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.method == Method::Post)
        .collect()
}

#[tokio::test]
async fn the_fake_records_the_json_body_of_a_comment() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    // The fake has no route for it in this wave (404): only what the client sent is checked.
    let _ = client_for(&fake, SECRET)
        .post_comment(
            &name("alice/notes"),
            pr(7),
            &MessageBody::new("hello").expect("body"),
        )
        .await;
    let posts = post_requests(&fake);
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].path, "/repos/alice/notes/issues/7/comments");
    assert_eq!(posts[0].body, Some(serde_json::json!({"body": "hello"})));
}

#[tokio::test]
async fn post_review_bodies_carry_event_commit_id_and_omit_a_missing_body() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);
    let words = MessageBody::new("looks good").expect("body");
    let requests = [
        ReviewRequest::Approve { body: None },
        ReviewRequest::Approve {
            body: Some(words.clone()),
        },
        ReviewRequest::RequestChanges { body: words },
    ];
    for request in &requests {
        // 404 from the fake in this wave: only what the client sent is checked.
        let _ = client
            .post_review(&name("alice/notes"), pr(7), &sha(), request)
            .await;
    }
    let posts = post_requests(&fake);
    assert_eq!(posts.len(), 3, "{posts:?}");
    assert!(
        posts
            .iter()
            .all(|p| p.path == "/repos/alice/notes/pulls/7/reviews")
    );
    assert_eq!(
        posts[0].body,
        Some(serde_json::json!({"event": "APPROVE", "commit_id": HEAD_SHA}))
    );
    assert!(
        posts[0]
            .body
            .as_ref()
            .is_some_and(|b| b.get("body").is_none()),
        "body must be omitted, not null: {:?}",
        posts[0].body
    );
    assert_eq!(
        posts[1].body,
        Some(serde_json::json!({"event": "APPROVE", "body": "looks good", "commit_id": HEAD_SHA}))
    );
    assert_eq!(
        posts[2].body,
        Some(
            serde_json::json!({"event": "REQUEST_CHANGES", "body": "looks good", "commit_id": HEAD_SHA})
        )
    );
}

#[tokio::test]
async fn a_long_body_is_sent_whole() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    let _ = client_for(&fake, SECRET)
        .post_comment(&name("alice/notes"), pr(1), &text(MAX_BODY_CHARS))
        .await;
    let posts = post_requests(&fake);
    let sent = posts[0]
        .body
        .as_ref()
        .and_then(|b| b["body"].as_str())
        .expect("body");
    assert_eq!(sent.chars().count(), MAX_BODY_CHARS);
}

/// Failures injected by the fake on every new route: (label, method, path).
fn new_routes() -> Vec<(&'static str, Method, &'static str)> {
    vec![
        ("current_user", Method::Get, "/user"),
        ("pull", Method::Get, "/repos/alice/notes/pulls/7"),
        ("reviews", Method::Get, "/repos/alice/notes/pulls/7/reviews"),
        (
            "comments",
            Method::Get,
            "/repos/alice/notes/issues/7/comments",
        ),
        (
            "pull_files",
            Method::Get,
            "/repos/alice/notes/pulls/7/files",
        ),
        (
            "post_comment",
            Method::Post,
            "/repos/alice/notes/issues/7/comments",
        ),
        (
            "post_review",
            Method::Post,
            "/repos/alice/notes/pulls/7/reviews",
        ),
    ]
}

async fn call(client: &Client, label: &str) -> Result<(), Error> {
    let repo = name("alice/notes");
    let body = MessageBody::new("text").expect("body");
    let approve = ReviewRequest::Approve { body: None };
    match label {
        "current_user" => client.current_user().await.map(drop),
        "pull" => client.pull(&repo, pr(7)).await.map(drop),
        "reviews" => client.reviews(&repo, pr(7)).await.map(drop),
        "comments" => client.comments(&repo, pr(7)).await.map(drop),
        "pull_files" => client.pull_files(&repo, pr(7)).await.map(drop),
        "post_comment" => client.post_comment(&repo, pr(7), &body).await.map(drop),
        "post_review" => client
            .post_review(&repo, pr(7), &sha(), &approve)
            .await
            .map(drop),
        other => panic!("unknown route {other}"),
    }
}

#[tokio::test]
async fn every_new_route_classifies_401_403_404_422_429_and_500_without_the_token() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);
    for (label, method, path) in new_routes() {
        for (response, check) in [
            (
                FailureResponse::Unauthorized,
                (|e: &Error| *e == Error::Unauthorized) as fn(&Error) -> bool,
            ),
            (
                FailureResponse::Forbidden {
                    message: "Repository was archived so is read-only.".into(),
                },
                |e| matches!(e, Error::Forbidden { message } if message == "Repository was archived so is read-only."),
            ),
            (FailureResponse::NotFound, |e| *e == Error::NotFound),
            (
                FailureResponse::Unprocessable {
                    message: "Validation Failed".into(),
                },
                |e| matches!(e, Error::Unprocessable { message } if message.starts_with("Validation Failed")),
            ),
            (FailureResponse::RateLimited, |e| {
                matches!(e, Error::RateLimited { reset_at: Some(_) })
            }),
            (FailureResponse::ServerError, |e| {
                matches!(e, Error::Api { status: 500, .. })
            }),
        ] {
            fake.with_state(|s| {
                s.failures = vec![Failure {
                    method,
                    path: path.into(),
                    response: response.clone(),
                    remaining: Some(1),
                }];
            });
            let error = call(&client, label).await.expect_err(label);
            assert!(check(&error), "{label} {response:?}: {error:?}");
            assert_no_secret(&error);
        }
    }
}

#[tokio::test]
async fn a_failed_publication_is_not_retried() {
    let fake = FakeGitHub::start(state_with_token(vec![]))
        .await
        .expect("fake");
    let client = client_for(&fake, SECRET);
    for (label, path) in [
        ("post_comment", "/repos/alice/notes/issues/7/comments"),
        ("post_review", "/repos/alice/notes/pulls/7/reviews"),
    ] {
        fake.with_state(|s| {
            s.failures = vec![Failure {
                method: Method::Post,
                path: path.into(),
                response: FailureResponse::ServerError,
                remaining: None,
            }];
        });
        let before = post_requests(&fake).len();
        assert!(call(&client, label).await.is_err());
        assert_eq!(post_requests(&fake).len(), before + 1, "{label}");
    }
}

#[tokio::test]
async fn an_echoed_token_is_masked_in_publication_errors() {
    for (status, kind) in [
        ("403 Forbidden", "forbidden"),
        ("422 Unprocessable Entity", "unprocessable"),
    ] {
        let body = format!(r#"{{"message":"token {SECRET} refused","errors":["{SECRET}"]}}"#);
        let (url, _heads) = scripted_server(2, move |_, _, _| http_answer(status, "", &body));
        let client = client_at(&url);
        let comment = client
            .post_comment(&name("a/b"), pr(1), &text(2))
            .await
            .expect_err(kind);
        let review = client
            .post_review(
                &name("a/b"),
                pr(1),
                &sha(),
                &ReviewRequest::Approve { body: None },
            )
            .await
            .expect_err(kind);
        for error in [comment, review] {
            assert_no_secret(&error);
            assert!(error.to_string().contains("***"), "{kind}: {error}");
        }
    }
}

#[tokio::test]
async fn a_network_failure_on_publication_is_a_network_error_without_the_token() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let client = client_at(&format!("http://127.0.0.1:{port}"));
    let error = client
        .post_comment(&name("a/b"), pr(1), &text(2))
        .await
        .expect_err("refused");
    assert!(matches!(error, Error::Network { .. }), "{error:?}");
    assert_no_secret(&error);
}
