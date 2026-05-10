use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorMode {
    Ansi16,
    Xterm256,
    #[default]
    TrueColor,
}

impl ColorMode {
    pub const ALL: [Self; 3] = [Self::Ansi16, Self::Xterm256, Self::TrueColor];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ansi16 => "16 color",
            Self::Xterm256 => "256 color",
            Self::TrueColor => "24-bit",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Ansi16 => Self::Xterm256,
            Self::Xterm256 => Self::TrueColor,
            Self::TrueColor => Self::Ansi16,
        }
    }
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ansi16 => "16",
            Self::Xterm256 => "256",
            Self::TrueColor => "truecolor",
        })
    }
}

impl FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "16" | "ansi16" | "ansi-16" | "xterm" => Ok(Self::Ansi16),
            "256" | "xterm256" | "xterm-256" | "xterm-256color" => Ok(Self::Xterm256),
            "24" | "24bit" | "24-bit" | "truecolor" | "true-color" => Ok(Self::TrueColor),
            other => Err(format!(
                "unknown color mode {other:?}; expected 16, 256, or truecolor"
            )),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorViewMode {
    HideUnmapped,
    #[default]
    NearestMapped,
}

impl ColorViewMode {
    pub const ALL: [Self; 2] = [Self::HideUnmapped, Self::NearestMapped];

    pub const fn label(self) -> &'static str {
        match self {
            Self::HideUnmapped => "hide unmapped",
            Self::NearestMapped => "nearest mapped",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::HideUnmapped => Self::NearestMapped,
            Self::NearestMapped => Self::HideUnmapped,
        }
    }
}

impl fmt::Display for ColorViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::HideUnmapped => "hide-unmapped",
            Self::NearestMapped => "nearest-mapped",
        })
    }
}

