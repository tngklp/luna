//! Rendering helpers that are easier to test outside the widget tree:
//! unified diffs, a usage sparkline, and the composer's line editing.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// One line of a rendered diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Context,
    Header,
}

/// Parse a unified diff into coloured lines.
///
/// Only the parts a reader needs are kept: the file header, hunk headers, and
/// the changed lines with a little context.
pub fn parse_diff(text: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    for line in text.lines() {
        let kind = if line.starts_with("+++")
            || line.starts_with("---")
            || line.starts_with("@@")
        {
            DiffKind::Header
        } else if line.starts_with('+') {
            DiffKind::Added
        } else if line.starts_with('-') {
            DiffKind::Removed
        } else {
            DiffKind::Context
        };
        out.push(DiffLine { kind, text: line.to_string() });
    }
    out
}

/// Render a diff as styled lines.
pub fn render_diff(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    parse_diff(text)
        .into_iter()
        .map(|line| {
            let (colour, prefix) = match line.kind {
                DiffKind::Added => (theme.diff_added, ""),
                DiffKind::Removed => (theme.diff_removed, ""),
                DiffKind::Header => (theme.md_code_border, ""),
                DiffKind::Context => (theme.tool_output, ""),
            };
            let text = truncate(&line.text, width);
            Line::from(Span::styled(format!("{prefix}{text}"), Style::default().fg(colour)))
        })
        .collect()
}

/// Cut a string to a display width, marking the cut.
fn truncate(text: &str, width: usize) -> String {
    if width == 0 || text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The characters a sparkline is drawn with, low to high.
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render a series of values as a sparkline.
///
/// An empty series renders as nothing rather than a row of blanks, so the status
/// bar does not reserve space for a graph that has no data.
pub fn sparkline(values: &[u64]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    values
        .iter()
        .map(|value| {
            let index = ((*value as f64 / max as f64) * (BARS.len() - 1) as f64).round() as usize;
            BARS[index.min(BARS.len() - 1)]
        })
        .collect()
}

/// A sparkline with its own colour.
pub fn sparkline_spans(values: &[u64], colour: Color) -> Vec<Span<'static>> {
    vec![Span::styled(sparkline(values), Style::default().fg(colour))]
}

/// The composer's text buffer, with cursor movement that respects characters.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    text: String,
    /// Byte offset of the cursor.
    cursor: usize,
}

impl Editor {
    pub fn new() -> Self {
        Editor::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Take the contents, leaving the editor empty.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn insert_str(&mut self, value: &str) {
        self.text.insert_str(self.cursor, value);
        self.cursor += value.len();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.prev_boundary(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Delete the character after the cursor.
    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let end = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..end, "");
    }

    /// Delete the word before the cursor.
    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut start = self.cursor;
        // Skip trailing whitespace, then the word itself.
        while start > 0 {
            let previous = self.prev_boundary(start);
            let ch = self.text[previous..start].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            start = previous;
        }
        while start > 0 {
            let previous = self.prev_boundary(start);
            let ch = self.text[previous..start].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            start = previous;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Delete from the cursor to the start of the line.
    pub fn delete_to_line_start(&mut self) {
        let start = self.text[..self.cursor].rfind('\n').map(|index| index + 1).unwrap_or(0);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
    }

    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map(|index| index + 1).unwrap_or(0);
    }

    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map(|index| self.cursor + index)
            .unwrap_or(self.text.len());
    }

