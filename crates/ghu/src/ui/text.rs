//! Text coming from the network, made safe to draw, and the simple Markdown rendering (#6, ADR 0003 § 4 and § 7).
//!
//! Used by every screen that shows network text (detail, diff, compose). No dependency, no panic on any input.
use std::borrow::Cow;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Text safe to draw: tab → 4 spaces; other control characters (C0, DEL, C1), bidi embedding / override /
/// isolate marks (U+202A..=U+202E, U+2066..=U+2069) and U+FEFF → U+FFFD. Newlines are the caller's: it splits
/// with `str::lines()` first (`\n` and `\r\n` end a line), so only a lone `\r` reaches `clean` and becomes
/// U+FFFD. Borrowed when nothing changes. Pure, never panics.
pub(crate) fn clean(text: &str) -> Cow<'_, str> {
    if !text.chars().any(needs_cleaning) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if needs_cleaning(c) => out.push(char::REPLACEMENT_CHARACTER),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// `true` for a tab, a control character (C0, DEL, C1), a bidi embedding / override / isolate mark or U+FEFF.
fn needs_cleaning(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// The Markdown subset of ADR 0003 § 4 as styled lines: `text` split with `str::lines()`, each line cleaned
/// ([`clean`]), `indent` spaces before each line. Headings `#`..`######` bold, bullets `•`, fenced blocks
/// (three backticks or `~~~`) indented by two more spaces and dimmed, `**x**` / `__x__` bold, ``x`` code
/// style; links, images, HTML, tables and quotes left as text. Never panics.
pub(crate) fn markdown(text: &str, indent: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    // The marker character (`` ` `` or `~`) of the open fenced block, if any.
    let mut fence: Option<char> = None;
    for raw in text.lines() {
        let line = clean(raw);
        let marker = fence_marker(&line);
        match (fence, marker) {
            (None, Some(open)) => {
                fence = Some(open);
                continue;
            }
            (Some(open), Some(close)) if open == close => {
                fence = None;
                continue;
            }
            _ => {}
        }
        if line.is_empty() {
            lines.push(Line::default());
            continue;
        }
        let mut spans = Vec::new();
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        if fence.is_some() {
            spans.push(Span::styled(
                format!("  {line}"),
                Style::new().add_modifier(Modifier::DIM),
            ));
        } else {
            spans.extend(block_spans(&line));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// ``` ` `` or `~` when `line` starts (after spaces) with three of them.
fn fence_marker(line: &str) -> Option<char> {
    let trimmed = line.trim_start();
    ['`', '~']
        .into_iter()
        .find(|&marker| trimmed.chars().take(3).filter(|&c| c == marker).count() == 3)
}

/// The spans of one line outside a fenced block: heading, bullet or plain text with inline styles.
fn block_spans(line: &str) -> Vec<Span<'static>> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        let title = line[hashes + 1..].trim();
        return vec![Span::styled(
            title.to_owned(),
            Style::new().add_modifier(Modifier::BOLD),
        )];
    }
    let trimmed = line.trim_start();
    let lead = &line[..line.len() - trimmed.len()];
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            let mut spans = vec![Span::raw(format!("{lead}• "))];
            spans.extend(inline(rest));
            return spans;
        }
    }
    inline(line)
}

/// Inline styles: `**x**` and `__x__` bold, ``x`` code. A marker without a closing one, or around nothing, is
/// text; `__` inside a word (`snake__case__x`) is text too.
fn inline(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while at < line.len() {
        let rest = &line[at..];
        let before = line[..at].chars().next_back();
        if let Some((consumed, content, style)) = styled_run(rest, before) {
            if !plain.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut plain)));
            }
            spans.push(Span::styled(content.to_owned(), style));
            at += consumed;
            continue;
        }
        let c = rest.chars().next().expect("rest is not empty");
        plain.push(c);
        at += c.len_utf8();
    }
    if !plain.is_empty() {
        spans.push(Span::raw(plain));
    }
    spans
}

