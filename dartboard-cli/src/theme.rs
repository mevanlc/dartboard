use dartboard_core::RgbColor;
use ratatui::style::Color;

pub const BORDER: Color = Color::Rgb(96, 64, 32);
pub const ACCENT: Color = Color::Rgb(184, 120, 40);
pub const TEXT: Color = Color::Rgb(136, 128, 120);
pub const MUTED: Color = Color::Rgb(112, 104, 104);
pub const MUTED_GREATER: Color = Color::Rgb(64, 56, 56);
pub const SELECTION_BG: Color = Color::Rgb(64, 40, 24);
pub const HIGHLIGHT: Color = Color::Rgb(208, 166, 89);
pub const ERROR: Color = Color::Rgb(176, 48, 56);
pub const OOB_BG: Color = Color::Rgb(16, 16, 16);
pub const FLOAT_BG: Color = Color::Rgb(32, 48, 64);

pub const PLAYER_PALETTE: [RgbColor; 9] = [
    RgbColor::new(255, 110, 64),
    RgbColor::new(255, 236, 96),
    RgbColor::new(145, 226, 88),
    RgbColor::new(72, 220, 170),
    RgbColor::new(84, 196, 255),
    RgbColor::new(128, 163, 255),
    RgbColor::new(192, 132, 255),
    RgbColor::new(255, 124, 196),
    RgbColor::new(176, 48, 56),
];

/// Per-session drawing colors. Unlike `PLAYER_PALETTE`, these do not identify
/// participants and can be changed without changing the connected user color.
pub const PAINT_PALETTE: [RgbColor; 16] = [
    RgbColor::new(255, 110, 64),
    RgbColor::new(255, 236, 96),
    RgbColor::new(255, 214, 102),
    RgbColor::new(145, 226, 88),
    RgbColor::new(188, 255, 128),
    RgbColor::new(72, 220, 170),
    RgbColor::new(86, 245, 214),
    RgbColor::new(84, 196, 255),
    RgbColor::new(96, 225, 255),
    RgbColor::new(128, 163, 255),
    RgbColor::new(164, 146, 255),
    RgbColor::new(192, 132, 255),
    RgbColor::new(224, 116, 255),
    RgbColor::new(255, 124, 196),
    RgbColor::new(255, 142, 158),
    RgbColor::new(238, 242, 255),
];

pub const PLAYER_COLOR_NAMES: [&str; 9] = [
    "salmon", "amber", "lime", "mint", "sky", "indigo", "violet", "rose", "maroon",
];

pub const DEFAULT_GLYPH_FG: RgbColor = RgbColor::new(136, 128, 120);

pub const fn rat(c: RgbColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
