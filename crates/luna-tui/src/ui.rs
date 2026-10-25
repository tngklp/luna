//! Rendering.
//!
//! Layout, top to bottom: a header with the model and config source, the
//! transcript, the slash-command palette when it is open, the composer, and a
//! status strip. The transcript is wrapped by hand so scrolling is measured in
//! visual lines rather than logical ones.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Line, Mode};
use crate::theme::Theme;

/// Rows the palette occupies, including its border.
///
/// A palette with no matches still gets a row, so it can say so instead of
/// silently vanishing.
fn palette_height(app: &App) -> u16 {
    match &app.palette {
        Some(palette) => {
            let rows = palette.matches().len().clamp(1, 8) as u16;
            rows + 2
        }
        None => 0,
    }
}

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let theme = &app.theme;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(palette_height(app)),
            Constraint::Length(composer_height(app)),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(frame, rows[0], app);

    let columns = if app.show_sessions {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(34), Constraint::Min(20)])
            .split(rows[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20)])
            .split(rows[1])
    };

    if app.show_sessions {
        draw_sessions(frame, columns[0], app);
    }
    draw_transcript(frame, *columns.last().unwrap(), app);

    if palette_height(app) > 0 {
        draw_palette(frame, rows[2], app);
    }
    draw_composer(frame, rows[3], app);
    draw_status(frame, rows[4], app);

    if app.mode == Mode::Approval {
        draw_approval(frame, area, app);
    }

    let _ = theme;
}

fn composer_height(app: &App) -> u16 {
    let lines = app.input.lines().count().max(1) as u16;
    (lines + 2).min(10)
}

/// The top bar: what this is, which model, and where the config came from.
fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let mut spans = vec![
        Span::styled(" ☾ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("luna", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Span::styled("  ", Style::default()),
        Span::styled(app.model.clone(), Style::default().fg(theme.muted)),
    ];

    if !app.agents.is_empty() {
        spans.push(Span::styled("  ·  ", Style::default().fg(theme.line)));
        spans.push(Span::styled(
            format!("{} agent{}", app.agents.len(), if app.agents.len() == 1 { "" } else { "s" }),
            Style::default().fg(theme.border_accent),
        ));
    }

    // The config path is long, so it is right-aligned and truncated to whatever
    // room is left after the model.
    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let room = (area.width as usize).saturating_sub(used + 2);
    if room > 12 {
        let path = truncate_left(&app.config_path, room);
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(path, Style::default().fg(theme.dim)));
    }

    frame.render_widget(
        Paragraph::new(TextLine::from(spans)).style(Style::default().bg(theme.panel)),
        area,
    );
}

