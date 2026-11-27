//! A multi-line text field (#8; `Ctrl-E` and [`TextArea::edited`] by #9). ADR 0003 § 4.
//!
//! Contract for dev[tui-compose] (text, keys, paste, render), then dev[tui-editor] (`Ctrl-E`, `edited`), in that
//! order.

use std::cell::Cell;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text::clean;
use super::{Notice, Shared};
use crate::{EditorError, KeyCode, KeyEvent, KeyModifiers};

/// A multi-line text field. The text holds no control character but `\n` and `\t` (others dropped on insert,
/// paste and editor result; `\r\n` and `\r` become `\n`). Cursor in characters; no soft wrap: the view
/// scrolls to keep the cursor visible. Private fields belong to the lots above.
pub(crate) struct TextArea {
    /// The lines, without their `\n`; never empty (one empty line for an empty text).
    lines: Vec<String>,
    /// Cursor: (line, character within the line).
    cursor: (usize, usize),
    /// First line shown, updated at render time.
    offset: Cell<usize>,
    /// First screen column shown (display width, tabs expanded), updated at render time.
    column_offset: Cell<usize>,
}

/// What [`TextArea::key`] did with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKey {
    /// The key changed the text or the cursor (or asked for the editor).
    Handled,
    /// Not a text key: the owner decides (`Ctrl-S`, `Esc`…).
    Ignored,
}

