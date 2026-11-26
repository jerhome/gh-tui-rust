//! Drawing the frame: header, the top screen's body, footer, key help.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::keys::{GLOBAL, HelpRow, help_rows};
use super::{App, Notice};

/// Spinner frames shown before `Loading`.
const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

/// Draws the whole screen: header ` ghu · <title of the top screen>`, the top screen's body, then the footer: the
/// notice if any (red for an error), else `<spinner> Loading…` while busy, followed by `? help · q quit`.
pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let top = app.top();
    frame.render_widget(Line::from(format!(" ghu · {}", top.title())).bold(), header);
    top.render(frame, body);

    let line = match &app.shared.notice {
        Some(Notice::Info(text)) => Line::from(format!(" {text}")),
        Some(Notice::Error(text)) => Line::from(format!(" {text}")).red(),
        None if app.shared.is_busy() => {
            let spin = SPINNER[app.spinner % SPINNER.len()];
            Line::from(format!(" {spin} Loading…   ? help · q quit"))
        }
        None => Line::from(" ? help · q quit"),
    };
    frame.render_widget(line, footer);

    if app.help {
        let mut rows = top.help();
        rows.extend(help_rows(&GLOBAL));
        draw_help(frame, body, &rows);
    }
}

/// The key help, centred over `area`.
fn draw_help(frame: &mut Frame<'_>, area: Rect, rows: &[HelpRow]) {
    let lines: Vec<Line<'_>> = rows
        .iter()
        .map(|row| {
            Line::from(vec![
                Span::raw(format!(" {:<8}", row.label)).bold(),
                Span::raw(row.what),
            ])
        })
        .collect();
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(36)])
        .flex(Flex::Center)
        .areas(row);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys ")),
        popup,
    );
}