/// Keep the end of a path, which is the part that identifies it.
fn truncate_left(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let skip = count - width.saturating_sub(1);
    format!("…{}", text.chars().skip(skip).collect::<String>())
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
        " Sessions",
        Style::default().fg(theme.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(TextLine::from(""));
    if app.sessions.is_empty() {
        lines.push(TextLine::from(Span::styled(
            "  no saved sessions",
            Style::default().fg(theme.dim),
        )));
    }
    for path in app.sessions.iter().take(inner.height.saturating_sub(3) as usize) {
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        lines.push(TextLine::from(Span::styled(
            format!("  {name}"),
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

    if app.lines.is_empty() {
        draw_empty_state(&mut rendered, app, width);
    } else {
        for line in &app.lines {
            render_line(&mut rendered, line, theme, width);
        }
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

/// What the transcript shows before the first message.
///
/// Deliberately quiet: the header already names the model, so this only points
/// at the two things a new user needs — how to send, and how to find commands.
fn draw_empty_state(out: &mut Vec<TextLine<'static>>, app: &App, width: usize) {
    let theme = &app.theme;
    let pad = width.saturating_sub(48) / 2;
    let indent = " ".repeat(pad);

    out.push(TextLine::from(""));
    out.push(TextLine::from(""));
    out.push(TextLine::from(Span::styled(
        format!("{indent}☾"),
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    out.push(TextLine::from(""));
    out.push(TextLine::from(Span::styled(
        format!("{indent}Ask Luna to build something."),
        Style::default().fg(theme.text),
    )));
    out.push(TextLine::from(""));
    out.push(TextLine::from(vec![
        Span::styled(format!("{indent}Type "), Style::default().fg(theme.dim)),
        Span::styled("/", Style::default().fg(theme.accent)),
        Span::styled(" for commands", Style::default().fg(theme.dim)),
        Span::styled("  ·  ", Style::default().fg(theme.line)),
        Span::styled("Enter", Style::default().fg(theme.muted)),
        Span::styled(" to send", Style::default().fg(theme.dim)),
    ]));
}

fn render_line(out: &mut Vec<TextLine<'static>>, line: &Line, theme: &Theme, width: usize) {
    match line {
        Line::User(text) => {
            out.push(TextLine::from(""));
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(
                    TextLine::from(vec![
                        Span::styled("▌ ", Style::default().fg(theme.accent)),
                        Span::styled(wrapped, Style::default().fg(theme.user_text)),
                    ])
                    .style(Style::default().bg(theme.user_bg)),
                );
            }
        }
        Line::Assistant(text) => {
            render_markdown(out, text, theme, width);
        }
        Line::Thinking(text) => {
            for wrapped in wrap(text, width.saturating_sub(2)) {
                out.push(TextLine::from(Span::styled(
                    format!("  {wrapped}"),
                    Style::default().fg(theme.thinking_text).add_modifier(Modifier::ITALIC),
                )));
            }
        }
        Line::Tool { name, detail, running, is_error } => {
            let (marker, colour, background) = if *running {
                ("◐", theme.warning, theme.tool_pending_bg)
            } else if *is_error {
                ("✗", theme.error, theme.tool_error_bg)
            } else {
                ("✓", theme.success, theme.tool_success_bg)
            };
            let mut spans = vec![
                Span::styled(format!("{marker} "), Style::default().fg(colour)),
                Span::styled(name.clone(), Style::default().fg(theme.tool_title)),
            ];
            if !detail.is_empty() {
                spans.push(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(theme.tool_output),
                ));
            }
            out.push(TextLine::from(spans).style(Style::default().bg(background)));
        }
        Line::ToolOutput(text) => {
            for wrapped in wrap(text, width.saturating_sub(4)) {
                out.push(TextLine::from(Span::styled(
                    format!("    {wrapped}"),
                    Style::default().fg(theme.tool_output),
                )));
            }
        }
        Line::Notice { level, text } => {
            let colour = match level.as_str() {
                "warning" => theme.warning,
                "error" => theme.error,
                _ => theme.dim,
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
                    Style::default().fg(theme.error),
                )));
            }
        }
    }
}

/// Render assistant text as markdown: fenced code is highlighted, headings and
/// bullets get their own styling, everything else is wrapped prose.
fn render_markdown(out: &mut Vec<TextLine<'static>>, text: &str, theme: &Theme, width: usize) {
    let mut in_fence = false;
    let mut language = String::new();
    let mut code = String::new();

    for raw in text.split('\n') {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                flush_code(out, &code, &language, theme);
                code.clear();
                language.clear();
                in_fence = false;
                // Close the block so the prose after it does not read as code.
                out.push(TextLine::from(Span::styled(
                    "```",
                    Style::default().fg(theme.md_code_border),
                )));
            } else {
                in_fence = true;
                language = trimmed.trim_start_matches('`').trim().to_string();
                // The fence itself is shown, dimmed, so a long block has a
                // visible boundary.
                out.push(TextLine::from(Span::styled(
                    format!("```{language}"),
                    Style::default().fg(theme.md_code_border),
                )));
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
                Style::default().fg(theme.md_heading).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            for (index, wrapped) in wrap(item, width.saturating_sub(2)).into_iter().enumerate() {
                let prefix = if index == 0 { "• " } else { "  " };
                out.push(TextLine::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.md_bullet)),
                    Span::styled(wrapped, Style::default().fg(theme.text)),
                ]));
            }
            continue;
        }

        // A diff line is worth colouring: it is the one place where the sign
        // carries meaning the words do not.
        if let Some(added) = raw.strip_prefix("+ ") {
            out.push(TextLine::from(Span::styled(
                format!("+ {added}"),
                Style::default().fg(theme.diff_added),
            )));
            continue;
        }
        if let Some(removed) = raw.strip_prefix("- ") {
            out.push(TextLine::from(Span::styled(
                format!("- {removed}"),
                Style::default().fg(theme.diff_removed),
            )));
            continue;
        }

        for wrapped in wrap(raw, width) {
            out.push(TextLine::from(inline_spans(&wrapped, theme)));
        }
    }

    // An unterminated fence still shows its code rather than swallowing it.
    if in_fence && !code.is_empty() {
        flush_code(out, &code, &language, theme);
    }
}

/// Style inline code and links inside a prose line.
///
/// A full inline parser would be overkill for a terminal; backticks and bare
/// URLs cover what models actually emit.
fn inline_spans(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let code_at = rest.find('`');
        let url_at = rest.find("http://").or_else(|| rest.find("https://"));

        let next = match (code_at, url_at) {
            (Some(code), Some(url)) if code < url => Some((code, true)),
            (Some(_), Some(url)) => Some((url, false)),
            (Some(code), None) => Some((code, true)),
            (None, Some(url)) => Some((url, false)),
            (None, None) => None,
        };

        let Some((index, is_code)) = next else {
            spans.push(Span::styled(rest.to_string(), Style::default().fg(theme.text)));
            break;
        };

        if index > 0 {
            spans.push(Span::styled(rest[..index].to_string(), Style::default().fg(theme.text)));
        }
        let tail = &rest[index..];

        if is_code {
            match tail[1..].find('`') {
                Some(end) => {
                    let code = &tail[1..1 + end];
                    spans.push(Span::styled(code.to_string(), Style::default().fg(theme.md_code)));
                    rest = &tail[end + 2..];
                }
                None => {
                    spans.push(Span::styled(tail.to_string(), Style::default().fg(theme.text)));
                    break;
                }
            }
        } else {
            let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            spans.push(Span::styled(tail[..end].to_string(), Style::default().fg(theme.md_link)));
            rest = &tail[end..];
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(theme.text)));
    }
    spans
}

