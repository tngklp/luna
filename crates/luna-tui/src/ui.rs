//! TUI rendering.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as RLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use crate::app::{App, Line};
pub fn draw(f: &mut Frame, app: &App) {
    // When agents exist, show the dashboard: agent sidebar + chat.
    if app.agents.is_empty() {
        draw_main(f, app, f.area());
    } else {
        let [sidebar, main] = Layout::horizontal([Constraint::Length(26), Constraint::Min(20)]).areas(f.area());
        draw_agent_sidebar(f, app, sidebar);
        draw_main(f, app, main);
    }
}

fn draw_main(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let [chat_area, input_area, status_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(area);

    // Conversation pane.
    let lines: Vec<RLine> = app
        .lines
        .iter()
        .map(|l| match l {
            Line::User(text) => RLine::from(vec![
                Span::styled("you  ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)),
                Span::raw(text),
            ]),
            Line::Assistant(text) => RLine::from(vec![
                Span::styled("luna ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(text),
            ]),
            Line::Tool { name, detail, is_error } => RLine::from(vec![
                Span::styled(
                    "tool ",
                    Style::default().fg(if *is_error { Color::Red } else { Color::Green }),
                ),
                Span::styled(name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(detail.clone(), Style::default().fg(Color::DarkGray)),
            ]),
            Line::Transition(text) => RLine::from(Span::styled(
                format!("  ⟡ {text}"),
                Style::default().fg(Color::Yellow),
            )),
            Line::Error(text) => RLine::from(Span::styled(
                format!("error: {text}"),
                Style::default().fg(Color::Red),
            )),
            Line::Status(text) => RLine::from(Span::styled(
                text.clone(),
                Style::default().fg(Color::DarkGray),
            )),
        })
        .collect();

    let chat = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Luna "))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(chat, chat_area);

    // Scrollbar on the chat pane.
    let total_lines = app.lines.len() as u16;
    if total_lines > 0 {
        let mut sb = ScrollbarState::new(total_lines as usize).position(app.scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chat_area,
            &mut sb,
        );
    }

    // Input pane.
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(" message (Enter to send) "));
    f.render_widget(input, input_area);
    // Cursor at end of input.
    let x = (app.input.chars().count() as u16 + 1).min(input_area.width.saturating_sub(2));
    f.set_cursor_position((input_area.x + 1 + x, input_area.y + 1));

    // Status bar.
    let status_style = if app.running {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let status = Paragraph::new(RLine::from(vec![
        Span::styled(if app.running { "● running" } else { "○ idle" }, status_style),
        Span::raw("  "),
        Span::styled("Ctrl+C quit · Ctrl+D cancel · Tab agents", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(status, status_area);

    // Approval modal.
    if let Some(p) = &app.approval {
        draw_approval_modal(f, p);
    }
}

fn draw_agent_sidebar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut lines: Vec<RLine> = vec![RLine::from(Span::styled(
        " AGENTS",
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    ))];
    let mut ids: Vec<&String> = app.agents.keys().collect();
    ids.sort();
    for id in ids {
        let st = &app.agents[id];
        let (icon, color) = match st.status.as_str() {
            "done" => ("✓", Color::Green),
            "failed" => ("✗", Color::Red),
            "spawned" => ("○", Color::DarkGray),
            s if s.starts_with("tool:") => ("⚙", Color::Yellow),
            "running" | "" => ("●", Color::Cyan),
            _ => ("●", Color::Cyan),
        };
        let focused = app.focus_agent.as_deref() == Some(id.as_str());
        let prefix = if focused { ">" } else { " " };
        lines.push(RLine::from(vec![
            Span::styled(format!("{prefix}{icon} "), Style::default().fg(color)),
            Span::styled(
                id.clone(),
                if focused {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ]));
        if let Some(p) = &st.parent {
            lines.push(RLine::from(Span::styled(
                format!("    ↳ {p}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(RLine::from(Span::styled(
            format!("    {} tools", st.tool_calls),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(RLine::from(""));
    lines.push(RLine::from(Span::styled(
        "Tab: cycle focus",
        Style::default().fg(Color::DarkGray),
    )));

    let sidebar = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" run "));
    f.render_widget(sidebar, area);
}

fn draw_approval_modal(f: &mut Frame, p: &crate::app::PendingApproval) {
    let area = centered_rect(60, 40, f.area());
    f.render_widget(Clear, area);
    let text = vec![
        RLine::from(vec![
            Span::styled(
                "Approval required: ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(p.tool_name.clone(), Style::default().fg(Color::Yellow)),
        ]),
        RLine::from(""),
        RLine::from(Span::styled(
            p.arguments.clone(),
            Style::default().fg(Color::Gray),
        )),
        RLine::from(""),
        RLine::from(vec![
            Span::styled("y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("/Enter approve   "),
            Span::styled("n", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("/Esc deny"),
        ]),
    ];
    let modal = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Luna wants to run a tool "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(modal, area);
}

/// Rect centered in `r` with the given percentage size.
fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup[1])[1]
}