impl TextArea {
    /// An empty field, cursor at the start.
    pub(crate) fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: (0, 0),
            offset: Cell::new(0),
            column_offset: Cell::new(0),
        }
    }

    /// The whole text, lines joined with `\n`.
    pub(crate) fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Characters (with or without Shift, and AltGr = Ctrl+Alt on Windows), Enter (newline), Backspace, Delete,
    /// arrows, Home, End; `Ctrl-E` (#9): [`Shared::request_edit`] with the current text.
    pub(crate) fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> TextKey {
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let altgr = key.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT;
        match key.code {
            KeyCode::Char('e') if key.modifiers == KeyModifiers::CONTROL => {
                shared.request_edit(self.text());
            }
            KeyCode::Char(c) if plain || altgr => self.insert_char(c),
            KeyCode::Enter if plain => self.insert_newline(),
            KeyCode::Backspace if plain => self.backspace(),
            KeyCode::Delete if plain => self.delete(),
            KeyCode::Left if plain => self.left(),
            KeyCode::Right if plain => self.right(),
            KeyCode::Up if plain => self.vertical(false),
            KeyCode::Down if plain => self.vertical(true),
            KeyCode::Home if plain => self.cursor.1 = 0,
            KeyCode::End if plain => self.cursor.1 = self.line_len(self.cursor.0),
            _ => return TextKey::Ignored,
        }
        TextKey::Handled
    }

    /// Inserts pasted text at the cursor (same filtering as typed text).
    pub(crate) fn paste(&mut self, text: &str) {
        let text = normalise(text);
        let mut pieces = text.split('\n');
        if let Some(first) = pieces.next() {
            self.insert_piece(first);
        }
        for piece in pieces {
            self.insert_newline();
            self.insert_piece(piece);
        }
    }

    /// The editor's answer. `Ok`: replaces the text (trailing newlines removed, leading U+FEFF removed, same
    /// filtering as a paste), cursor at the end. `Err`: text unchanged, `Notice::Error` with the message of
    /// ADR 0003 § 5.
    pub(crate) fn edited(&mut self, result: Result<String, EditorError>, shared: &mut Shared) {
        match result {
            Ok(text) => {
                let text = normalise(text.strip_prefix('\u{feff}').unwrap_or(&text));
                self.lines = text
                    .trim_end_matches('\n')
                    .split('\n')
                    .map(str::to_owned)
                    .collect();
                let last = self.lines.len() - 1;
                self.cursor = (last, self.line_len(last));
            }
            Err(error) => shared.notify(Notice::Error(editor_message(&error))),
        }
    }

    /// Draws the field in `area` and places the terminal cursor.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = usize::from(area.width);
        let height = usize::from(area.height);
        let (line, column) = self.cursor;
        // Vertical scroll: keep the cursor line in the window.
        let mut first = self.offset.get().min(line);
        if line >= first + height {
            first = line + 1 - height;
        }
        self.offset.set(first);
        // Horizontal scroll: keep the cursor cell in the window.
        let cursor_column = self.display_width_before(line, column);
        let mut left = self.column_offset.get().min(cursor_column);
        if cursor_column >= left + width {
            left = cursor_column + 1 - width;
        }
        self.column_offset.set(left);

        let shown: Vec<Line<'_>> = self
            .lines
            .iter()
            .skip(first)
            .take(height)
            .map(|text| Line::from(visible(&clean(text), left, width)))
            .collect();
        frame.render_widget(Paragraph::new(shown), area);
        // Both offsets are bounded by the area, so the casts cannot truncate.
        let x = area
            .x
            .saturating_add(u16::try_from(cursor_column - left).unwrap_or(0));
        let y = area
            .y
            .saturating_add(u16::try_from(line - first).unwrap_or(0));
        frame.set_cursor_position(Position::new(x, y));
    }

    fn line_len(&self, line: usize) -> usize {
        self.lines[line].chars().count()
    }

    /// Byte index of character `column` of `line` (the end of the line past its last character).
    fn byte_at(&self, line: usize, column: usize) -> usize {
        let text = &self.lines[line];
        text.char_indices()
            .nth(column)
            .map_or(text.len(), |(index, _)| index)
    }

    /// Display width of what precedes the cursor column on `line`, tabs expanded.
    fn display_width_before(&self, line: usize, column: usize) -> usize {
        let end = self.byte_at(line, column);
        cells(&clean(&self.lines[line][..end]))
    }

    fn insert_char(&mut self, c: char) {
        if c == '\t' || !c.is_control() {
            let mut buffer = [0; 4];
            self.insert_piece(c.encode_utf8(&mut buffer));
        }
    }

    /// Inserts text without newline at the cursor (already filtered).
    fn insert_piece(&mut self, piece: &str) {
        let (line, column) = self.cursor;
        let at = self.byte_at(line, column);
        self.lines[line].insert_str(at, piece);
        self.cursor.1 += piece.chars().count();
    }

    fn insert_newline(&mut self) {
        let (line, column) = self.cursor;
        let at = self.byte_at(line, column);
        let tail = self.lines[line].split_off(at);
        self.lines.insert(line + 1, tail);
        self.cursor = (line + 1, 0);
    }

    fn backspace(&mut self) {
        let (line, column) = self.cursor;
        if column > 0 {
            let at = self.byte_at(line, column - 1);
            self.lines[line].remove(at);
            self.cursor.1 -= 1;
        } else if line > 0 {
            let joined = self.lines.remove(line);
            let previous = line - 1;
            let end = self.line_len(previous);
            self.lines[previous].push_str(&joined);
            self.cursor = (previous, end);
        }
    }

    fn delete(&mut self) {
        let (line, column) = self.cursor;
        if column < self.line_len(line) {
            let at = self.byte_at(line, column);
            self.lines[line].remove(at);
        } else if line + 1 < self.lines.len() {
            let next = self.lines.remove(line + 1);
            self.lines[line].push_str(&next);
        }
    }

    fn left(&mut self) {
        let (line, column) = self.cursor;
        if column > 0 {
            self.cursor.1 -= 1;
        } else if line > 0 {
            self.cursor = (line - 1, self.line_len(line - 1));
        }
    }

    fn right(&mut self) {
        let (line, column) = self.cursor;
        if column < self.line_len(line) {
            self.cursor.1 += 1;
        } else if line + 1 < self.lines.len() {
            self.cursor = (line + 1, 0);
        }
    }

    /// Up or down one line, the column kept within the new line.
    fn vertical(&mut self, down: bool) {
        let (line, column) = self.cursor;
        let target = if down {
            if line + 1 >= self.lines.len() {
                return;
            }
            line + 1
        } else {
            if line == 0 {
                return;
            }
            line - 1
        };
        self.cursor = (target, column.min(self.line_len(target)));
    }
}

