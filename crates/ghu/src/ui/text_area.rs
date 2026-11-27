//! A multi-line text field (#8; `Ctrl-E` and [`TextArea::edited`] by #9). ADR 0003 § 4.
//!
//! Contract for dev[tui-compose] (text, keys, paste, render), then dev[tui-editor] (`Ctrl-E`, `edited`), in that
//! order.
#![expect(
    dead_code,
    reason = "ADR 0003 wave 0: used from wave 3 (dev[tui-compose])"
)]

use std::cell::Cell;

use ratatui::Frame;
use ratatui::layout::Rect;

use super::Shared;
use crate::{EditorError, KeyEvent};

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
        }
    }

    /// The whole text, lines joined with `\n`.
    pub(crate) fn text(&self) -> String {
        todo!("ADR 0003, wave 3: dev[tui-compose]")
    }

    /// Characters (with or without Shift, and AltGr = Ctrl+Alt on Windows), Enter (newline), Backspace, Delete,
    /// arrows, Home, End; `Ctrl-E` (#9): [`Shared::request_edit`] with the current text (until wave 4: ignored,
    /// returns [`TextKey::Handled`] without effect).
    pub(crate) fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> TextKey {
        let _ = (key, shared);
        todo!("ADR 0003, wave 3: dev[tui-compose]")
    }

    /// Inserts pasted text at the cursor (same filtering as typed text).
    pub(crate) fn paste(&mut self, text: &str) {
        let _ = text;
        todo!("ADR 0003, wave 3: dev[tui-compose]")
    }

    /// The editor's answer. `Ok`: replaces the text (trailing newlines removed, leading U+FEFF removed, same
    /// filtering as a paste), cursor at the end. `Err`: text unchanged, `Notice::Error` with the message of
    /// ADR 0003 § 5.
    pub(crate) fn edited(&mut self, result: Result<String, EditorError>, shared: &mut Shared) {
        let _ = (result, shared);
        todo!("ADR 0003, wave 4: dev[tui-editor]")
    }

    /// Draws the field in `area` and places the terminal cursor.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let _ = (frame, area);
        todo!("ADR 0003, wave 3: dev[tui-compose]")
    }
}
