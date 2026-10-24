//! TUI rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as RLine, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, Focus, Item, PendingApproval, ToolItem};
use crate::markdown;

const ACCENT: Color = Color::Rgb(139, 92, 246);
const DIM: Color = Color::Rgb(120, 120, 130);
const OK: Color = Color::Rgb(63, 185, 80);
const ERR: Color = Color::Rgb(248, 81, 73);
const WARN: Color = Color::Rgb(210, 153, 34);

pub fn draw(f: &mut Frame, app: &App) {
    let [body, status] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(1)]).areas(f.area());

    // Agent sidebar only when a multi-agent run is active.
    let chat_area = if app.agents.is_empty() {
        body
    } else {
        let [side, main] =
            Layout::horizontal([Constraint::Length(24), Constraint::Min(30)]).areas(body);
        draw_agents(f, app, side);
        main
    };

    draw_chat(f, app, chat_area);

    if app.session_picker {
        draw_session_picker(f, app);
    }
    if let Some(p) = &app.approval {
        draw_approval(f, p);
    }

    draw_status(f, app, status);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let [transcript, composer] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(composer_height(app))])
            .areas(area);

    // ---- transcript ----
    let width = transcript.width.saturating_sub(2);
    let mut lines: Vec<RLine> = Vec::new();
    for item in &app.items {
        match item {
            Item::User(text) => {
                lines.push(RLine::from(vec![
                    Span::styled("▌ ", Style::default().fg(ACCENT)),
                    Span::styled(
                        "you",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                ]));
                for l in text.lines() {
                    lines.push(RLine::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::raw(l.to_string()),
                    ]));
                }
                lines.push(RLine::from(""));
            }
            Item::Assistant(text) => {
                lines.push(RLine::from(Span::styled(
                    "luna",
                    Style::default().fg(OK).add_modifier(Modifier::BOLD),
                )));
                for l in markdown::render(text, width) {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(l.spans);
                    lines.push(RLine::from(spans));
                }
                lines.push(RLine::from(""));
            }
            Item::Tool(t) => lines.extend(tool_card(t, width)),
            Item::Note(text) => lines.push(RLine::from(Span::styled(
                format!("  ⟡ {text}"),
                Style::default().fg(DIM),
            ))),
            Item::Error(text) => lines.push(RLine::from(Span::styled(
                format!("  ✕ {text}"),
                Style::default().fg(ERR),
            ))),
        }
    }

    let total = lines.len() as u16;
    let view_h = transcript.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(view_h);
    let scroll = max_scroll.saturating_sub(app.scroll.min(max_scroll));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if app.focus == Focus::Transcript {
                    ACCENT
                } else {
                    Color::Rgb(45, 51, 59)
                }))
                .title(Span::styled(
                    " Luna ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(para, transcript);

    if total > view_h {
        let mut sb =
            ScrollbarState::new(max_scroll as usize).position((max_scroll - scroll) as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            transcript,
            &mut sb,
        );
    }

    // ---- composer ----
    draw_composer(f, app, composer);
}

fn composer_height(app: &App) -> u16 {
    let input_lines = app.input.lines().count().max(1) as u16;
    let menu = app.slash_matches().len() as u16;
    let base = input_lines + 2;
    if menu > 0 { base + menu + 1 } else { base }
}

fn draw_composer(f: &mut Frame, app: &App, area: Rect) {
    let menu = app.slash_matches();
    let menu_h = if menu.is_empty() {
        0
    } else {
        menu.len() as u16 + 1
    };
    let [input_area, menu_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(menu_h)]).areas(area);

    let border = if app.focus == Focus::Composer {
        ACCENT
    } else {
        Color::Rgb(45, 51, 59)
    };
    let title = if app.running {
        format!(" {} running… ", spinner(app.tick))
    } else {
        " message ".to_string()
    };
    let input = Paragraph::new(app.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(Span::styled(
                title,
                Style::default().fg(if app.running { OK } else { DIM }),
            )),
    );
    f.render_widget(input, input_area);

    // Cursor.
    let before = &app.input[..app.cursor.min(app.input.len())];
    let row = before.matches('\n').count() as u16;
    let col = before
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count())
        .unwrap_or(0) as u16;
    f.set_cursor_position((
        input_area.x + 1 + col.min(input_area.width.saturating_sub(2)),
        input_area.y + 1 + row.min(input_area.height.saturating_sub(2)),
    ));

    if !menu.is_empty() {
        let mut lines = Vec::new();
        for (i, cmd) in menu.iter().enumerate() {
            let selected = app.slash_index == Some(i);
            lines.push(RLine::from(vec![
                Span::styled(
                    if selected { " ▸ " } else { "   " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!("{:<10}", cmd.name),
                    Style::default()
                        .fg(if selected { ACCENT } else { Color::White })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(cmd.description, Style::default().fg(DIM)),
            ]));
        }
        f.render_widget(Paragraph::new(lines), menu_area);
    }
}

fn tool_card(t: &ToolItem, width: u16) -> Vec<RLine<'static>> {
    let mut out = Vec::new();
    let (icon, color) = if t.running {
        ("◐", WARN)
    } else if t.is_error {
        ("✕", ERR)
    } else {
        ("✓", OK)
    };
    let chev = if t.expanded { "▾" } else { "▸" };
    out.push(RLine::from(vec![
        Span::styled(format!("  {chev} {icon} "), Style::default().fg(color)),
        Span::styled(
            t.name.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", first_line(&t.args)),
            Style::default().fg(DIM),
        ),
    ]));

    if t.expanded {
        for l in t.args.lines().take(20) {
            out.push(RLine::from(vec![
                Span::styled("      ", Style::default()),
                Span::styled(l.to_string(), Style::default().fg(Color::Gray)),
            ]));
        }
        if let Some(o) = &t.output {
            out.push(RLine::from(Span::styled(
                "      ── output ──",
                Style::default().fg(DIM),
            )));
            for l in o.lines().take(30) {
                out.push(RLine::from(vec![
                    Span::styled("      ", Style::default()),
                    Span::styled(
                        truncate(l, width.saturating_sub(8) as usize),
                        Style::default().fg(if t.is_error { ERR } else { Color::Gray }),
                    ),
                ]));
            }
        }
    }
    out
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or("");
    truncate(l, 60)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn draw_agents(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![RLine::from(Span::styled(
        " AGENTS",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))];
    let mut ids: Vec<&String> = app.agents.keys().collect();
    ids.sort();
    for id in ids {
        let st = &app.agents[id];
        let (icon, color) = match st.status.as_str() {
            "done" => ("✓", OK),
            "failed" => ("✕", ERR),
            "spawned" => ("○", DIM),
            s if s.starts_with("tool:") => ("⚙", WARN),
            _ => ("●", ACCENT),
        };
        lines.push(RLine::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(color)),
            Span::styled(id.clone(), Style::default()),
        ]));
        if let Some(p) = &st.parent {
            lines.push(RLine::from(Span::styled(
                format!("   ↳ {p}"),
                Style::default().fg(DIM),
            )));
        }
        lines.push(RLine::from(Span::styled(
            format!("   {} tools", st.tool_calls),
            Style::default().fg(DIM),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" run ")),
        area,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let (dot, dot_color) = if app.running {
        (spinner(app.tick), OK)
    } else {
        ("○", DIM)
    };
    let left = RLine::from(vec![
        Span::styled(format!(" {dot} "), Style::default().fg(dot_color)),
        Span::styled(
            if app.running { "running" } else { "idle" },
            Style::default().fg(dot_color),
        ),
        Span::styled("  ·  ", Style::default().fg(DIM)),
        Span::styled(app.model_label.clone(), Style::default().fg(DIM)),
        Span::styled("  ·  ", Style::default().fg(DIM)),
        Span::styled(format!("~{} tok", app.tokens), Style::default().fg(DIM)),
    ]);
    let right = Span::styled(
        "Enter send · Esc quit · Ctrl+D cancel · PgUp/PgDn scroll · / commands ",
        Style::default().fg(DIM),
    );
    let [l, r] = Layout::horizontal([Constraint::Min(20), Constraint::Length(62)]).areas(area);
    f.render_widget(Paragraph::new(left), l);
    f.render_widget(Paragraph::new(RLine::from(right)), r);
}

