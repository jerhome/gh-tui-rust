//! The Diff of one PR (#7): its changed files on the left, the patch of the selected one on the right.
//! Contract: ADR 0003 § 4 « Diff (#7) ». Loads with [`Client::pull_files`](gh_client::Client::pull_files)
//! ([`Outcome::Files`]). Every text from the network goes through [`text::clean`]; the patch is split with
//! `str::lines()` (so `\n` and `\r\n` end a line) and drawn without wrapping, windowed by screen columns (a CJK
//! character or an emoji takes two).

use std::cell::Cell;

use gh_client::{FileChange, FileStatus, PrNumber, RepoName};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::keys::{self, Binding};
use super::text::clean;
use super::{Flow, HelpRow, Outcome, RequestId, Shared, View, error_message};
use crate::{KeyCode, KeyEvent, KeyModifiers};

/// Columns of one `Left` / `Right` step.
const HSTEP: usize = 8;
/// Narrowest file list, in columns (borders included).
const MIN_LIST: u16 = 24;

/// What a key does on this screen.
#[derive(Clone, Copy)]
enum Action {
    Up,
    Down,
    PageUp,
    PageDown,
    Left,
    Right,
    Next,
    Previous,
    Refresh,
}

/// The key map of the screen, in the order of the help.
const TABLE: [Binding<Action>; 9] = [
    Binding {
        label: "n",
        what: "Next file",
        action: Action::Next,
        keys: &[(KeyCode::Char('n'), KeyModifiers::NONE)],
    },
    Binding {
        label: "p",
        what: "Previous file",
        action: Action::Previous,
        keys: &[(KeyCode::Char('p'), KeyModifiers::NONE)],
    },
    Binding {
        label: "Up/k",
        what: "Scroll up",
        action: Action::Up,
        keys: &[
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Char('k'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Down/j",
        what: "Scroll down",
        action: Action::Down,
        keys: &[
            (KeyCode::Down, KeyModifiers::NONE),
            (KeyCode::Char('j'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "PgUp",
        what: "Page up",
        action: Action::PageUp,
        keys: &[(KeyCode::PageUp, KeyModifiers::NONE)],
    },
    Binding {
        label: "PgDn",
        what: "Page down",
        action: Action::PageDown,
        keys: &[(KeyCode::PageDown, KeyModifiers::NONE)],
    },
    Binding {
        label: "Left/h",
        what: "Scroll left",
        action: Action::Left,
        keys: &[
            (KeyCode::Left, KeyModifiers::NONE),
            (KeyCode::Char('h'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Right/l",
        what: "Scroll right",
        action: Action::Right,
        keys: &[
            (KeyCode::Right, KeyModifiers::NONE),
            (KeyCode::Char('l'), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "r",
        what: "Reload",
        action: Action::Refresh,
        keys: &[(KeyCode::Char('r'), KeyModifiers::NONE)],
    },
];

/// What the screen has to show.
enum Load {
    /// The first answer has not come yet.
    Loading,
    /// The changed files; kept while a reload is in flight.
    Loaded(Vec<FileChange>),
    /// The request failed: a message for the user.
    Failed(String),
}

/// The Diff screen.
pub(crate) struct DiffScreen {
    repo: RepoName,
    number: PrNumber,
    load: Load,
    /// The latest request; only its answer is applied.
    pending: Option<RequestId>,
    /// Index of the selected file (always valid for a loaded list, brought back within by every use).
    selected: usize,
    /// First patch row shown, and first column shown. May exceed the bounds measured by the last render: every
    /// use brings them back within.
    scroll: usize,
    hscroll: usize,
    /// Measured by the last render (key handling has no frame): largest useful `scroll`, largest useful
    /// `hscroll`, rows of the patch view, first row of the file list shown.
    max_scroll: Cell<usize>,
    max_hscroll: Cell<usize>,
    page: Cell<usize>,
    list_offset: Cell<usize>,
}

impl DiffScreen {
    /// Opened by `d` on the detail; starts loading the files at once.
    pub(crate) fn new(repo: RepoName, number: PrNumber, shared: &mut Shared) -> Self {
        let mut screen = Self::empty(repo, number);
        screen.load(shared);
        screen
    }

    fn empty(repo: RepoName, number: PrNumber) -> Self {
        Self {
            repo,
            number,
            load: Load::Loading,
            pending: None,
            selected: 0,
            scroll: 0,
            hscroll: 0,
            max_scroll: Cell::new(0),
            max_hscroll: Cell::new(0),
            page: Cell::new(0),
            list_offset: Cell::new(0),
        }
    }

    /// A screen already loaded with `files`, for the drawing tests.
    #[cfg(test)]
    fn loaded(repo: RepoName, number: PrNumber, files: Vec<FileChange>) -> Self {
        let mut screen = Self::empty(repo, number);
        screen.load = Load::Loaded(files);
        screen
    }

    /// Requests the files; the content already shown stays until the answer comes.
    fn load(&mut self, shared: &mut Shared) {
        let client = shared.client().clone();
        let repo = self.repo.clone();
        let number = self.number;
        self.pending = Some(
            shared.spawn(async move { Outcome::Files(client.pull_files(&repo, number).await) }),
        );
    }

    fn files(&self) -> &[FileChange] {
        match &self.load {
            Load::Loaded(files) => files,
            Load::Loading | Load::Failed(_) => &[],
        }
    }

    /// Moves the selection by `step` (-1 or 1), stopping at both ends; a move resets the scrolling.
    fn select(&mut self, step: isize) {
        let last = self.files().len().saturating_sub(1);
        let next = self
            .selected
            .min(last)
            .saturating_add_signed(step)
            .min(last);
        if next != self.selected {
            self.selected = next;
            self.scroll = 0;
            self.hscroll = 0;
        }
    }

    fn scroll_down(&mut self, rows: usize) {
        self.scroll = self.scroll.saturating_add(rows).min(self.max_scroll.get());
    }

    fn scroll_up(&mut self, rows: usize) {
        self.scroll = self.scroll.min(self.max_scroll.get()).saturating_sub(rows);
    }

    /// A page of scrolling: the rows of the view but one, so the last line stays in view.
    fn page_rows(&self) -> usize {
        self.page.get().saturating_sub(1).max(1)
    }

    /// Replaces the files after a (re)load: the selection stays on the same path if it is still there.
    fn set_files(&mut self, files: Vec<FileChange>) {
        let path = self
            .files()
            .get(self.selected)
            .map(|file| file.path.clone());
        let same = path.and_then(|path| files.iter().position(|file| file.path == path));
        match same {
            Some(index) => self.selected = index,
            None => {
                self.selected = self.selected.min(files.len().saturating_sub(1));
                self.scroll = 0;
                self.hscroll = 0;
            }
        }
        self.load = Load::Loaded(files);
    }
}

impl View for DiffScreen {
    /// `<owner/name> · #<n> · Diff`.
    fn title(&self) -> String {
        format!("{} · {} · Diff", self.repo, self.number)
    }

    fn help(&self) -> Vec<HelpRow> {
        keys::help_rows(&TABLE)
    }

    fn key(&mut self, key: KeyEvent, shared: &mut Shared) -> Flow {
        match keys::lookup(&TABLE, key) {
            Some(Action::Next) => self.select(1),
            Some(Action::Previous) => self.select(-1),
            Some(Action::Up) => self.scroll_up(1),
            Some(Action::Down) => self.scroll_down(1),
            Some(Action::PageUp) => self.scroll_up(self.page_rows()),
            Some(Action::PageDown) => self.scroll_down(self.page_rows()),
            Some(Action::Left) => {
                self.hscroll = self
                    .hscroll
                    .min(self.max_hscroll.get())
                    .saturating_sub(HSTEP)
            }
            Some(Action::Right) => {
                self.hscroll = self
                    .hscroll
                    .saturating_add(HSTEP)
                    .min(self.max_hscroll.get());
            }
            Some(Action::Refresh) => self.load(shared),
            None => return Flow::Ignored,
        }
        Flow::Handled
    }

    fn apply(&mut self, id: RequestId, outcome: Outcome, _shared: &mut Shared) -> Option<Outcome> {
        if self.pending != Some(id) {
            return Some(outcome);
        }
        self.pending = None;
        match outcome {
            Outcome::Files(Ok(files)) => {
                tracing::debug!(files = files.len(), "changed files loaded");
                self.set_files(files);
            }
            Outcome::Files(Err(error)) => {
                tracing::warn!(%error, "loading the changed files failed");
                self.load = Load::Failed(error_message("load the changed files", &error));
            }
            Outcome::TaskFailed => self.load = Load::Failed(super::TASK_FAILED.to_owned()),
            other => return Some(other),
        }
        None
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        match &self.load {
            Load::Loading => frame.render_widget(Block::bordered(), area),
            Load::Failed(message) => frame.render_widget(
                Paragraph::new(Line::from(format!(" {message}")).red())
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered()),
                area,
            ),
            Load::Loaded(files) if files.is_empty() => frame.render_widget(
                Paragraph::new(" No changed files.").block(Block::bordered()),
                area,
            ),
            Load::Loaded(files) => self.render_files(frame, area, files),
        }
    }
}

impl DiffScreen {
    /// The two panes: the file list (a third of the width, at least [`MIN_LIST`] columns) and the selected file.
    fn render_files(&self, frame: &mut Frame<'_>, area: Rect, files: &[FileChange]) {
        let list_width = (area.width / 3).max(MIN_LIST).min(area.width);
        let [left, right] =
            Layout::horizontal([Constraint::Length(list_width), Constraint::Min(0)]).areas(area);
        let selected = self.selected.min(files.len() - 1);
        self.render_list(frame, left, files, selected);
        self.render_patch(frame, right, &files[selected]);
    }

    fn render_list(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        files: &[FileChange],
        selected: usize,
    ) {
        let block = Block::bordered().title(format!(" Files ({}) ", files.len()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = usize::from(inner.height);
        let width = usize::from(inner.width);
        // Keep the selected file in view, moving the window as little as possible.
        let mut offset = self.list_offset.get().min(files.len().saturating_sub(1));
        if selected < offset {
            offset = selected;
        }
        if rows > 0 && selected >= offset + rows {
            offset = selected + 1 - rows;
        }
        self.list_offset.set(offset);
        let lines: Vec<Line<'static>> = files
            .iter()
            .enumerate()
            .skip(offset)
            .take(rows)
            .map(|(index, file)| list_line(file, index == selected, width))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_patch(&self, frame: &mut Frame<'_>, area: Rect, file: &FileChange) {
        let block = Block::bordered();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [header, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let width = usize::from(inner.width);
        frame.render_widget(
            Line::from(truncate_cols(&header_text(file), width)).bold(),
            header,
        );
        let rows = usize::from(body.height);
        self.page.set(rows);
        let Some(patch) = file.patch.as_deref() else {
            self.max_scroll.set(0);
            self.max_hscroll.set(0);
            frame.render_widget(
                Paragraph::new(clean(&missing_patch_message(file)).into_owned())
                    .wrap(Wrap { trim: false }),
                body,
            );
            return;
        };
        let (mut count, mut longest) = (0_usize, 0_usize);
        for line in patch.lines() {
            count += 1;
            longest = longest.max(cols_of(&clean(line)));
        }
        let max_scroll = count.saturating_sub(rows);
        let max_hscroll = longest.saturating_sub(width);
        self.max_scroll.set(max_scroll);
        self.max_hscroll.set(max_hscroll);
        let scroll = self.scroll.min(max_scroll);
        let hscroll = self.hscroll.min(max_hscroll);
        let lines: Vec<Line<'static>> = patch
            .lines()
            .skip(scroll)
            .take(rows)
            .map(|raw| {
                Line::from(Span::styled(
                    window_cols(&clean(raw), hscroll, width),
                    patch_style(raw),
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
    }
}

/// The status letter of a file in the list.
fn status_letter(status: FileStatus) -> char {
    match status {
        FileStatus::Added => 'A',
        FileStatus::Removed => 'D',
        FileStatus::Modified | FileStatus::Changed => 'M',
        FileStatus::Renamed => 'R',
        FileStatus::Copied => 'C',
        FileStatus::Unchanged => ' ',
    }
}

/// One row of the file list: `> M +2 -1 path`, the path cut with `…` to fit `width` columns.
fn list_line(file: &FileChange, selected: bool, width: usize) -> Line<'static> {
    let mark = if selected { '>' } else { ' ' };
    let prefix = format!(
        "{mark} {} +{} -{} ",
        status_letter(file.status),
        file.additions,
        file.deletions
    );
    let room = width.saturating_sub(cols_of(&prefix));
    let text = format!("{prefix}{}", truncate_cols(&clean(&file.path), room));
    let line = Line::from(truncate_cols(&text, width));
    if selected { line.bold() } else { line }
}

/// `<path>`, or `<previous> → <path>` when the file has a previous name.
fn header_text(file: &FileChange) -> String {
    match &file.previous_path {
        Some(previous) => format!("{} → {}", clean(previous), clean(&file.path)),
        None => clean(&file.path).into_owned(),
    }
}

/// Why there is no patch to show.
fn missing_patch_message(file: &FileChange) -> String {
    match (
        &file.previous_path,
        file.status,
        file.additions,
        file.deletions,
    ) {
        (Some(previous), FileStatus::Renamed, 0, 0) => {
            format!("Renamed from {previous}, no content change.")
        }
        _ => "No diff from GitHub: binary file, or too large to show.".to_owned(),
    }
}

/// The colour of a patch line, from its first character: `+` green, `-` red, `@@` cyan, `` dimmed.
fn patch_style(line: &str) -> Style {
    if line.starts_with("@@") {
        Style::new().fg(Color::Cyan)
    } else if line.starts_with('+') {
        Style::new().fg(Color::Green)
    } else if line.starts_with('-') {
        Style::new().fg(Color::Red)
    } else if line.starts_with('\\') {
        Style::new().add_modifier(Modifier::DIM)
    } else {
        Style::new()
    }
}

/// Screen columns taken by one character (0 for a combining mark, 2 for CJK and most emoji).
fn char_cols(c: char) -> usize {
    let mut buffer = [0; 4];
    Span::raw(&*c.encode_utf8(&mut buffer)).width()
}

/// Screen columns taken by `text`.
fn cols_of(text: &str) -> usize {
    text.chars().map(char_cols).sum()
}

/// `text` cut to at most `width` columns; when it does not fit, the last column holds `…`.
fn truncate_cols(text: &str, width: usize) -> String {
    if cols_of(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cols = char_cols(c);
        if used + cols + 1 > width {
            break;
        }
        out.push(c);
        used += cols;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// The part of `text` from column `skip` on, at most `width` columns. A wide character cut by the left edge
/// becomes spaces for its visible column (nothing is dropped without a trace); one that does not fit at the right
/// edge is left out.
fn window_cols(text: &str, skip: usize, width: usize) -> String {
    let mut out = String::new();
    let mut position = 0;
    let mut used = 0;
    for c in text.chars() {
        let cols = char_cols(c);
        if position < skip {
            position += cols;
            if position > skip {
                let visible = (position - skip).min(width);
                out.extend(std::iter::repeat_n(' ', visible));
                used += visible;
            }
            continue;
        }
        if cols == 0 && out.is_empty() && skip > 0 {
            // A combining mark whose base character was skipped.
            continue;
        }
        if used + cols > width {
            break;
        }
        out.push(c);
        position += cols;
        used += cols;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gh_client::{ApiBase, Client, Token};
    use gh_fake::{FakeGitHub, Pull, PullFile, Repo, State, User};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    /// The files of PR #1 of `alice/notes` as the client reads them from the fake: the model types are built by
    /// `gh-client` only.
    async fn fetch(files: Vec<PullFile>) -> Vec<FileChange> {
        let mut state = State::new(User {
            login: "alice".into(),
            id: 1,
        });
        state.repos = vec![Repo::new("alice", "notes")];
        let mut pull = Pull::new("alice", "notes", 1, "Title", "bob");
        pull.files = files;
        state.pulls = vec![pull];
        let token = state.token.clone();
        let fake = FakeGitHub::start(state).await.expect("fake server");
        let client = Client::new(
            ApiBase::parse(&fake.base_url()).expect("base url"),
            Token::new(token).expect("token"),
        )
        .expect("client");
        let name = RepoName::new("alice", "notes").expect("repo name");
        let number = PrNumber::new(1).expect("number");
        client.pull_files(&name, number).await.expect("files")
    }

    async fn screen_with(files: Vec<PullFile>) -> DiffScreen {
        let files = fetch(files).await;
        DiffScreen::loaded(
            RepoName::new("alice", "notes").expect("repo name"),
            PrNumber::new(1).expect("number"),
            files,
        )
    }

    /// Draws `screen` on a `width` x `height` terminal and returns the buffer.
    fn draw(screen: &DiffScreen, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| screen.render(frame, frame.area()))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    /// The cell where `needle` starts in a row of `buffer` (cells joined; the file list is skipped by starting
    /// at column `from`).
    fn find(buffer: &Buffer, needle: &str, from: u16) -> (u16, u16) {
        for y in 0..buffer.area.height {
            let row: Vec<&str> = (from..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            for start in 0..row.len() {
                if row[start..].concat().starts_with(needle) {
                    return (from + u16::try_from(start).expect("column"), y);
                }
            }
        }
        panic!("{needle:?} not in the buffer: {buffer:?}");
    }

    fn patch_file(patch: &str) -> PullFile {
        PullFile::modified("a.rs", patch)
    }

    #[tokio::test]
    async fn patch_lines_are_coloured_in_the_buffer() {
        let screen = screen_with(vec![patch_file(
            "@@ -1,3 +1,3 @@\n context\n+added\n-removed\n\\ No newline at end of file",
        )])
        .await;
        let buffer = draw(&screen, 100, 20);
        let style_of = |needle: &str| {
            let (x, y) = find(&buffer, needle, 34);
            let cell = &buffer[(x, y)];
            (cell.fg, cell.modifier)
        };
        assert_eq!(style_of("@@ -1,3").0, Color::Cyan);
        assert_eq!(style_of("+added").0, Color::Green);
        assert_eq!(style_of("-removed").0, Color::Red);
        assert_eq!(style_of("\\ No newline").1, Modifier::DIM);
        let context = style_of(" context");
        assert_eq!(context, (Color::Reset, Modifier::empty()));
        // The whole line has the colour, to its last character.
        let (x, y) = find(&buffer, "+added", 34);
        assert_eq!(buffer[(x + 5, y)].fg, Color::Green);
    }

    #[tokio::test]
    async fn the_colour_follows_the_line_not_its_visible_start() {
        let long = format!("+{}", "x".repeat(120));
        let mut screen = screen_with(vec![patch_file(&format!(
            "@@ -1 +1 @@\n{long}\n-{}",
            "y".repeat(120)
        ))])
        .await;
        screen.hscroll = 8;
        let buffer = draw(&screen, 100, 20);
        let (x, y) = find(&buffer, "xxxx", 34);
        assert_eq!(buffer[(x, y)].fg, Color::Green);
        let (x, y) = find(&buffer, "yyyy", 34);
        assert_eq!(buffer[(x, y)].fg, Color::Red);
    }

    #[tokio::test]
    async fn only_the_selected_file_is_bold_in_the_list() {
        let mut screen =
            screen_with(vec![patch_file("+a"), PullFile::modified("b.rs", "+b")]).await;
        let buffer = draw(&screen, 100, 20);
        let (x, y) = find(&buffer, "> M", 0);
        assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
        let (x, y) = find(&buffer, "M +1 -0 b.rs", 0);
        assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
        screen.select(1);
        let buffer = draw(&screen, 100, 20);
        let (x, y) = find(&buffer, "> M +1 -0 b.rs", 0);
        assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    }

    #[tokio::test]
    async fn a_reload_keeps_the_selection_on_the_same_path() {
        let files = fetch(vec![
            PullFile::modified("a.rs", "+a"),
            PullFile::modified("b.rs", "+b"),
            PullFile::modified("c.rs", "+c"),
        ])
        .await;
        let mut screen = DiffScreen::loaded(
            RepoName::new("alice", "notes").expect("repo name"),
            PrNumber::new(1).expect("number"),
            files,
        );
        screen.select(1);
        screen.scroll = 3;
        // "b.rs" moved to the end: still selected, scroll kept.
        let reordered = fetch(vec![
            PullFile::modified("a.rs", "+a"),
            PullFile::modified("c.rs", "+c"),
            PullFile::modified("b.rs", "+b"),
        ])
        .await;
        screen.set_files(reordered);
        assert_eq!((screen.selected, screen.scroll), (2, 3));
        // "b.rs" is gone: the index stays within the list and the scroll is reset.
        let shorter = fetch(vec![PullFile::modified("a.rs", "+a")]).await;
        screen.set_files(shorter);
        assert_eq!((screen.selected, screen.scroll), (0, 0));
        screen.set_files(Vec::new());
        assert_eq!(screen.selected, 0);
    }

    #[test]
    fn the_key_table_uses_no_reserved_key_and_its_help_fits() {
        assert!(!keys::uses_reserved_key(&TABLE));
        for row in keys::help_rows(&TABLE) {
            assert!(row.label.chars().count() <= 8, "{}", row.label);
            assert!(row.what.chars().count() <= 26, "{}", row.what);
        }
    }

    #[test]
    fn window_cols_cuts_by_columns() {
        assert_eq!(window_cols("abcdef", 0, 4), "abcd");
        assert_eq!(window_cols("abcdef", 2, 3), "cde");
        assert_eq!(window_cols("abc", 5, 3), "");
        assert_eq!(window_cols("abc", 0, 0), "");
        // A wide character takes two columns: it is left out when only one is left.
        assert_eq!(window_cols("a日b", 0, 2), "a");
        assert_eq!(window_cols("a日b", 0, 3), "a日");
        assert_eq!(window_cols("日本語", 0, 5), "日本");
        // Cut by the left edge: its visible column becomes a space, the rest keeps its place.
        assert_eq!(window_cols("日本語", 1, 6), " 本語");
        assert_eq!(window_cols("日本語", 2, 6), "本語");
        assert_eq!(window_cols("日本語", 3, 6), " 語");
        // A combining mark follows its base character, and is dropped with it.
        assert_eq!(window_cols("e\u{301}x", 0, 2), "e\u{301}x");
        assert_eq!(window_cols("e\u{301}x", 1, 2), "x");
    }

    #[test]
    fn truncate_cols_marks_the_cut_and_fits_the_width() {
        assert_eq!(truncate_cols("abc", 3), "abc");
        assert_eq!(truncate_cols("abcd", 3), "ab…");
        assert_eq!(truncate_cols("abcd", 1), "…");
        assert_eq!(truncate_cols("abcd", 0), "");
        assert_eq!(truncate_cols("", 0), "");
        // Columns, not characters: 3 wide characters are 6 columns.
        assert_eq!(truncate_cols("日本語", 6), "日本語");
        assert_eq!(truncate_cols("日本語", 5), "日本…");
        assert_eq!(truncate_cols("日本語", 4), "日…");
        assert_eq!(truncate_cols("日本語", 2), "…");
    }

    /// A small deterministic generator (xorshift), as in the other modules.
    fn generator(seed: u64) -> impl FnMut() -> usize {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % 1000).expect("small")
        }
    }

    #[test]
    fn column_helpers_never_overflow_their_width_nor_panic() {
        const POOL: [&str; 9] = ["a", "é", "日", "😀", " ", "\u{301}", "…", "\u{200d}", "x"];
        let mut next = generator(0x2545_f491_4f6c_dd1d);
        for _ in 0..2000 {
            let text: String = (0..next() % 12)
                .map(|_| POOL[next() % POOL.len()])
                .collect();
            let (skip, width) = (next() % 16, next() % 16);
            assert!(
                cols_of(&window_cols(&text, skip, width)) <= width,
                "{text:?} {skip} {width}"
            );
            let cut = truncate_cols(&text, width);
            assert!(cols_of(&cut) <= width, "{text:?} {width} -> {cut:?}");
            if cols_of(&text) <= width {
                assert_eq!(cut, text);
            } else if width > 0 {
                assert!(cut.ends_with('…'), "{text:?} {width} -> {cut:?}");
            }
        }
    }

    #[tokio::test]
    async fn drawing_never_panics_whatever_the_size_and_the_content() {
        let mut renamed = PullFile::modified("ren\u{1b}amed", "");
        renamed.patch = None;
        renamed.previous_filename = Some("old\u{202e}".to_owned());
        renamed.status = gh_fake::FileStatus::Renamed;
        let mut screen = screen_with(vec![
            PullFile::modified(
                "日本語/😀.rs",
                "@@\n+日本語😀\r\n-\u{1b}[31m\n\\\n+a\rb\n+\t\u{202e}",
            ),
            renamed,
            PullFile::modified("empty.rs", ""),
        ])
        .await;
        for (selected, scroll, hscroll) in [(0, 0, 0), (1, 5, 5), (2, 1000, 1000), (99, 99, 99)] {
            (screen.selected, screen.scroll, screen.hscroll) = (selected, scroll, hscroll);
            for width in [0, 1, 2, 5, 23, 24, 25, 40, 100] {
                for height in [0, 1, 2, 3, 4, 10, 30] {
                    draw(&screen, width, height);
                }
            }
        }
        let loading = DiffScreen::empty(
            RepoName::new("alice", "notes").expect("repo name"),
            PrNumber::new(1).expect("number"),
        );
        draw(&loading, 0, 0);
        draw(&loading, 30, 5);
    }
}
