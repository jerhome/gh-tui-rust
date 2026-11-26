//! Drawing the application state.

use gh_client::Repo;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::keys::BINDINGS;
use super::{App, RepoList};

/// Spinner frames shown before `Loading`.
const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

/// Draws the whole screen.
pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Line::from(" ghu · Repo list").bold(), header);

    let lines: Vec<Line<'_>> = match &app.repos {
        RepoList::Loading => Vec::new(),
        RepoList::Loaded(repos) if repos.is_empty() => vec![Line::from(" No repos.")],
        RepoList::Loaded(repos) => repos.iter().map(repo_line).collect(),
        RepoList::Failed(message) => vec![Line::from(format!(" {message}")).red()],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered())
            .wrap(Wrap { trim: false }),
        body,
    );

    let status = if app.is_busy() {
        let spin = SPINNER[app.spinner % SPINNER.len()];
        format!(" {spin} Loading…   ? help · q quit")
    } else {
        " ? help · q quit".to_owned()
    };
    frame.render_widget(Line::from(status), footer);

    if app.help {
        draw_help(frame, body);
    }
}

/// The key help, centred over `area`.
fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let lines: Vec<Line<'_>> = BINDINGS
        .iter()
        .map(|binding| {
            Line::from(vec![
                Span::raw(format!(" {:<8}", binding.label)).bold(),
                Span::raw(binding.what),
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

/// One row of the Repo list: name, markers, description.
fn repo_line(repo: &Repo) -> Line<'_> {
    let mut spans = vec![Span::styled(
        format!(" {}", repo.name),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    for (flag, label) in [
        (repo.fork, "fork"),
        (repo.private, "private"),
        (repo.archived, "archived"),
    ] {
        if flag {
            spans.push(Span::raw(format!(" [{label}]")).dim());
        }
    }
    if let Some(description) = &repo.description {
        spans.push(Span::raw(format!("  {description}")));
    }
    Line::from(spans)
}
