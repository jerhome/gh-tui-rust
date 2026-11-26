//! Mapping of an HTTP error answer to [`Error`].

use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::Error;

/// Longest GitHub message kept in an error, in characters (the body comes from the network).
const MAX_MESSAGE_CHARS: usize = 300;

/// Classifies a non-2xx answer (ticket #2: 401, 403, 429, 404, 422, anything else).
///
/// - 401: [`Error::Unauthorized`].
/// - 403 or 429 with `x-ratelimit-remaining: 0`, or whose message mentions a secondary rate limit:
///   [`Error::RateLimited`] with `x-ratelimit-reset` when it is a number.
/// - other 403: [`Error::Forbidden`]; 404: [`Error::NotFound`]; 422: [`Error::Unprocessable`]; the rest, including a
///   429 without a rate-limit marker: [`Error::Api`].
pub(crate) fn classify(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> Error {
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let message = json
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or_default();
    match status.as_u16() {
        401 => Error::Unauthorized,
        403 | 429 if is_rate_limited(headers, message) => Error::RateLimited {
            reset_at: header_u64(headers, "x-ratelimit-reset"),
        },
        403 => Error::Forbidden {
            message: truncate(message),
        },
        404 => Error::NotFound,
        422 => Error::Unprocessable {
            message: truncate(&unprocessable_message(&json, message)),
        },
        code => Error::Api {
            status: code,
            message: truncate(message),
        },
    }
}

fn is_rate_limited(headers: &HeaderMap, message: &str) -> bool {
    header_u64(headers, "x-ratelimit-remaining") == Some(0)
        || message
            .to_ascii_lowercase()
            .contains("secondary rate limit")
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// `message` followed by the `errors[]` texts (plain strings, or objects with a `message`), joined with `; `.
fn unprocessable_message(json: &serde_json::Value, message: &str) -> String {
    let details = json
        .get("errors")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .as_str()
                .or_else(|| entry.get("message").and_then(|m| m.as_str()))
        });
    std::iter::once(message)
        .chain(details)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn truncate(message: &str) -> String {
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// Replaces every occurrence of `secret` in the texts carried by `error` by `***` (defence in depth: GitHub never
/// echoes the token, but a proxy or a fake might, and the contract is that no error ever shows it).
pub(crate) fn redact(error: Error, secret: &str) -> Error {
    let mask = |text: String| text.replace(secret, "***");
    match error {
        Error::Network { message } => Error::Network {
            message: mask(message),
        },
        Error::Forbidden { message } => Error::Forbidden {
            message: mask(message),
        },
        Error::Unprocessable { message } => Error::Unprocessable {
            message: mask(message),
        },
        Error::Api { status, message } => Error::Api {
            status,
            message: mask(message),
        },
        Error::UnexpectedResponse { message } => Error::UnexpectedResponse {
            message: mask(message),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    use super::{classify, redact};
    use crate::Error;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_static(name),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn run(status: u16, pairs: &[(&'static str, &str)], body: &str) -> Error {
        classify(
            StatusCode::from_u16(status).unwrap(),
            &headers(pairs),
            body.as_bytes(),
        )
    }

    #[test]
    fn unauthorized() {
        assert_eq!(
            run(401, &[], r#"{"message":"Bad credentials"}"#),
            Error::Unauthorized
        );
        assert_eq!(run(401, &[], ""), Error::Unauthorized);
    }

    #[test]
    fn primary_rate_limit_carries_the_reset_time() {
        let h = [
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1893456000"),
        ];
        let body = r#"{"message":"API rate limit exceeded for user ID 1."}"#;
        assert_eq!(
            run(403, &h, body),
            Error::RateLimited {
                reset_at: Some(1_893_456_000)
            }
        );
        assert_eq!(
            run(429, &h, body),
            Error::RateLimited {
                reset_at: Some(1_893_456_000)
            }
        );
    }

    #[test]
    fn rate_limit_without_a_usable_reset_has_none() {
        assert_eq!(
            run(403, &[("x-ratelimit-remaining", "0")], "{}"),
            Error::RateLimited { reset_at: None }
        );
        let h = [
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "soon"),
        ];
        assert_eq!(run(403, &h, "{}"), Error::RateLimited { reset_at: None });
    }

    #[test]
    fn secondary_rate_limit_is_recognised_by_its_message() {
        let body =
            r#"{"message":"You have exceeded a Secondary Rate Limit. Please wait a few minutes."}"#;
        assert_eq!(
            run(403, &[("x-ratelimit-remaining", "4000")], body),
            Error::RateLimited { reset_at: None }
        );
        assert_eq!(run(429, &[], body), Error::RateLimited { reset_at: None });
    }

    #[test]
    fn forbidden_with_quota_left_keeps_github_message() {
        let h = [("x-ratelimit-remaining", "4999")];
        assert_eq!(
            run(
                403,
                &h,
                r#"{"message":"Resource not accessible by personal access token"}"#
            ),
            Error::Forbidden {
                message: "Resource not accessible by personal access token".into()
            }
        );
        assert_eq!(
            run(403, &[], "not json"),
            Error::Forbidden {
                message: String::new()
            }
        );
    }

    #[test]
    fn a_429_without_marker_is_a_plain_api_error() {
        assert_eq!(
            run(429, &[], r#"{"message":"Slow down"}"#),
            Error::Api {
                status: 429,
                message: "Slow down".into()
            }
        );
    }

    #[test]
    fn not_found() {
        assert_eq!(run(404, &[], r#"{"message":"Not Found"}"#), Error::NotFound);
    }

    #[test]
    fn unprocessable_joins_message_and_error_entries() {
        let body = r#"{"message":"Validation Failed","errors":[{"resource":"Review","code":"custom","message":"Pending review exists"},"plain text",{"code":"missing"}]}"#;
        assert_eq!(
            run(422, &[], body),
            Error::Unprocessable {
                message: "Validation Failed; Pending review exists; plain text".into()
            }
        );
        assert_eq!(
            run(422, &[], r#"{"message":"Nope"}"#),
            Error::Unprocessable {
                message: "Nope".into()
            }
        );
        assert_eq!(
            run(422, &[], ""),
            Error::Unprocessable {
                message: String::new()
            }
        );
    }

    #[test]
    fn other_statuses_are_api_errors_with_their_code() {
        assert_eq!(
            run(500, &[], "{}"),
            Error::Api {
                status: 500,
                message: String::new()
            }
        );
        assert_eq!(
            run(409, &[], r#"{"message":"Conflict"}"#),
            Error::Api {
                status: 409,
                message: "Conflict".into()
            }
        );
    }

    #[test]
    fn a_huge_message_is_truncated() {
        let body = format!(r#"{{"message":"{}"}}"#, "é".repeat(10_000));
        match run(500, &[], &body) {
            Error::Api { message, .. } => assert_eq!(message.chars().count(), 300),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn redact_masks_the_secret_in_every_text_field() {
        let secret = "ghp_secret";
        let leaky = format!("bad {secret} twice {secret}");
        let masked = "bad *** twice ***";
        assert_eq!(
            redact(
                Error::Forbidden {
                    message: leaky.clone()
                },
                secret
            ),
            Error::Forbidden {
                message: masked.into()
            }
        );
        assert_eq!(
            redact(
                Error::Unprocessable {
                    message: leaky.clone()
                },
                secret
            ),
            Error::Unprocessable {
                message: masked.into()
            }
        );
        assert_eq!(
            redact(
                Error::Api {
                    status: 500,
                    message: leaky.clone()
                },
                secret
            ),
            Error::Api {
                status: 500,
                message: masked.into()
            }
        );
        assert_eq!(
            redact(
                Error::Network {
                    message: leaky.clone()
                },
                secret
            ),
            Error::Network {
                message: masked.into()
            }
        );
        assert_eq!(
            redact(Error::UnexpectedResponse { message: leaky }, secret),
            Error::UnexpectedResponse {
                message: masked.into()
            }
        );
        assert_eq!(redact(Error::Unauthorized, secret), Error::Unauthorized);
    }
}