/// The notice for an editor failure (ADR 0003 § 5).
fn editor_message(error: &EditorError) -> String {
    match error {
        EditorError::Unparsable { var } => {
            format!("Cannot read {var}: unbalanced quote or no program.")
        }
        EditorError::NotFound { program } => format!(
            "Cannot open the editor: {} was not found. Set VISUAL or EDITOR.",
            clean(program)
        ),
        EditorError::Failed { code: Some(code) } => {
            format!("The editor failed (exit code {code}): the text is unchanged.")
        }
        EditorError::Failed { code: None } => {
            "The editor failed (stopped): the text is unchanged.".to_owned()
        }
        EditorError::TooLarge => {
            "The edited text is larger than 1 MiB: the text is unchanged.".to_owned()
        }
        EditorError::NotUtf8 => "The edited text is not UTF-8: the text is unchanged.".to_owned(),
        EditorError::ReturnedAtOnce => {
            "The editor returned at once: the text is unchanged. Set VISUAL to an \
             editor that waits (vim, nano, \"code.cmd --wait\")."
                .to_owned()
        }
        EditorError::Io { message } => format!("Cannot use the editor: {}.", clean(message)),
    }
}

/// The text a field accepts: `\r\n` and `\r` become `\n`; control characters but `\n` and `\t` are dropped.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Terminal columns of `text` (CJK and emoji count two).
fn cells(text: &str) -> usize {
    Span::raw(text).width()
}

/// Terminal columns of one character.
fn cell(c: char) -> usize {
    let mut buffer = [0; 4];
    cells(c.encode_utf8(&mut buffer))
}

/// The part of `text` between columns `skip` and `skip + width`. A wide character cut by the left edge leaves
/// blanks; one that does not fit on the right is left out.
fn visible(text: &str, skip: usize, width: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for c in text.chars() {
        let size = cell(c);
        if column < skip {
            column += size;
            if column > skip {
                out.push_str(&" ".repeat(column - skip));
            }
            continue;
        }
        if size == 0 && out.is_empty() {
            continue;
        }
        if column + size > skip + width {
            break;
        }
        out.push(c);
        column += size;
    }
    out
}

/// `text` cut into rows of at most `width` columns (a character is never split); one empty row for an empty text.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for c in text.chars() {
        let size = cell(c);
        if used + size > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push(c);
        used += size;
    }
    rows.push(row);
    rows
}

