//! Rendering.
//!
//! Layout mirrors the GUI: a session sidebar, the transcript, the composer, and
//! a status strip. The transcript is wrapped by hand so scrolling is measured in
//! visual lines rather than logical ones.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Line, Mode};
use crate::theme::Theme;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let theme = &app.theme;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(composer_height(app)),
            Constraint::Length(1),
        ])
        .split(area);

    let columns = if app.show_sessions {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(34), Constraint::Min(20)])
            .split(rows[0])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20)])
            .split(rows[0])
    };

    if app.show_sessions {
        draw_sessions(frame, columns[0], app);
    }
    draw_transcript(frame, *columns.last().unwrap(), app);
    draw_composer(frame, rows[1], app);
    draw_status(frame, rows[2], app);

    if app.mode == Mode::Approval {
        draw_approval(frame, area, app);
    }

    let _ = theme;
}

fn composer_height(app: &App) -> u16 {
    let lines = app.input.lines().count().max(1) as u16;
    (lines + 2).min(10)
}

fn draw_sessions(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme.line))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<TextLine> = Vec::new();
    lines.push(TextLine::from(Span::styled(
        "Sessions",
        Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
    )));
    lines.push(TextLine::from(""));
    if app.sessions.is_empty() {
        lines.push(TextLine::from(Span::styled(
            "no saved sessions",
            Style::default().fg(theme.faint),
        )));
    }
    for path in app.sessions.iter().take(inner.height.saturating_sub(3) as usize) {
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        lines.push(TextLine::from(Span::styled(
            format!("· {name}"),
            Style::default().fg(theme.text),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_transcript(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default().style(Style::default().bg(theme.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width.max(1) as usize;
    let mut rendered: Vec<TextLine> = Vec::new();
    for line in &app.lines {
        render_line(&mut rendered, line, theme, width);
    }

    // Scrolling is measured from the bottom so new output stays visible.
    let total = rendered.len();
    let height = inner.height as usize;
    let offset = if app.follow {
        total.saturating_sub(height)
    } else {
        total.saturating_sub(height).saturating_sub(app.scroll)
    };

    let visible: Vec<TextLine> = rendered.into_iter().skip(offset).take(height).collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

fn render_line(out: &mut Vec<TextLine<'static>>, line: &Line, theme: &Theme, width: usize) {
    match line {
        Line::User(text) => {
            out.push(TextLine::from(""));
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(TextLine::from(vec![
                    Span::styled("▌ ", Style::default().fg(theme.user)),
                    Span::styled(wrapped, Style::default().fg(theme.text)),
                ]));
            }
        }
        Line::Assistant(text) => {
            render_markdown(out, text, theme, width);
        }
        Line::Thinking(text) => {
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(TextLine::from(Span::styled(
                    format!("  {wrapped}"),
                    Style::default().fg(theme.faint).add_modifier(Modifier::ITALIC),
                )));
            }
        }
        Line::Tool { name, detail, running, is_error } => {
            let (marker, colour) = if *running {
                ("◐", theme.warn)
            } else if *is_error {
                ("✗", theme.bad)
            } else {
                ("✓", theme.ok)
            };
            let mut spans = vec![
                Span::styled(format!("{marker} "), Style::default().fg(colour)),
                Span::styled(name.clone(), Style::default().fg(theme.accent)),
            ];
            if !detail.is_empty() {
                spans.push(Span::styled(format!("  {detail}"), Style::default().fg(theme.dim)));
            }
            out.push(TextLine::from(spans));
        }
        Line::ToolOutput(text) => {
            for wrapped in wrap(text, width.saturating_sub(4)) {
                out.push(TextLine::from(Span::styled(
                    format!("    {wrapped}"),
                    Style::default().fg(theme.dim),
                )));
            }
        }
        Line::Notice { level, text } => {
            let colour = match level.as_str() {
                "warning" => theme.warn,
                "error" => theme.bad,
                _ => theme.faint,
            };
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(TextLine::from(Span::styled(
                    format!("· {wrapped}"),
                    Style::default().fg(colour),
                )));
            }
        }
        Line::Error(text) => {
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(TextLine::from(Span::styled(
                    format!("! {wrapped}"),
                    Style::default().fg(theme.bad),
                )));
            }
        }
    }
}

/// Render assistant text as markdown: fenced code is highlighted, inline code
/// and headings get their own styling, everything else is wrapped prose.
fn render_markdown(out: &mut Vec<TextLine<'static>>, text: &str, theme: &Theme, width: usize) {
    let mut in_fence = false;
    let mut language = String::new();
    let mut code = String::new();

    for raw in text.split('\n') {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                // Flush the block through the highlighter.
                for spans in crate::highlight::highlight(&code, &language) {
                    let converted: Vec<Span<'static>> =
                        spans.into_iter().map(|(text, style)| Span::styled(text, style)).collect();
                    out.push(TextLine::from(converted));
                }
                code.clear();
                language.clear();
                in_fence = false;
            } else {
                in_fence = true;
                language = trimmed.trim_start_matches('`').trim().to_string();
            }
            continue;
        }

        if in_fence {
            code.push_str(raw);
            code.push('\n');
            continue;
        }

        if let Some(heading) = trimmed.strip_prefix("### ") {
            out.push(TextLine::from(Span::styled(
                heading.to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ").or_else(|| trimmed.strip_prefix("# ")) {
            out.push(TextLine::from(Span::styled(
                heading.to_string(),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            for (index, wrapped) in wrap(item, width.saturating_sub(2)).into_iter().enumerate() {
                let prefix = if index == 0 { "• " } else { "  " };
                out.push(TextLine::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.faint)),
                    Span::styled(wrapped, Style::default().fg(theme.text)),
                ]));
            }
            continue;
        }

        for wrapped in wrap(raw, width) {
            out.push(TextLine::from(Span::styled(wrapped, Style::default().fg(theme.text))));
        }
    }

    // An unterminated fence still shows its code rather than swallowing it.
    if in_fence && !code.is_empty() {
        for spans in crate::highlight::highlight(&code, &language) {
            let converted: Vec<Span<'static>> =
                spans.into_iter().map(|(text, style)| Span::styled(text, style)).collect();
            out.push(TextLine::from(converted));
        }
    }
}

/// Wrap text to a width, preserving explicit newlines.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split(' ') {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn draw_composer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let title = if app.running { " message (steers the run) " } else { " message " };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.line))
        .title(Span::styled(title, Style::default().fg(theme.faint)))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if app.input.is_empty() {
        Span::styled("Ask Luna to build something…", Style::default().fg(theme.faint))
    } else {
        Span::styled(app.input.clone(), Style::default().fg(theme.text))
    };
    frame.render_widget(Paragraph::new(TextLine::from(text)).wrap(Wrap { trim: false }), inner);

    // Place the cursor where the user is typing.
    if app.mode == Mode::Normal {
        let before = &app.input[..app.cursor.min(app.input.len())];
        let row = before.matches('\n').count() as u16;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u16;
        let x = inner.x + column.min(inner.width.saturating_sub(1));
        let y = inner.y + row.min(inner.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let state = if app.running { "running" } else { "idle" };
    let state_colour = if app.running { theme.warn } else { theme.ok };

    let mut spans = vec![
        Span::styled(format!(" {state} "), Style::default().fg(state_colour)),
        Span::styled("│ ", Style::default().fg(theme.line)),
        Span::styled(app.model.clone(), Style::default().fg(theme.dim)),
        Span::styled(" │ ", Style::default().fg(theme.line)),
        Span::styled(format!("thinking {}", app.thinking.as_str()), Style::default().fg(theme.dim)),
        Span::styled(" │ ", Style::default().fg(theme.line)),
        Span::styled(
            format!("ctx {:.0}%", app.context_percent),
            Style::default().fg(if app.context_percent > 80.0 { theme.warn } else { theme.dim }),
        ),
        Span::styled(" │ ", Style::default().fg(theme.line)),
        Span::styled(
            format!("{} in · {} out", app.tokens_in, app.tokens_out),
            Style::default().fg(theme.dim),
        ),
    ];
    if app.cost > 0.0 {
        spans.push(Span::styled(" │ ", Style::default().fg(theme.line)));
        spans.push(Span::styled(format!("${:.4}", app.cost), Style::default().fg(theme.dim)));
    }
    if !app.agents.is_empty() {
        spans.push(Span::styled(" │ ", Style::default().fg(theme.line)));
        spans.push(Span::styled(
            format!("{} agents", app.agents.len()),
            Style::default().fg(theme.accent),
        ));
    }
    spans.push(Span::styled(" │ ", Style::default().fg(theme.line)));
    spans.push(Span::styled(app.status.clone(), Style::default().fg(theme.faint)));

    frame.render_widget(
        Paragraph::new(TextLine::from(spans)).style(Style::default().bg(theme.panel)),
        area,
    );
}

fn draw_approval(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(pending) = &app.pending else { return };

    let width = area.width.min(72);
    let height = 9.min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn))
        .title(Span::styled(" approval needed ", Style::default().fg(theme.warn)))
        .style(Style::default().bg(theme.raise));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        TextLine::from(Span::styled(pending.title.clone(), Style::default().fg(theme.text))),
        TextLine::from(""),
        TextLine::from(Span::styled(
            format!("{}: {}", pending.tool, pending.detail),
            Style::default().fg(theme.dim),
        )),
        TextLine::from(""),
        TextLine::from(vec![
            Span::styled("y", Style::default().fg(theme.ok).add_modifier(Modifier::BOLD)),
            Span::styled(" allow   ", Style::default().fg(theme.dim)),
            Span::styled("a", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" always   ", Style::default().fg(theme.dim)),
            Span::styled("n", Style::default().fg(theme.bad).add_modifier(Modifier::BOLD)),
            Span::styled(" deny", Style::default().fg(theme.dim)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_respects_the_width() {
        let wrapped = wrap("one two three four five", 10);
        assert!(wrapped.iter().all(|line| line.chars().count() <= 10));
        assert_eq!(wrapped.join(" "), "one two three four five");
    }

    #[test]
    fn wrapping_keeps_explicit_newlines() {
        let wrapped = wrap("a\n\nb", 20);
        assert_eq!(wrapped, vec!["a", "", "b"]);
    }

    #[test]
    fn wrapping_never_returns_nothing() {
        assert_eq!(wrap("", 20), vec![String::new()]);
    }
}
