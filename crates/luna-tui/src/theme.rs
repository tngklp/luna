//! The palette.
//!
//! Token names follow pi's theme vocabulary (`accent`, `muted`, `dim`,
//! `toolSuccessBg`, `mdHeading`, …) so the two interfaces describe colour the
//! same way, and so a future theme file can map onto these fields directly.

use ratatui::style::Color;

pub struct Theme {
    /// Window background.
    pub bg: Color,
    /// Raised surfaces: the sidebar, the composer.
    pub panel: Color,
    /// Hover and inset surfaces.
    pub raise: Color,
    /// Dividers and borders.
    pub line: Color,
    /// Normal borders.
    pub border: Color,
    /// Highlighted borders.
    pub border_accent: Color,

    /// Default text.
    pub text: Color,
    /// Secondary text.
    pub muted: Color,
    /// Very dimmed text.
    pub dim: Color,

    /// Primary accent: the logo, the cursor, selected rows.
    pub accent: Color,
    /// Background of a selected row.
    pub selected_bg: Color,

    pub success: Color,
    pub warning: Color,
    pub error: Color,

    /// User message text.
    pub user_text: Color,
    /// User message background.
    pub user_bg: Color,
    /// Thinking block text.
    pub thinking_text: Color,

    /// Tool box backgrounds, by state.
    pub tool_pending_bg: Color,
    pub tool_success_bg: Color,
    pub tool_error_bg: Color,
    /// Tool box title.
    pub tool_title: Color,
    /// Tool box output.
    pub tool_output: Color,

    /// Markdown headings.
    pub md_heading: Color,
    /// Inline code.
    pub md_code: Color,
    /// Code block content.
    pub md_code_block: Color,
    /// Code block fences.
    pub md_code_border: Color,
    /// List bullets.
    pub md_bullet: Color,
    /// Links.
    pub md_link: Color,

    /// Diff lines.
    pub diff_added: Color,
    pub diff_removed: Color,
}

impl Theme {
    /// The default dark palette.
    ///
    /// Values are the sRGB equivalents of pi's `dark` theme, so the two
    /// interfaces read as the same product.
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
}
