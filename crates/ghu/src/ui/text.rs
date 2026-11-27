//! Text coming from the network, made safe to draw, and the simple Markdown rendering (#6, ADR 0003 § 4 and § 7).
//!
//! Contract for dev[tui-detail]; used by every screen that shows network text (detail, diff, compose). No
//! dependency, no panic on any input.
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: used from wave 2 (dev[tui-detail])"
)]

use std::borrow::Cow;

use ratatui::text::Line;

/// Text safe to draw: tab → 4 spaces; other control characters (C0, DEL, C1), bidi embedding / override /
/// isolate marks (U+202A..=U+202E, U+2066..=U+2069) and U+FEFF → U+FFFD. Newlines are the caller's: it splits
/// with `str::lines()` first (`\n` and `\r\n` end a line), so only a lone `\r` reaches `clean` and becomes
/// U+FFFD. Borrowed when nothing changes. Pure, never panics.
pub(crate) fn clean(text: &str) -> Cow<'_, str> {
    let _ = text;
    todo!("ADR 0003, wave 2: dev[tui-detail]")
}

/// The Markdown subset of ADR 0003 § 4 as styled lines: `text` split with `str::lines()`, each line cleaned
/// ([`clean`]), `indent` spaces before each line. Headings `#`..`######` bold, bullets `•`, fenced blocks
/// (three backticks or `~~~`) indented by two more spaces and dimmed, `**x**` / `__x__` bold, `` `x` `` code
/// style; links, images, HTML, tables and quotes left as text. Never panics.
pub(crate) fn markdown(text: &str, indent: usize) -> Vec<Line<'static>> {
    let _ = (text, indent);
    todo!("ADR 0003, wave 2: dev[tui-detail]")
}