impl FromStr for ColorViewMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hide" | "hide-unmapped" | "hide_unmapped" => Ok(Self::HideUnmapped),
            "nearest" | "nearest-mapped" | "nearest_mapped" => Ok(Self::NearestMapped),
            other => Err(format!(
                "unknown color view mode {other:?}; expected hide-unmapped or nearest-mapped"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorModeEntry {
    pub ansi16: u8,
    pub xterm256: u8,
    pub rgb: RgbColor,
}

impl ColorModeEntry {
    pub const fn ansi16_rgb(self) -> RgbColor {
        xterm_ansi16_rgb(self.ansi16)
    }
}

pub const XTERM_COLOR_LOOKUP: [ColorModeEntry; 256] = build_xterm_color_lookup();

pub fn constrain_rgb(
    rgb: RgbColor,
    color_mode: ColorMode,
    view_mode: ColorViewMode,
) -> Option<RgbColor> {
    if view_mode == ColorViewMode::HideUnmapped && !rgb_is_mapped(rgb, color_mode) {
        return None;
    }

    Some(match color_mode {
        ColorMode::Ansi16 => counterparts_from_rgb(rgb).ansi16_rgb(),
        ColorMode::Xterm256 => counterparts_from_rgb(rgb).rgb,
        ColorMode::TrueColor => rgb,
    })
}

pub fn rgb_is_mapped(rgb: RgbColor, color_mode: ColorMode) -> bool {
    match color_mode {
        ColorMode::Ansi16 => (0..16).any(|index| xterm_ansi16_rgb(index) == rgb),
        ColorMode::Xterm256 => XTERM_COLOR_LOOKUP.iter().any(|entry| entry.rgb == rgb),
        ColorMode::TrueColor => true,
    }
}

pub fn counterparts_from_rgb(rgb: RgbColor) -> ColorModeEntry {
    let mut best = XTERM_COLOR_LOOKUP[0];
    let mut best_distance = color_distance(rgb, best.rgb);
    let mut index = 1;

    while index < XTERM_COLOR_LOOKUP.len() {
        let candidate = XTERM_COLOR_LOOKUP[index];
        let distance = color_distance(rgb, candidate.rgb);
        if distance < best_distance {
            best = candidate;
            best_distance = distance;
        }
        index += 1;
    }

    best
}

pub fn counterparts_from_xterm256(index: u8) -> ColorModeEntry {
    XTERM_COLOR_LOOKUP[index as usize]
}

pub fn counterparts_from_ansi16(index: u8) -> ColorModeEntry {
    XTERM_COLOR_LOOKUP[(index & 0x0f) as usize]
}

const fn build_xterm_color_lookup() -> [ColorModeEntry; 256] {
    let mut lookup = [ColorModeEntry {
        ansi16: 0,
        xterm256: 0,
        rgb: RgbColor::new(0, 0, 0),
    }; 256];
    let mut index = 0;

    while index < 256 {
        let rgb = xterm256_rgb(index as u8);
        lookup[index] = ColorModeEntry {
            ansi16: nearest_ansi16(rgb),
            xterm256: index as u8,
            rgb,
        };
        index += 1;
    }

    lookup
}

const fn nearest_ansi16(rgb: RgbColor) -> u8 {
    let mut best = 0;
    let mut best_distance = color_distance(rgb, xterm_ansi16_rgb(0));
    let mut index = 1;

    while index < 16 {
        let distance = color_distance(rgb, xterm_ansi16_rgb(index));
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
        index += 1;
    }

    best
}

const fn color_distance(a: RgbColor, b: RgbColor) -> u32 {
    let dr = a.r as i32 - b.r as i32;
    let dg = a.g as i32 - b.g as i32;
    let db = a.b as i32 - b.b as i32;
    (dr * dr + dg * dg + db * db) as u32
}

const fn xterm256_rgb(index: u8) -> RgbColor {
    match index {
        0..=15 => xterm_ansi16_rgb(index),
        16..=231 => {
            let cube_index = index - 16;
            let r = xterm_color_cube_level(cube_index / 36);
            let g = xterm_color_cube_level((cube_index % 36) / 6);
            let b = xterm_color_cube_level(cube_index % 6);
            RgbColor::new(r, g, b)
        }
        _ => {
            let level = 8 + (index - 232) * 10;
            RgbColor::new(level, level, level)
        }
    }
}

const fn xterm_color_cube_level(level: u8) -> u8 {
    match level {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    }
}

const fn xterm_ansi16_rgb(index: u8) -> RgbColor {
    match index & 0x0f {
        0 => RgbColor::new(0, 0, 0),
        1 => RgbColor::new(205, 0, 0),
        2 => RgbColor::new(0, 205, 0),
        3 => RgbColor::new(205, 205, 0),
        4 => RgbColor::new(0, 0, 238),
        5 => RgbColor::new(205, 0, 205),
        6 => RgbColor::new(0, 205, 205),
        7 => RgbColor::new(229, 229, 229),
        8 => RgbColor::new(127, 127, 127),
        9 => RgbColor::new(255, 0, 0),
        10 => RgbColor::new(0, 255, 0),
        11 => RgbColor::new(255, 255, 0),
        12 => RgbColor::new(92, 92, 255),
        13 => RgbColor::new(255, 0, 255),
        14 => RgbColor::new(0, 255, 255),
        _ => RgbColor::new(255, 255, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_mode_parse_accepts_terminal_names() {
        assert_eq!("xterm".parse::<ColorMode>(), Ok(ColorMode::Ansi16));
        assert_eq!(
            "xterm-256color".parse::<ColorMode>(),
            Ok(ColorMode::Xterm256)
        );
        assert_eq!("truecolor".parse::<ColorMode>(), Ok(ColorMode::TrueColor));
    }

    #[test]
    fn color_view_mode_parse_accepts_policy_names() {
        assert_eq!(
            "hide-unmapped".parse::<ColorViewMode>(),
            Ok(ColorViewMode::HideUnmapped)
        );
        assert_eq!(
            "nearest-mapped".parse::<ColorViewMode>(),
            Ok(ColorViewMode::NearestMapped)
        );
    }

    #[test]
    fn xterm_lookup_contains_standard_counterparts() {
        let red = counterparts_from_xterm256(9);
        assert_eq!(red.ansi16, 9);
        assert_eq!(red.rgb, RgbColor::new(255, 0, 0));

        let orange = counterparts_from_xterm256(208);
        assert_eq!(orange.rgb, RgbColor::new(255, 135, 0));
        assert_eq!(orange.xterm256, 208);
    }

    #[test]
    fn rgb_counterparts_are_stable() {
        let entry = counterparts_from_rgb(RgbColor::new(255, 110, 64));
        assert_eq!(entry.xterm256, 203);
        assert_eq!(entry.ansi16, 9);
    }

    #[test]
    fn hide_unmapped_suppresses_inexact_colors() {
        let salmon = RgbColor::new(255, 110, 64);
        assert_eq!(
            constrain_rgb(salmon, ColorMode::Xterm256, ColorViewMode::HideUnmapped),
            None
        );
        assert_eq!(
            constrain_rgb(salmon, ColorMode::Xterm256, ColorViewMode::NearestMapped),
            Some(RgbColor::new(255, 95, 95))
        );
    }
}