/// Highlight a block of code, falling back to the theme's code colour when the
/// language is unknown.
fn flush_code(out: &mut Vec<TextLine<'static>>, code: &str, language: &str, theme: &Theme) {
    let lines = crate::highlight::highlight(code, language);
    if lines.is_empty() {
        return;
    }
    for spans in lines {
        let converted: Vec<Span<'static>> = spans
            .into_iter()
            .map(|(text, style)| {
                // The highlighter returns the default foreground for plain text;
                // the theme's code colour reads better against the transcript.
                let style = if style.fg.is_none() {
                    Style::default().fg(theme.md_code_block)
                } else {
                    style
                };
                Span::styled(text, style)
            })
            .collect();
        out.push(TextLine::from(converted));
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

/// The slash-command list, shown above the composer while it is open.
fn draw_palette(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(palette) = &app.palette else { return };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if palette.is_empty() {
        frame.render_widget(
            Paragraph::new(TextLine::from(Span::styled(
                format!("  no command matches /{}", palette.query()),
                Style::default().fg(theme.dim),
            ))),
            inner,
        );
        return;
    }

    let matches = palette.matches();
    let selected = palette.selected();
    let name_width = matches.iter().map(|command| command.name.len()).max().unwrap_or(0);
    let mut lines: Vec<TextLine> = Vec::new();

    for (index, command) in matches.iter().take(inner.height as usize).enumerate() {
        let is_selected = index == selected;
        let marker = if is_selected { "› " } else { "  " };
        let name_style = if is_selected {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let mut spans = vec![
            Span::styled(marker, Style::default().fg(theme.accent)),
            Span::styled(format!("{:<width$}", command.name, width = name_width), name_style),
            Span::styled("  ", Style::default()),
            Span::styled(
                command.description,
                Style::default().fg(if is_selected { theme.muted } else { theme.dim }),
            ),
        ];
        if let Some(argument) = command.argument {
            spans.push(Span::styled(format!("  {argument}"), Style::default().fg(theme.dim)));
        }
        let line = TextLine::from(spans);
        lines.push(if is_selected {
            line.style(Style::default().bg(theme.selected_bg))
        } else {
            line
        });
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_composer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let title = if app.running { " message · steers the run " } else { " message " };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.line))
        .title(Span::styled(title, Style::default().fg(theme.dim)))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if app.input.is_empty() {
        Span::styled("Ask Luna to build something…", Style::default().fg(theme.dim))
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
    let (state, state_colour) =
        if app.running { ("running", theme.warning) } else { ("idle", theme.success) };

    let mut spans = vec![
        Span::styled(format!(" {state} "), Style::default().fg(state_colour)),
        Span::styled("│ ", Style::default().fg(theme.line)),
        Span::styled(format!("thinking {}", app.thinking.as_str()), Style::default().fg(theme.dim)),
        Span::styled(" │ ", Style::default().fg(theme.line)),
        Span::styled(
            format!("ctx {:.0}%", app.context_percent),
            Style::default().fg(if app.context_percent > 80.0 { theme.warning } else { theme.dim }),
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
    spans.push(Span::styled(" │ ", Style::default().fg(theme.line)));
    spans.push(Span::styled(app.status.clone(), Style::default().fg(theme.dim)));

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
        .border_style(Style::default().fg(theme.warning))
        .title(Span::styled(" approval needed ", Style::default().fg(theme.warning)))
        .style(Style::default().bg(theme.raise));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        TextLine::from(Span::styled(pending.title.clone(), Style::default().fg(theme.text))),
        TextLine::from(""),
        TextLine::from(Span::styled(
            format!("{}: {}", pending.tool, pending.detail),
            Style::default().fg(theme.muted),
        )),
        TextLine::from(""),
        TextLine::from(vec![
            Span::styled("y", Style::default().fg(theme.success).add_modifier(Modifier::BOLD)),
            Span::styled(" allow   ", Style::default().fg(theme.dim)),
            Span::styled("a", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" always   ", Style::default().fg(theme.dim)),
            Span::styled("n", Style::default().fg(theme.error).add_modifier(Modifier::BOLD)),
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

    #[test]
    fn a_long_word_is_not_dropped() {
        let wrapped = wrap("supercalifragilisticexpialidocious", 10);
        assert_eq!(wrapped.join(""), "supercalifragilisticexpialidocious");
    }
}