/// If `rest` starts with a complete bold or code run: the bytes it takes, its content and its style.
fn styled_run(rest: &str, before: Option<char>) -> Option<(usize, &str, Style)> {
    if let Some(inner) = rest.strip_prefix('`') {
        let end = inner.find('`').filter(|&end| end > 0)?;
        return Some((end + 2, &inner[..end], Style::new().fg(Color::Yellow)));
    }
    let marker = if rest.starts_with("**") {
        "**"
    } else if rest.starts_with("__") && !before.is_some_and(char::is_alphanumeric) {
        "__"
    } else {
        return None;
    };
    let inner = &rest[2..];
    let end = inner.find(marker).filter(|&end| end > 0)?;
    let after = inner[end + 2..].chars().next();
    if marker == "__" && after.is_some_and(char::is_alphanumeric) {
        return None;
    }
    Some((
        end + 4,
        &inner[..end],
        Style::new().add_modifier(Modifier::BOLD),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn plain(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn rendered(text: &str, indent: usize) -> Vec<String> {
        markdown(text, indent).iter().map(plain).collect()
    }

    /// The span of `line` whose content is exactly `content`.
    fn span<'a>(line: &'a Line<'static>, content: &str) -> &'a ratatui::text::Span<'static> {
        line.spans
            .iter()
            .find(|s| s.content == content)
            .unwrap_or_else(|| panic!("no span {content:?} in {line:?}"))
    }

    fn is_bold(span: &ratatui::text::Span<'_>) -> bool {
        span.style.add_modifier.contains(Modifier::BOLD)
    }

    fn is_dim(span: &ratatui::text::Span<'_>) -> bool {
        span.style.add_modifier.contains(Modifier::DIM)
    }

    #[test]
    fn clean_leaves_ordinary_text_borrowed() {
        for text in [
            "",
            "plain",
            "héllo wörld",
            "日本語 👨\u{200d}👩\u{200d}👧",
            "a  b",
        ] {
            assert!(matches!(clean(text), Cow::Borrowed(_)), "{text:?}");
            assert_eq!(clean(text), text);
        }
    }

    #[test]
    fn clean_turns_a_tab_into_four_spaces() {
        assert_eq!(clean("a\tb"), "a    b");
        assert_eq!(clean("\t"), "    ");
    }

    #[test]
    fn clean_replaces_control_characters_with_the_replacement_character() {
        assert_eq!(clean("a\u{1b}[31mb"), "a\u{fffd}[31mb");
        assert_eq!(clean("\u{0}\u{7}\u{8}\u{7f}"), "\u{fffd}".repeat(4));
        // C1 controls (CSI is U+009B).
        assert_eq!(clean("\u{80}\u{9b}\u{9f}"), "\u{fffd}".repeat(3));
        // A lone CR reaches `clean` (callers split lines first); so would a stray newline.
        assert_eq!(clean("a\rb"), "a\u{fffd}b");
        assert_eq!(clean("a\nb"), "a\u{fffd}b");
    }

    #[test]
    fn clean_replaces_bidi_marks_and_the_byte_order_mark() {
        for c in ('\u{202a}'..='\u{202e}')
            .chain('\u{2066}'..='\u{2069}')
            .chain(['\u{feff}'])
        {
            let text = format!("a{c}b");
            assert_eq!(clean(&text), "a\u{fffd}b", "U+{:04X}", u32::from(c));
        }
        // Neighbours of the ranges are ordinary characters.
        for c in [
            '\u{2029}', '\u{202f}', '\u{2065}', '\u{206a}', '\u{200d}', '\u{fffd}',
        ] {
            let text = format!("a{c}b");
            assert_eq!(clean(&text), text, "U+{:04X}", u32::from(c));
        }
    }

    #[test]
    fn markdown_renders_headings_in_bold_without_the_hashes() {
        let lines = markdown("# One\n###### Six\n####### Seven\n#NoSpace", 0);
        assert_eq!(plain(&lines[0]), "One");
        assert!(lines[0].spans.iter().all(is_bold));
        assert_eq!(plain(&lines[1]), "Six");
        assert!(lines[1].spans.iter().all(is_bold));
        // Not headings: seven hashes, no space after the hashes.
        assert_eq!(plain(&lines[2]), "####### Seven");
        assert!(!lines[2].spans.iter().any(is_bold));
        assert_eq!(plain(&lines[3]), "#NoSpace");
    }

    #[test]
    fn markdown_renders_bullets_and_keeps_numbers_and_nesting() {
        assert_eq!(
            rendered(
                "- a\n* b\n+ c\n  - nested\n1. one\n-not a bullet\n**bold** line",
                0
            ),
            [
                "• a",
                "• b",
                "• c",
                "  • nested",
                "1. one",
                "-not a bullet",
                "bold line"
            ]
        );
    }

    #[test]
    fn markdown_renders_bold_and_code_spans() {
        let lines = markdown("a **b** c __d__ e `f` g", 0);
        let line = &lines[0];
        assert_eq!(plain(line), "a b c d e f g");
        assert!(is_bold(span(line, "b")));
        assert!(is_bold(span(line, "d")));
        assert_eq!(span(line, "f").style.fg, Some(Color::Yellow));
        assert!(!is_bold(span(line, "a ")));
    }

    #[test]
    fn markdown_leaves_unbalanced_or_empty_markers_as_text() {
        for text in ["**open", "a ** b", "`open", "``", "****", "snake__case__x"] {
            let got = rendered(text, 0);
            assert_eq!(got, [text], "{text:?}");
        }
        // `__` inside a word is not bold; after a space it is.
        let lines = markdown("x __y__", 0);
        assert!(is_bold(span(&lines[0], "y")));
    }

    #[test]
    fn markdown_keeps_links_images_html_tables_and_quotes_as_text() {
        let text = "[link](https://example.com)\n![alt](https://example.com/a.png)\n<b>raw</b>\n| a | b |\n> quoted";
        assert_eq!(rendered(text, 0), text.lines().collect::<Vec<_>>());
    }

    #[test]
    fn markdown_indents_fenced_blocks_and_interprets_nothing_inside() {
        let text = "before\n```sh\n# not a heading\n- not a bullet **x**\n```\nafter\n~~~\ninside tilde\n~~~";
        let lines = markdown(text, 0);
        let got: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(
            got,
            [
                "before",
                "  # not a heading",
                "  - not a bullet **x**",
                "after",
                "  inside tilde"
            ]
        );
        assert!(lines[1].spans.iter().all(is_dim));
        assert!(lines[2].spans.iter().all(is_dim));
        assert!(!lines[3].spans.iter().any(is_dim));
        assert!(lines[4].spans.iter().all(is_dim));
    }

    #[test]
    fn markdown_closes_a_fence_only_with_the_same_marker_and_runs_to_the_end_when_unclosed() {
        assert_eq!(
            rendered("```\na\n~~~\nb\n```\nc\n```\nunclosed", 0),
            ["  a", "  ~~~", "  b", "c", "  unclosed"]
        );
    }

    #[test]
    fn markdown_adds_the_indent_to_every_line() {
        assert_eq!(
            rendered("a\n\n- b\n```\nc\n```", 2),
            ["  a", "", "  • b", "    c"]
        );
    }

    #[test]
    fn markdown_splits_lines_like_str_lines_and_cleans_each_one() {
        // CRLF ends a line without leaving a replacement character; a lone CR is made visible.
        assert_eq!(rendered("a\r\nb", 0), ["a", "b"]);
        assert_eq!(rendered("a\rb", 0), ["a\u{fffd}b"]);
        assert_eq!(rendered("a\n\nb\n", 0), ["a", "", "b"]);
        assert_eq!(rendered("", 0), Vec::<String>::new());
        // Escape sequences and bidi marks never survive, even inside markers and fences.
        assert_eq!(rendered("**\u{1b}[31m**", 0), ["\u{fffd}[31m"]);
        assert_eq!(rendered("```\n\u{202e}x\n```", 0), ["  \u{fffd}x"]);
        assert_eq!(rendered("- \u{9b}", 0), ["• \u{fffd}"]);
    }

    /// Deterministic generator (xorshift64, fixed seed): no dependency, same inputs on every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn markdown_and_clean_never_panic_and_never_let_a_control_character_out() {
        const POOL: [&str; 30] = [
            "*",
            "**",
            "_",
            "__",
            "`",
            "```",
            "~~~",
            "#",
            "# ",
            "- ",
            "+ ",
            "1. ",
            " ",
            "  ",
            "\t",
            "\n",
            "\r",
            "\r\n",
            "a",
            "bc",
            "é",
            "日",
            "👨\u{200d}👩",
            "\u{1b}",
            "\u{0}",
            "\u{9b}",
            "\u{202e}",
            "\u{feff}",
            "[",
            "](",
        ];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..3000 {
            let len = usize::try_from(rng.next() % 40).expect("small");
            let text: String = (0..len)
                .map(|_| POOL[usize::try_from(rng.next() % 30).expect("small")])
                .collect();
            let indent = usize::try_from(rng.next() % 4).expect("small");
            for line in markdown(&text, indent) {
                for span in &line.spans {
                    for c in span.content.chars() {
                        assert!(!c.is_control(), "control {c:?} from {text:?}");
                        assert!(
                            !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'),
                            "bidi {c:?} from {text:?}"
                        );
                    }
                }
            }
            let cleaned = clean(&text);
            assert_eq!(clean(&cleaned), cleaned, "clean is idempotent for {text:?}");
        }
    }
}
