//! The palette, mirroring the GUI's theme tokens so both front-ends look like
//! the same product.

use ratatui::style::Color;

pub struct Theme {
    pub bg: Color,
    pub panel: Color,
    pub raise: Color,
    pub line: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub bad: Color,
    pub user: Color,
}

impl Theme {
    /// The default dark palette, matching the GUI's `luna` theme.
    pub fn dark() -> Theme {
        Theme {
            bg: Color::Rgb(0x11, 0x12, 0x16),
            panel: Color::Rgb(0x17, 0x19, 0x1f),
            raise: Color::Rgb(0x1e, 0x21, 0x28),
            line: Color::Rgb(0x2a, 0x2e, 0x37),
            text: Color::Rgb(0xe4, 0xe6, 0xeb),
            dim: Color::Rgb(0x9a, 0xa0, 0xad),
            faint: Color::Rgb(0x6b, 0x72, 0x80),
            accent: Color::Rgb(0x8b, 0x9d, 0xff),
            ok: Color::Rgb(0x6f, 0xd0, 0x9a),
            warn: Color::Rgb(0xe8, 0xc0, 0x6a),
            bad: Color::Rgb(0xe8, 0x7a, 0x7a),
            user: Color::Rgb(0x9d, 0xb4, 0xff),
        }
    }
}
