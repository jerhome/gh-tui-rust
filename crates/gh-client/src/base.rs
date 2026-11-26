//! The base URL of the API.

use crate::Error;

/// Base URL of a GitHub REST API (`https://api.github.com`, or the fake server in tests).
///
/// Invariants (checked by [`ApiBase::parse`]):
/// - scheme `https`, or `http` only when the host is a loopback address (`localhost`, `127.0.0.0/8`, `::1`),
///   so the token never travels in clear text to another machine;
/// - no user info, query or fragment;
/// - a path is allowed (kept for a future `/api/v3` prefix) and stored without its trailing `/`, so routes
///   are joined as `{base}/user/repos`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiBase(url::Url);

impl ApiBase {
    /// Parses and validates a base URL (the value of `GHU_API_URL`, for example).
    ///
    /// Leading and trailing whitespace is ignored. Errors: [`Error::InvalidBaseUrl`] with a reason a user can act
    /// on (not a URL, wrong scheme, clear-text `http` to a non-loopback host, user info, query or fragment).
    pub fn parse(input: &str) -> Result<Self, Error> {
        let invalid = |reason: &str| Error::InvalidBaseUrl {
            reason: reason.to_owned(),
        };
        // The parser's own error text is dropped on purpose: it can quote the input, which may hold credentials.
        let url = url::Url::parse(input.trim())
            .map_err(|_| invalid("not a valid URL (expected https://host)"))?;
        match url.scheme() {
            "https" => {}
            "http" if is_loopback(&url) => {}
            "http" => {
                return Err(invalid(
                    "http is only accepted for localhost or loopback addresses, use https",
                ));
            }
            _ => {
                return Err(invalid(
                    "the scheme must be https (or http for a loopback address)",
                ));
            }
        }
        if url.host().is_none() {
            return Err(invalid("the URL has no host"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid(
                "the URL must not contain user info (user:password@)",
            ));
        }
        if url.query().is_some() {
            return Err(invalid("the URL must not contain a query (?...)"));
        }
        if url.fragment().is_some() {
            return Err(invalid("the URL must not contain a fragment (#...)"));
        }
        Ok(Self(url))
    }

    /// The base URL as text, without trailing `/` (`https://api.github.com`, `http://127.0.0.1:40123`).
    pub fn as_str(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }
}

impl ApiBase {
    /// Whether the host is `localhost` or a loopback address (the fake server of the tests).
    pub(crate) fn is_loopback(&self) -> bool {
        is_loopback(&self.0)
    }
}

impl Default for ApiBase {
    /// [`crate::DEFAULT_API_BASE`].
    fn default() -> Self {
        Self::parse(crate::DEFAULT_API_BASE).expect("DEFAULT_API_BASE is a valid https URL")
    }
}

/// Whether the host is `localhost` or a loopback IP address.
fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ApiBase;
    use crate::{DEFAULT_API_BASE, Error};

    fn reason(input: &str) -> String {
        match ApiBase::parse(input) {
            Err(Error::InvalidBaseUrl { reason }) => reason,
            other => panic!("expected InvalidBaseUrl for {input:?}, got {other:?}"),
        }
    }

    #[test]
    fn https_url_is_kept_without_trailing_slash() {
        assert_eq!(
            ApiBase::parse("https://api.github.com").unwrap().as_str(),
            "https://api.github.com"
        );
        assert_eq!(
            ApiBase::parse("https://api.github.com/").unwrap().as_str(),
            "https://api.github.com"
        );
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            ApiBase::parse("  https://api.github.com\n")
                .unwrap()
                .as_str(),
            "https://api.github.com"
        );
    }

    #[test]
    fn a_path_is_kept_without_its_trailing_slash() {
        assert_eq!(
            ApiBase::parse("https://ghe.example.com/api/v3/")
                .unwrap()
                .as_str(),
            "https://ghe.example.com/api/v3"
        );
    }

    #[test]
    fn http_is_accepted_for_loopback_hosts_only() {
        for ok in [
            "http://127.0.0.1:40123",
            "http://localhost:8080",
            "http://127.5.6.7",
            "http://[::1]:9000",
        ] {
            assert!(ApiBase::parse(ok).is_ok(), "{ok} should be accepted");
        }
        for bad in [
            "http://api.github.com",
            "http://192.168.1.10:8080",
            "http://0.0.0.0:80",
            "http://localhost.example.com",
            "http://127.0.0.1.example.com",
        ] {
            assert!(
                reason(bad).contains("http"),
                "{bad} should be refused for its scheme"
            );
        }
    }

    #[test]
    fn other_schemes_are_refused() {
        for bad in ["ftp://api.github.com", "file:///etc/passwd", "mailto:a@b.c"] {
            reason(bad);
        }
    }

    #[test]
    fn not_a_url_is_refused() {
        for bad in ["", "   ", "api.github.com", "https://"] {
            reason(bad);
        }
    }

    #[test]
    fn user_info_query_and_fragment_are_refused() {
        assert!(reason("https://user:secret@api.github.com").contains("user"));
        assert!(reason("https://user@api.github.com").contains("user"));
        assert!(reason("https://:secret@api.github.com").contains("user"));
        assert!(reason("https://api.github.com/?a=b").contains("query"));
        assert!(reason("https://api.github.com?").contains("query"));
        assert!(reason("https://api.github.com/#top").contains("fragment"));
    }

    #[test]
    fn the_reason_never_echoes_credentials() {
        assert!(!reason("https://user:hunter2@api.github.com").contains("hunter2"));
    }

    #[test]
    fn default_is_the_public_api() {
        assert_eq!(ApiBase::default().as_str(), DEFAULT_API_BASE);
        assert_eq!(
            ApiBase::default(),
            ApiBase::parse(DEFAULT_API_BASE).unwrap()
        );
    }
}
