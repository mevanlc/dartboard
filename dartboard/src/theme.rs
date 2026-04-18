use dartboard_core::RgbColor;
use ratatui::style::Color;

pub const BORDER: Color = Color::Rgb(96, 64, 32);
pub const ACCENT: Color = Color::Rgb(184, 120, 40);
pub const TEXT: Color = Color::Rgb(136, 128, 120);
pub const MUTED: Color = Color::Rgb(112, 104, 104);
pub const MUTED_GREATER: Color = Color::Rgb(64, 56, 56);
pub const SELECTION_BG: Color = Color::Rgb(64, 40, 24);
pub const HIGHLIGHT: Color = Color::Rgb(208, 166, 89);
pub const OOB_BG: Color = Color::Rgb(16, 16, 16);
pub const FLOAT_BG: Color = Color::Rgb(32, 48, 64);

pub const PLAYER_PALETTE: [RgbColor; 8] = [
    RgbColor::new(255, 110, 64),
    RgbColor::new(255, 196, 64),
    RgbColor::new(145, 226, 88),
    RgbColor::new(72, 220, 170),
    RgbColor::new(84, 196, 255),
    RgbColor::new(128, 163, 255),
    RgbColor::new(192, 132, 255),
    RgbColor::new(255, 124, 196),
];

pub const DEFAULT_GLYPH_FG: RgbColor = RgbColor::new(136, 128, 120);

pub const fn rat(c: RgbColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
