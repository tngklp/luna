//! Syntax highlighting for fenced code blocks.
//!
//! `syntect` is loaded once and reused; a fresh `SyntaxSet` per block would cost
//! more than the render itself.

use once_cell::sync::Lazy;
use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme as SynTheme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAXES: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME: Lazy<SynTheme> =
    Lazy::new(|| ThemeSet::load_defaults().themes.remove("base16-ocean.dark").unwrap_or_default());

/// One highlighted line: the text plus the style of each span.
pub type Spans = Vec<(String, Style)>;

/// Highlight a block of code. Unknown languages fall back to plain text.
pub fn highlight(code: &str, language: &str) -> Vec<Spans> {
    let syntax = SYNTAXES
        .find_syntax_by_token(language)
        .or_else(|| SYNTAXES.find_syntax_by_extension(language))
        .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, &SYNTAXES).unwrap_or_default();
        let spans: Spans = ranges
            .into_iter()
            .map(|(style, text)| {
                let colour = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                let mut converted = Style::default().fg(colour);
                if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
                    converted = converted.add_modifier(Modifier::BOLD);
                }
                if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
                    converted = converted.add_modifier(Modifier::ITALIC);
                }
                (text.trim_end_matches('\n').to_string(), converted)
            })
            .collect();
        out.push(spans);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_known_languages() {
        let lines = highlight("fn main() {}\n", "rust");
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].is_empty());
    }

    #[test]
    fn unknown_languages_still_render() {
        let lines = highlight("some text\n", "not-a-language");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].iter().any(|(text, _)| text.contains("some text")));
    }
}