fn draw_session_picker(f: &mut Frame, app: &App) {
    let area = centered(70, 60, f.area());
    f.render_widget(Clear, area);
    let mut lines = Vec::new();
    for (i, (_id, preview)) in app.sessions.iter().enumerate() {
        let selected = i == app.session_index;
        lines.push(RLine::from(vec![
            Span::styled(
                if selected { " ▸ " } else { "   " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                truncate(preview, 70),
                Style::default()
                    .fg(if selected { Color::White } else { DIM })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT))
                .title(" sessions — ↑↓ select · Enter open · Esc close "),
        ),
        area,
    );
}

fn draw_approval(f: &mut Frame, p: &PendingApproval) {
    let area = centered(64, 40, f.area());
    f.render_widget(Clear, area);
    let mut lines = vec![
        RLine::from(vec![
            Span::styled(
                "Approval required: ",
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(p.tool_name.clone(), Style::default().fg(WARN)),
        ]),
        RLine::from(""),
    ];
    for l in p.arguments.lines().take(12) {
        lines.push(RLine::from(Span::styled(
            truncate(l, 58),
            Style::default().fg(Color::Gray),
        )));
    }
    lines.push(RLine::from(""));
    lines.push(RLine::from(vec![
        Span::styled("y", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
        Span::raw("/Enter approve   "),
        Span::styled("n", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
        Span::raw("/Esc deny"),
    ]));
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(WARN))
                    .title(" Luna wants to run a tool "),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn spinner(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick / 2) as usize % FRAMES.len()]
}

fn centered(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - py) / 2),
        Constraint::Percentage(py),
        Constraint::Percentage((100 - py) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - px) / 2),
        Constraint::Percentage(px),
        Constraint::Percentage((100 - px) / 2),
    ])
    .split(v[1])[1]
}
