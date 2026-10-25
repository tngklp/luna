//! Themes.
//!
//! A theme is a named palette. The built-ins mirror the GUI's token names, so a
//! colour means the same thing in both interfaces, and a theme file can be
//! dropped into `~/.luna/themes/` without a rebuild.

use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

/// Every colour the interface uses.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub panel: Color,
    pub raise: Color,
    pub line: Color,
    pub border: Color,
    pub border_accent: Color,

    pub text: Color,
    pub muted: Color,
    pub dim: Color,

    pub accent: Color,
    pub selected_bg: Color,

    pub success: Color,
    pub warning: Color,
    pub error: Color,

    pub user_text: Color,
    pub user_bg: Color,
    pub thinking_text: Color,

    pub tool_pending_bg: Color,
    pub tool_success_bg: Color,
    pub tool_error_bg: Color,
    pub tool_title: Color,
    pub tool_output: Color,

    pub md_heading: Color,
    pub md_code: Color,
    pub md_code_block: Color,
    pub md_code_border: Color,
    pub md_bullet: Color,
    pub md_link: Color,

    pub diff_added: Color,
    pub diff_removed: Color,
}

/// A theme as written on disk: `#rrggbb` strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFile {
    pub name: String,
    #[serde(default)]
    pub appearance: Option<String>,
    pub colors: std::collections::BTreeMap<String, String>,
}

impl Theme {
    /// The default dark palette.
    pub fn dark() -> Theme {
        Theme {
            bg: Color::Rgb(0x16, 0x14, 0x1c),
            panel: Color::Rgb(0x1c, 0x1a, 0x23),
            raise: Color::Rgb(0x24, 0x21, 0x2c),
            line: Color::Rgb(0x2e, 0x2b, 0x38),
            border: Color::Rgb(0x6f, 0x7a, 0xd6),
            border_accent: Color::Rgb(0xb1, 0x7a, 0xd4),

            text: Color::Rgb(0xe2, 0xe1, 0xe6),
            muted: Color::Rgb(0xa3, 0xa1, 0xad),
            dim: Color::Rgb(0x84, 0x82, 0x8f),

            accent: Color::Rgb(0xc0, 0x8a, 0xe0),
            selected_bg: Color::Rgb(0x24, 0x2c, 0x4a),

            success: Color::Rgb(0x7d, 0xd6, 0xa8),
            warning: Color::Rgb(0xe0, 0xd0, 0x7a),
            error: Color::Rgb(0xe8, 0x8a, 0x7a),

            user_text: Color::Rgb(0xe2, 0xe1, 0xe6),
            user_bg: Color::Rgb(0x24, 0x2c, 0x4a),
            thinking_text: Color::Rgb(0x9d, 0x9b, 0xa8),

            tool_pending_bg: Color::Rgb(0x24, 0x22, 0x2b),
            tool_success_bg: Color::Rgb(0x1e, 0x2e, 0x28),
            tool_error_bg: Color::Rgb(0x33, 0x22, 0x22),
            tool_title: Color::Rgb(0xe2, 0xe1, 0xe6),
            tool_output: Color::Rgb(0xa3, 0xa1, 0xad),

            md_heading: Color::Rgb(0xe0, 0xd0, 0x7a),
            md_code: Color::Rgb(0xc0, 0x8a, 0xe0),
            md_code_block: Color::Rgb(0x7d, 0xd6, 0xa8),
            md_code_border: Color::Rgb(0x84, 0x82, 0x8f),
            md_bullet: Color::Rgb(0xc0, 0x8a, 0xe0),
            md_link: Color::Rgb(0x8a, 0xa8, 0xe0),

            diff_added: Color::Rgb(0x7d, 0xd6, 0xa8),
            diff_removed: Color::Rgb(0xe8, 0x8a, 0x7a),
        }
    }

    /// A light palette, for terminals on a light background.
    pub fn light() -> Theme {
        Theme {
            bg: Color::Rgb(0xfa, 0xf9, 0xfc),
            panel: Color::Rgb(0xf2, 0xf0, 0xf6),
            raise: Color::Rgb(0xe8, 0xe5, 0xef),
            line: Color::Rgb(0xd6, 0xd2, 0xe0),
            border: Color::Rgb(0x5a, 0x63, 0xc0),
            border_accent: Color::Rgb(0x8a, 0x4f, 0xb8),

            text: Color::Rgb(0x1e, 0x1b, 0x26),
            muted: Color::Rgb(0x55, 0x51, 0x60),
            dim: Color::Rgb(0x7a, 0x76, 0x86),

            accent: Color::Rgb(0x7a, 0x3f, 0xa8),
            selected_bg: Color::Rgb(0xdd, 0xd8, 0xf0),

            success: Color::Rgb(0x1f, 0x7a, 0x4d),
            warning: Color::Rgb(0x8a, 0x6a, 0x00),
            error: Color::Rgb(0xb0, 0x2a, 0x2a),

            user_text: Color::Rgb(0x1e, 0x1b, 0x26),
            user_bg: Color::Rgb(0xdd, 0xd8, 0xf0),
            thinking_text: Color::Rgb(0x6a, 0x66, 0x76),

            tool_pending_bg: Color::Rgb(0xee, 0xec, 0xf3),
            tool_success_bg: Color::Rgb(0xe2, 0xf2, 0xe8),
            tool_error_bg: Color::Rgb(0xf7, 0xe4, 0xe4),
            tool_title: Color::Rgb(0x1e, 0x1b, 0x26),
            tool_output: Color::Rgb(0x55, 0x51, 0x60),

            md_heading: Color::Rgb(0x8a, 0x6a, 0x00),
            md_code: Color::Rgb(0x7a, 0x3f, 0xa8),
            md_code_block: Color::Rgb(0x1f, 0x7a, 0x4d),
            md_code_border: Color::Rgb(0x7a, 0x76, 0x86),
            md_bullet: Color::Rgb(0x7a, 0x3f, 0xa8),
            md_link: Color::Rgb(0x2a, 0x4a, 0xa8),

            diff_added: Color::Rgb(0x1f, 0x7a, 0x4d),
            diff_removed: Color::Rgb(0xb0, 0x2a, 0x2a),
        }
    }