    /// Move up one line, keeping the column where possible.
    pub fn up(&mut self) {
        let column = self.column();
        let Some(line_start) = self.text[..self.cursor].rfind('\n') else { return };
        let previous_start =
            self.text[..line_start].rfind('\n').map(|index| index + 1).unwrap_or(0);
        let previous_len = line_start - previous_start;
        self.cursor = previous_start + column.min(previous_len);
        // Do not land inside a multi-byte character.
        while self.cursor > previous_start && !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    /// Move down one line, keeping the column where possible.
    pub fn down(&mut self) {
        let column = self.column();
        let Some(line_end) = self.text[self.cursor..].find('\n').map(|i| self.cursor + i) else {
            return;
        };
        let next_start = line_end + 1;
        let next_len = self.text[next_start..]
            .find('\n')
            .unwrap_or(self.text.len() - next_start);
        self.cursor = next_start + column.min(next_len);
        while self.cursor > next_start && !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    /// The cursor's column, in characters.
    pub fn column(&self) -> usize {
        let line_start = self.text[..self.cursor].rfind('\n').map(|index| index + 1).unwrap_or(0);
        self.text[line_start..self.cursor].chars().count()
    }

    /// The cursor's row, zero-based.
    pub fn row(&self) -> usize {
        self.text[..self.cursor].matches('\n').count()
    }

    fn prev_boundary(&self, from: usize) -> usize {
        if from == 0 {
            return 0;
        }
        let mut index = from - 1;
        while index > 0 && !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }

    fn next_boundary(&self, from: usize) -> usize {
        if from >= self.text.len() {
            return self.text.len();
        }
        let mut index = from + 1;
        while index < self.text.len() && !self.text.is_char_boundary(index) {
            index += 1;
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_is_classified_by_line() {
        let diff = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n same";
        let lines = parse_diff(diff);
        assert_eq!(lines[0].kind, DiffKind::Header);
        assert_eq!(lines[3].kind, DiffKind::Removed);
        assert_eq!(lines[4].kind, DiffKind::Added);
        assert_eq!(lines[5].kind, DiffKind::Context);
    }

    #[test]
    fn a_diff_renders_one_line_per_input_line() {
        let theme = Theme::dark();
        let lines = render_diff("+a\n-b\n c", &theme, 40);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn a_sparkline_scales_to_its_maximum() {
        // 0 maps to the lowest bar, the maximum to the highest, and the middle
        // value rounds to the middle bar.
        assert_eq!(sparkline(&[0, 8]), "▁█");
        assert_eq!(sparkline(&[0, 4, 8]), "▁▅█");
    }

    #[test]
    fn an_empty_sparkline_is_empty() {
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn a_flat_series_does_not_divide_by_zero() {
        assert_eq!(sparkline(&[0, 0]), "▁▁");
    }

    #[test]
    fn the_editor_inserts_and_deletes() {
        let mut editor = Editor::new();
        editor.insert_str("hello");
        assert_eq!(editor.text(), "hello");
        editor.backspace();
        assert_eq!(editor.text(), "hell");
        editor.left();
        editor.delete();
        assert_eq!(editor.text(), "hel");
    }

    #[test]
    fn the_editor_handles_multibyte_characters() {
        let mut editor = Editor::new();
        editor.insert_str("héllo");
        editor.backspace();
        assert_eq!(editor.text(), "héll");
        editor.left();
        editor.left();
        assert_eq!(editor.column(), 2);
    }

    #[test]
    fn deleting_a_word_stops_at_whitespace() {
        let mut editor = Editor::new();
        editor.insert_str("fix the parser");
        editor.delete_word();
        assert_eq!(editor.text(), "fix the ");
    }

    #[test]
    fn deleting_to_line_start_keeps_earlier_lines() {
        let mut editor = Editor::new();
        editor.insert_str("one\ntwo three");
        editor.delete_to_line_start();
        assert_eq!(editor.text(), "one\n");
    }

    #[test]
    fn home_and_end_stay_within_the_line() {
        let mut editor = Editor::new();
        editor.insert_str("one\ntwo");
        editor.home();
        assert_eq!(editor.column(), 0);
        assert_eq!(editor.row(), 1);
        editor.end();
        assert_eq!(editor.column(), 3);
    }

    #[test]
    fn moving_up_keeps_the_column() {
        let mut editor = Editor::new();
        editor.insert_str("abcdef\nxy");
        editor.up();
        assert_eq!(editor.row(), 0);
        assert_eq!(editor.column(), 2);
    }

    #[test]
    fn moving_down_keeps_the_column() {
        let mut editor = Editor::new();
        editor.insert_str("abcdef\nxy");
        editor.home();
        editor.up();
        editor.down();
        assert_eq!(editor.row(), 1);
    }

    #[test]
    fn taking_the_text_empties_the_editor() {
        let mut editor = Editor::new();
        editor.insert_str("hello");
        assert_eq!(editor.take(), "hello");
        assert!(editor.is_empty());
        assert_eq!(editor.cursor(), 0);
    }
}