/// A [`Shared`] for unit tests of screens: a client that is never used (nothing here sends), no pins, no editor.
#[cfg(test)]
pub(super) fn test_shared() -> Shared {
    use crate::editor::EditorVars;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let client = gh_client::Client::new(
        gh_client::ApiBase::default(),
        gh_client::Token::new("unused").expect("token"),
    )
    .expect("client");
    Shared {
        client,
        tx,
        in_flight: 0,
        next_id: 0,
        notice: None,
        pushed: Vec::new(),
        popped: None,
        editor: EditorVars::default(),
        edit: None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn press(area: &mut TextArea, shared: &mut Shared, code: KeyCode) -> TextKey {
        area.key(KeyEvent::new(code, KeyModifiers::NONE), shared)
    }

    fn type_text(area: &mut TextArea, shared: &mut Shared, text: &str) {
        for c in text.chars() {
            press(area, shared, KeyCode::Char(c));
        }
    }

    /// Draws `area` in a `width` x `height` terminal; returns the rows (second cells of wide characters are
    /// blanks, as in the harness) and the cursor position.
    fn draw(area: &TextArea, width: u16, height: u16) -> (Vec<String>, (u16, u16)) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| area.render(frame, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        let cursor = terminal.get_cursor_position().expect("cursor");
        (rows, (cursor.x, cursor.y))
    }

    fn area_with(text: &str) -> TextArea {
        let mut area = TextArea::new();
        area.paste(text);
        area
    }

    #[test]
    fn a_new_field_is_empty_and_the_text_joins_lines_with_newlines() {
        assert_eq!(TextArea::new().text(), "");
        assert_eq!(area_with("a\n\nb\n").text(), "a\n\nb\n");
    }

    #[test]
    fn a_paste_filters_like_typing() {
        assert_eq!(
            area_with("a\r\nb\rc\u{0}d\u{1b}e\u{7f}f\u{85}g\th").text(),
            "a\nb\ncdefg\th"
        );
        assert_eq!(area_with("\r\n\r\n").text(), "\n\n");
        assert_eq!(area_with("").text(), "");
        let mut shared = test_shared();
        let mut area = TextArea::new();
        // A control character typed (a Ctrl+Alt chord can produce one) is dropped.
        area.key(
            KeyEvent::new(
                KeyCode::Char('\u{1}'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            &mut shared,
        );
        assert_eq!(area.text(), "");
    }

    #[test]
    fn keys_that_are_not_text_are_left_to_the_owner() {
        let mut shared = test_shared();
        let mut area = area_with("abc");
        for (code, modifiers) in [
            (KeyCode::Char('s'), KeyModifiers::CONTROL),
            (KeyCode::Char('x'), KeyModifiers::ALT),
            (KeyCode::Esc, KeyModifiers::NONE),
            (KeyCode::Tab, KeyModifiers::NONE),
            (KeyCode::PageUp, KeyModifiers::NONE),
            (KeyCode::F(1), KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::CONTROL),
            (KeyCode::Left, KeyModifiers::ALT),
        ] {
            assert_eq!(
                area.key(KeyEvent::new(code, modifiers), &mut shared),
                TextKey::Ignored,
                "{code:?} {modifiers:?}"
            );
        }
        assert_eq!(area.text(), "abc");
        // Ctrl-E is the editor's (#9): used, text unchanged, editor asked for.
        assert_eq!(
            area.key(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                &mut shared
            ),
            TextKey::Handled
        );
        assert_eq!(area.text(), "abc");
        assert_eq!(shared.edit.as_deref(), Some("abc"));
    }

    #[test]
    fn the_cursor_is_placed_by_terminal_columns_not_characters() {
        let mut shared = test_shared();
        for (text, columns) in [
            ("abc", 3),
            ("日本", 4),
            ("😀", 2),
            ("a😀b", 4),
            ("e\u{301}", 1),
            ("\t", 4),
            ("a\tb", 6),
        ] {
            let mut area = TextArea::new();
            type_text(&mut area, &mut shared, text);
            let (_, cursor) = draw(&area, 20, 3);
            assert_eq!(cursor, (columns, 0), "{text:?}");
        }
        // Moving left over a wide character moves two columns.
        let mut area = area_with("日本語");
        press(&mut area, &mut shared, KeyCode::Left);
        assert_eq!(draw(&area, 20, 3).1, (4, 0));
        press(&mut area, &mut shared, KeyCode::Left);
        assert_eq!(draw(&area, 20, 3).1, (2, 0));
        press(&mut area, &mut shared, KeyCode::Home);
        assert_eq!(draw(&area, 20, 3).1, (0, 0));
    }

    #[test]
    fn the_cursor_follows_the_line_and_the_wide_text_is_drawn() {
        let area = area_with("one\n日本語 😀\nthree");
        let (rows, cursor) = draw(&area, 20, 5);
        assert_eq!(cursor, (5, 2));
        assert_eq!(rows[0], "one");
        assert_eq!(rows[2], "three");
        assert_eq!(rows[1].replace(' ', ""), "日本語😀");
    }

    #[test]
    fn a_long_line_scrolls_sideways_and_never_shows_half_a_wide_character() {
        let mut shared = test_shared();
        let line = "日".repeat(30);
        let mut area = area_with(&line);
        let (rows, cursor) = draw(&area, 10, 3);
        // The cursor is at the end, inside the window, on the cell after the last drawn character.
        assert!(cursor.0 < 10, "{cursor:?}");
        assert_eq!(cursor.1, 0);
        assert!(rows[0].chars().all(|c| c == '日' || c == ' '), "{rows:?}");
        assert!(rows[0].replace(' ', "").chars().count() >= 4, "{rows:?}");
        // Back at the start the first character is shown again.
        press(&mut area, &mut shared, KeyCode::Home);
        let (rows, cursor) = draw(&area, 10, 3);
        assert_eq!(cursor, (0, 0));
        assert!(rows[0].starts_with('日'), "{rows:?}");
        // One step right, then one more: scroll only when the cursor leaves the window.
        for _ in 0..4 {
            press(&mut area, &mut shared, KeyCode::Right);
        }
        let (_, cursor) = draw(&area, 10, 3);
        assert_eq!(cursor, (8, 0));
        press(&mut area, &mut shared, KeyCode::Right);
        let (rows, cursor) = draw(&area, 10, 3);
        assert!(cursor.0 < 10 && cursor.0 >= 8, "{cursor:?}");
        assert!(rows[0].chars().all(|c| c == '日' || c == ' '), "{rows:?}");
    }

    #[test]
    fn many_lines_scroll_to_keep_the_cursor_in_view() {
        let text = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut shared = test_shared();
        let mut area = area_with(&text);
        let (rows, cursor) = draw(&area, 20, 3);
        assert_eq!(rows, vec!["line 7", "line 8", "line 9"]);
        assert_eq!(cursor, (6, 2));
        for _ in 0..9 {
            press(&mut area, &mut shared, KeyCode::Up);
        }
        let (rows, cursor) = draw(&area, 20, 3);
        assert_eq!(rows, vec!["line 0", "line 1", "line 2"]);
        assert_eq!(cursor.1, 0);
    }

    #[test]
    fn an_empty_area_or_a_one_column_area_does_not_panic() {
        let area = area_with("日本語\nabc");
        for (width, height) in [(1, 1), (2, 1), (1, 3), (3, 1)] {
            let _ = draw(&area, width, height);
        }
        let mut terminal = Terminal::new(TestBackend::new(5, 5)).expect("terminal");
        terminal
            .draw(|frame| area.render(frame, Rect::new(0, 0, 0, 0)))
            .expect("draw");
    }

    #[test]
    fn wrap_cuts_by_columns_and_keeps_every_character() {
        assert_eq!(wrap("", 5), vec![String::new()]);
        assert_eq!(wrap("abcdef", 4), vec!["abcd", "ef"]);
        assert_eq!(wrap("日本語", 3), vec!["日", "本", "語"]);
        assert_eq!(wrap("日本語", 4), vec!["日本", "語"]);
        assert_eq!(wrap("a日", 2), vec!["a", "日"]);
        // A column too narrow for a wide character still moves on (one character per row).
        assert_eq!(wrap("日本", 1), vec!["日", "本"]);
        assert_eq!(wrap("abc", 0), vec!["a", "b", "c"]);
        assert_eq!(wrap("e\u{301}x", 1), vec!["e\u{301}", "x"]);
    }

    #[test]
    fn visible_blanks_a_wide_character_cut_by_the_left_edge() {
        assert_eq!(visible("日本語", 1, 10), " 本語");
        assert_eq!(visible("日本語", 2, 10), "本語");
        assert_eq!(visible("日本語", 0, 5), "日本");
        assert_eq!(visible("abc", 5, 3), "");
        assert_eq!(visible("e\u{301}bc", 1, 2), "bc");
    }

    /// Fixed-seed key and paste sequences: the invariants of the text hold after every step and nothing panics.
    #[test]
    fn random_keys_and_pastes_keep_the_invariants() {
        let mut shared = test_shared();
        let mut seed = 0x5eed_1234_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(seed >> 33).expect("fits")
        };
        const CHARS: [char; 10] = [
            'a', 'é', '日', '😀', ' ', '\t', '\u{301}', '\u{202e}', '\r', '\u{1b}',
        ];
        let mut area = TextArea::new();
        for step in 0..3000 {
            match next() % 12 {
                0 => press(&mut area, &mut shared, KeyCode::Enter),
                1 => press(&mut area, &mut shared, KeyCode::Backspace),
                2 => press(&mut area, &mut shared, KeyCode::Delete),
                3 => press(&mut area, &mut shared, KeyCode::Left),
                4 => press(&mut area, &mut shared, KeyCode::Right),
                5 => press(&mut area, &mut shared, KeyCode::Up),
                6 => press(&mut area, &mut shared, KeyCode::Down),
                7 => press(&mut area, &mut shared, KeyCode::Home),
                8 => press(&mut area, &mut shared, KeyCode::End),
                9 => {
                    let pasted: String = (0..next() % 6)
                        .map(|_| CHARS[next() % CHARS.len()])
                        .chain(['\n'])
                        .collect();
                    area.paste(&pasted);
                    TextKey::Handled
                }
                _ => press(
                    &mut area,
                    &mut shared,
                    KeyCode::Char(CHARS[next() % CHARS.len()]),
                ),
            };
            assert!(!area.lines.is_empty(), "step {step}");
            for line in &area.lines {
                assert!(!line.contains('\n'), "step {step}: {line:?}");
                assert!(
                    line.chars().all(|c| c == '\t' || !c.is_control()),
                    "step {step}: {line:?}"
                );
            }
            let (line, column) = area.cursor;
            assert!(line < area.lines.len(), "step {step}");
            assert!(column <= area.lines[line].chars().count(), "step {step}");
            if step % 50 == 0 {
                let (width, height) = (
                    u16::try_from(1 + next() % 12).expect("small"),
                    1 + (next() % 4) as u16,
                );
                let (_, cursor) = draw(&area, width, height);
                assert!(
                    cursor.0 < width && cursor.1 < height,
                    "step {step}: {cursor:?}"
                );
            }
        }
    }

    fn ctrl_e(area: &mut TextArea, shared: &mut Shared) -> TextKey {
        area.key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            shared,
        )
    }

    #[test]
    fn ctrl_e_asks_for_the_editor_with_the_current_text() {
        let mut shared = test_shared();
        let mut area = TextArea::new();
        type_text(&mut area, &mut shared, "ab");
        press(&mut area, &mut shared, KeyCode::Enter);
        type_text(&mut area, &mut shared, "c");
        assert_eq!(ctrl_e(&mut area, &mut shared), TextKey::Handled);
        assert_eq!(shared.edit.as_deref(), Some("ab\nc"));
        assert_eq!(area.text(), "ab\nc");
    }

    #[test]
    fn an_edited_text_replaces_the_field_cleaned_with_the_cursor_at_the_end() {
        let mut shared = test_shared();
        let mut area = TextArea::new();
        type_text(&mut area, &mut shared, "old");
        area.edited(
            Ok("\u{feff}one\r\ntwo\u{1b}\rthree\n\n".to_owned()),
            &mut shared,
        );
        assert_eq!(area.text(), "one\ntwo\nthree");
        assert_eq!(area.cursor, (2, 5));
        assert!(shared.notice.is_none());
        area.edited(Ok(String::new()), &mut shared);
        assert_eq!(area.text(), "");
        assert_eq!(area.cursor, (0, 0));
        area.edited(Ok("\n\n".to_owned()), &mut shared);
        assert_eq!(area.lines, vec![String::new()]);
    }

    #[test]
    fn an_editor_failure_keeps_the_text_and_says_why() {
        let cases = [
            (
                EditorError::Unparsable { var: "VISUAL" },
                "Cannot read VISUAL: unbalanced quote or no program.",
            ),
            (
                EditorError::NotFound {
                    program: "nvim".to_owned(),
                },
                "Cannot open the editor: nvim was not found. Set VISUAL or EDITOR.",
            ),
            (
                EditorError::Failed { code: Some(3) },
                "The editor failed (exit code 3): the text is unchanged.",
            ),
            (
                EditorError::Failed { code: None },
                "The editor failed (stopped): the text is unchanged.",
            ),
            (
                EditorError::TooLarge,
                "The edited text is larger than 1 MiB: the text is unchanged.",
            ),
            (
                EditorError::NotUtf8,
                "The edited text is not UTF-8: the text is unchanged.",
            ),
            (
                EditorError::ReturnedAtOnce,
                "The editor returned at once: the text is unchanged. Set VISUAL to an editor that waits (vim, \
                 nano, \"code.cmd --wait\").",
            ),
            (
                EditorError::Io {
                    message: "disk full".to_owned(),
                },
                "Cannot use the editor: disk full.",
            ),
        ];
        for (error, message) in cases {
            let mut shared = test_shared();
            let mut area = TextArea::new();
            type_text(&mut area, &mut shared, "kept");
            area.edited(Err(error), &mut shared);
            assert_eq!(area.text(), "kept");
            match &shared.notice {
                Some(Notice::Error(text)) => assert_eq!(text, message),
                other => panic!("{message}: {:?}", other.as_ref().map(|_| "not an error")),
            }
        }
    }
}
