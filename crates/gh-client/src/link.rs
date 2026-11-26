//! Pagination: the `Link` response header.

/// The target of `rel="next"` in a `Link` header value, if any.
///
/// Format (RFC 8288, as GitHub sends it): `<url>; rel="next", <url>; rel="last"`. A malformed entry is skipped.
pub(crate) fn next_link(header: &str) -> Option<&str> {
    let mut rest = header;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let end = after.find('>')?;
        let url = &after[..end];
        let (params, remainder) = split_once_outside_quotes(&after[end + 1..], ',');
        if !url.is_empty() && is_next(params) {
            return Some(url);
        }
        rest = remainder;
    }
    None
}

/// Whether the `;`-separated parameters of one entry hold `rel=next` (alone or in a space-separated list).
fn is_next(params: &str) -> bool {
    let mut rest = params;
    while !rest.is_empty() {
        let (param, remainder) = split_once_outside_quotes(rest, ';');
        rest = remainder;
        let Some((name, value)) = param.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("rel")
            && value
                .trim()
                .trim_matches('"')
                .split_whitespace()
                .any(|rel| rel.eq_ignore_ascii_case("next"))
        {
            return true;
        }
    }
    false
}

/// Splits at the first `delim` that is not inside a double-quoted string; the second part is empty if there is none.
fn split_once_outside_quotes(s: &str, delim: char) -> (&str, &str) {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c == delim && !in_quotes => return (&s[..i], &s[i + c.len_utf8()..]),
            _ => {}
        }
    }
    (s, "")
}

#[cfg(test)]
mod tests {
    use super::next_link;

    // Shape documented at https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api
    #[test]
    fn finds_next_among_several_relations() {
        let header = r#"<https://api.github.com/user/repos?page=3&per_page=100>; rel="next", <https://api.github.com/user/repos?page=50&per_page=100>; rel="last""#;
        assert_eq!(
            next_link(header),
            Some("https://api.github.com/user/repos?page=3&per_page=100")
        );
    }

    #[test]
    fn next_may_come_after_other_relations() {
        let header = r#"<https://h/r?page=1>; rel="prev", <https://h/r?page=1>; rel="first", <https://h/r?page=3>; rel="next""#;
        assert_eq!(next_link(header), Some("https://h/r?page=3"));
    }

    #[test]
    fn last_page_has_no_next() {
        let header = r#"<https://h/r?page=1>; rel="first", <https://h/r?page=2>; rel="prev""#;
        assert_eq!(next_link(header), None);
    }

    #[test]
    fn accepts_unquoted_relation_extra_parameters_and_relation_lists() {
        assert_eq!(next_link("<https://h/a>; rel=next"), Some("https://h/a"));
        assert_eq!(
            next_link(r#"<https://h/b>; title="x"; rel="prev next""#),
            Some("https://h/b")
        );
        assert_eq!(
            next_link(r#"<https://h/c>;REL="next""#),
            Some("https://h/c")
        );
    }

    #[test]
    fn a_relation_merely_containing_next_is_not_next() {
        assert_eq!(next_link(r#"<https://h/a>; rel="nextish""#), None);
        assert_eq!(next_link(r#"<https://h/a>; title="rel=next""#), None);
    }

    #[test]
    fn malformed_input_gives_none_and_never_panics() {
        for bad in [
            "",
            " ",
            ",",
            "<",
            ">",
            "<>",
            "<https://h/a",
            "https://h/a; rel=\"next\"",
            "rel=\"next\"",
            "<a>; rel=\"",
            ";;;",
            "<a>;",
        ] {
            assert_eq!(next_link(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_malformed_entry_does_not_hide_a_good_one() {
        assert_eq!(
            next_link(r#"garbage, <https://h/ok>; rel="next""#),
            Some("https://h/ok")
        );
    }
}
