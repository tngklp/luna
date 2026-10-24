//! Minimal markdown → ratatui lines renderer.
//!
//! Handles the constructs that matter in agent output: headings, bold/italic,
//! inline code, fenced code blocks, bullet/numbered lists, blockquotes, rules,
//! and links. Deliberately not a full CommonMark implementation.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render markdown into styled lines.
pub fn render(markdown: &str, width: u16) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buf: Vec<String> = Vec::new();

    for raw in markdown.lines() {
        let line = raw.trim_end();

        // Fenced code blocks.
        if line.trim_start().starts_with("```") {
            if in_code {
                out.extend(render_code_block(&code_buf, &code_lang, width));
                code_buf.clear();
                code_lang.clear();
                in_code = false;
            } else {
                in_code = true;
                code_lang = line.trim_start().trim_start_matches('`').trim().to_string();
            }
            continue;
        }
        if in_code {
            code_buf.push(line.to_string());
            continue;
        }

        if line.trim().is_empty() {
            out.push(Line::from(""));
            continue;
        }

        // Horizontal rule.
        let t = line.trim();
        if t == "---" || t == "***" || t == "___" {
            out.push(Line::from(Span::styled(
                "─".repeat(width.saturating_sub(2) as usize),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Headings.
        if let Some(rest) = t.strip_prefix("### ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = t.strip_prefix("## ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }

        // Blockquote.
        if let Some(rest) = t.strip_prefix("> ") {
            out.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(rest.to_string(), Style::default().fg(Color::Gray)),
            ]));
            continue;
        }

        // Bullet lists.
        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            let mut spans = vec![Span::styled("  • ", Style::default().fg(Color::Yellow))];
            spans.extend(inline_spans(rest));
            out.push(Line::from(spans));
            continue;
        }

        // Numbered lists.
        if let Some((num, rest)) = split_numbered(t) {
            let mut spans = vec![Span::styled(
                format!("  {num}. "),
                Style::default().fg(Color::Yellow),
            )];
            spans.extend(inline_spans(rest));
            out.push(Line::from(spans));
            continue;
        }

        // Paragraph.
        out.push(Line::from(inline_spans(t)));
    }

    // Unterminated code block.
    if in_code && !code_buf.is_empty() {
        out.extend(render_code_block(&code_buf, &code_lang, width));
    }

    out
}

fn split_numbered(s: &str) -> Option<(String, &str)> {
    let dot = s.find(". ")?;
    let num = &s[..dot];
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((num.to_string(), &s[dot + 2..]))
}

fn render_code_block(lines: &[String], lang: &str, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let label = if lang.is_empty() { "code" } else { lang };
    let bar = "─".repeat(width.saturating_sub(2) as usize);
    out.push(Line::from(vec![
        Span::styled("┌", Style::default().fg(Color::DarkGray)),
        Span::styled(format!(" {label} "), Style::default().fg(Color::DarkGray)),
        Span::styled(bar, Style::default().fg(Color::DarkGray)),
    ]));
    for l in lines {
        out.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            Span::styled(l.clone(), Style::default().fg(Color::Green)),
        ]));
    }
    out.push(Line::from(Span::styled(
        "└".to_string() + &"─".repeat(width.saturating_sub(3) as usize),
        Style::default().fg(Color::DarkGray),
    )));
    out
}

/// Parse inline markdown (bold, italic, code, links) into spans.
fn inline_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Inline code: `...`
        if chars[i] == '`'
            && let Some(end) = find_char(&chars, i + 1, '`')
        {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let code: String = chars[i + 1..end].iter().collect();
            spans.push(Span::styled(
                code,
                Style::default().fg(Color::Green).bg(Color::Black),
            ));
            i = end + 1;
            continue;
        }

        // Bold: **...**
        if chars[i] == '*'
            && i + 1 < chars.len()
            && chars[i + 1] == '*'
            && let Some(end) = find_double(&chars, i + 2, '*')
        {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let bold: String = chars[i + 2..end].iter().collect();
            spans.push(Span::styled(
                bold,
                Style::default().add_modifier(Modifier::BOLD),
            ));
            i = end + 2;
            continue;
        }

        // Italic: *...*
        if chars[i] == '*'
            && let Some(end) = find_char(&chars, i + 1, '*')
        {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let italic: String = chars[i + 1..end].iter().collect();
            spans.push(Span::styled(
                italic,
                Style::default().add_modifier(Modifier::ITALIC),
            ));
            i = end + 1;
            continue;
        }

        // Link: [text](url) -> show text in accent, url dimmed.
        if chars[i] == '['
            && let Some(close) = find_char(&chars, i + 1, ']')
            && close + 1 < chars.len()
            && chars[close + 1] == '('
            && let Some(paren) = find_char(&chars, close + 2, ')')
        {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let label: String = chars[i + 1..close].iter().collect();
            let url: String = chars[close + 2..paren].iter().collect();
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            spans.push(Span::styled(
                format!(" ({url})"),
                Style::default().fg(Color::DarkGray),
            ));
            i = paren + 1;
            continue;
        }

        buf.push(chars[i]);
        i += 1;
    }

    if !buf.is_empty() {
        spans.push(Span::raw(buf));
    }
    if spans.is_empty() {
        spans.push(Span::raw(""));
    }
    spans
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == target)
}

fn find_double(chars: &[char], from: usize, target: char) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == target && chars[i + 1] == target {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_and_bullets() {
        let lines = render("# Title\n\n- one\n- two", 40);
        assert!(lines.len() >= 4);
        assert!(lines[0].spans[0].content.contains("Title"));
    }

    #[test]
    fn renders_code_block_with_language() {
        let lines = render("```rust\nlet x = 1;\n```", 40);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("rust"));
        assert!(text.contains("let x = 1;"));
    }

    #[test]
    fn inline_code_and_bold() {
        let spans = inline_spans("use `cargo` and **bold** text");
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("cargo"));
        assert!(text.contains("bold"));
    }

    #[test]
    fn unterminated_code_block_still_renders() {
        let lines = render("```\nunclosed", 30);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("unclosed"));
    }
}