    /// Look a built-in up by name.
    pub fn by_name(name: &str) -> Option<Theme> {
        match name {
            "dark" | "luna-dark" => Some(Theme::dark()),
            "light" | "luna-light" => Some(Theme::light()),
            _ => None,
        }
    }

    /// Overlay a theme file's colours onto this palette.
    ///
    /// Unknown keys are ignored, so a file written for a newer Luna still loads.
    pub fn apply_file(&mut self, file: &ThemeFile) {
        for (key, value) in &file.colors {
            let Some(colour) = parse_colour(value) else { continue };
            match key.as_str() {
                "bg" => self.bg = colour,
                "panel" => self.panel = colour,
                "raise" => self.raise = colour,
                "line" => self.line = colour,
                "border" => self.border = colour,
                "borderAccent" => self.border_accent = colour,
                "text" => self.text = colour,
                "muted" => self.muted = colour,
                "dim" => self.dim = colour,
                "accent" => self.accent = colour,
                "selectedBg" => self.selected_bg = colour,
                "success" => self.success = colour,
                "warning" => self.warning = colour,
                "error" => self.error = colour,
                "userMessageText" => self.user_text = colour,
                "userMessageBg" => self.user_bg = colour,
                "thinkingText" => self.thinking_text = colour,
                "toolPendingBg" => self.tool_pending_bg = colour,
                "toolSuccessBg" => self.tool_success_bg = colour,
                "toolErrorBg" => self.tool_error_bg = colour,
                "toolTitle" => self.tool_title = colour,
                "toolOutput" => self.tool_output = colour,
                "mdHeading" => self.md_heading = colour,
                "mdCode" => self.md_code = colour,
                "mdCodeBlock" => self.md_code_block = colour,
                "mdCodeBlockBorder" => self.md_code_border = colour,
                "mdListBullet" => self.md_bullet = colour,
                "mdLink" => self.md_link = colour,
                "toolDiffAdded" => self.diff_added = colour,
                "toolDiffRemoved" => self.diff_removed = colour,
                _ => {}
            }
        }
    }

    /// Load a theme by name, looking in `dir` for a matching file.
    pub fn load(name: &str, dir: &Path) -> Theme {
        if let Some(theme) = Theme::by_name(name) {
            return theme;
        }
        let path = dir.join(format!("{name}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Theme::dark();
        };
        let Ok(file) = serde_json::from_str::<ThemeFile>(&text) else {
            return Theme::dark();
        };
        let mut theme = if file.appearance.as_deref() == Some("light") {
            Theme::light()
        } else {
            Theme::dark()
        };
        theme.apply_file(&file);
        theme
    }

    /// Every theme available, built-in and on disk.
    pub fn available(dir: &Path) -> Vec<String> {
        let mut names = vec!["dark".to_string(), "light".to_string()];
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

/// Parse `#rrggbb` or `#rgb`.
pub fn parse_colour(value: &str) -> Option<Color> {
    let hex = value.trim().trim_start_matches('#');
    let expand = |c: char| c.to_digit(16).map(|digit| (digit * 17) as u8);
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let r = expand(chars.next()?)?;
            let g = expand(chars.next()?)?;
            let b = expand(chars.next()?)?;
            Some(Color::Rgb(r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_in_both_forms() {
        assert_eq!(parse_colour("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_colour("#f00"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_colour("00ff00"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn a_bad_colour_is_rejected() {
        assert_eq!(parse_colour("#gg0000"), None);
        assert_eq!(parse_colour("#12345"), None);
    }

    #[test]
    fn built_in_themes_are_found_by_name() {
        assert!(Theme::by_name("dark").is_some());
        assert!(Theme::by_name("light").is_some());
        assert!(Theme::by_name("nope").is_none());
    }

    #[test]
    fn a_theme_file_overrides_only_what_it_sets() {
        let mut theme = Theme::dark();
        let original_text = theme.text;
        let file = ThemeFile {
            name: "custom".into(),
            appearance: None,
            colors: std::collections::BTreeMap::from([(
                "accent".to_string(),
                "#ff0000".to_string(),
            )]),
        };
        theme.apply_file(&file);
        assert_eq!(theme.accent, Color::Rgb(255, 0, 0));
        assert_eq!(theme.text, original_text);
    }

    #[test]
    fn an_unknown_theme_name_falls_back_to_dark() {
        let theme = Theme::load("does-not-exist", Path::new("/nonexistent"));
        assert_eq!(theme.bg, Theme::dark().bg);
    }

    #[test]
    fn available_lists_the_built_ins() {
        let names = Theme::available(Path::new("/nonexistent"));
        assert!(names.contains(&"dark".to_string()));
        assert!(names.contains(&"light".to_string()));
    }
}
