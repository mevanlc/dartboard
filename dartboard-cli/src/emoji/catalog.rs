use dartboard_picker_core::{sources, SectionSpec};
pub use dartboard_picker_core::{IconCatalogData, IconEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconPickerTab {
    Emoji,
    Unicode,
    NerdFont,
}

impl IconPickerTab {
    pub fn index(self) -> usize {
        match self {
            Self::Emoji => 0,
            Self::Unicode => 1,
            Self::NerdFont => 2,
        }
    }
}

const COMMON_EMOJI: &[&str] = &[
    "👍",
    "👎",
    "🙏",
    "🙌",
    "🙋",
    "🐐",
    "😂",
    "🫡",
    "👀",
    "💀",
    "🎉",
    "🤝",
    "❤\u{fe0f}",
    "✅",
    "🔥",
    "⚡",
    "🚀",
    "🤔",
    "🫠",
    "🌱",
    "🤖",
    "🔧",
    "💎",
    "⭐",
    "🎯",
];

const COMMON_NERD_NAMES: &[&str] = &[
    "cod hubot",
    "md folder",
    "md git",
    "oct zap",
    "md chart bar",
    "cod credit card",
    "md timer",
    "md target",
    "md rocket launch",
    "seti code",
];

const COMMON_UNICODE: &[(&str, &str)] = &[
    ("●", "Black Circle"),
    ("◆", "Black Diamond"),
    ("★", "Black Star"),
    ("→", "Rightwards Arrow"),
    ("│", "Box Drawings Light Vertical"),
    ("■", "Black Square"),
    ("▲", "Black Up-Pointing Triangle"),
    ("○", "White Circle"),
    ("✦", "Black Four Pointed Star"),
    ("⟩", "Mathematical Right Angle Bracket"),
    ("·", "Middle Dot"),
    ("»", "Right-Pointing Double Angle Quotation Mark"),
    ("✓", "Check Mark"),
    ("✗", "Ballot X"),
];

/// Build the dartboard-cli icon catalog: three tabs (Emoji, Unicode, NerdFont)
/// with a "common" curated section and a full section in each. Tab order must
/// match [`IconPickerTab::index`].
pub fn load_catalog() -> IconCatalogData {
    let emoji_tab = vec![
        SectionSpec::new("common emoji", sources::emoji_pick(COMMON_EMOJI)),
        SectionSpec::new("all emoji", sources::emoji_all()),
    ];

    let unicode_tab = vec![
        SectionSpec::new("box drawing", sources::unicode_range(0x2500..=0x259F)),
        SectionSpec::new("common", sources::unicode_pick(COMMON_UNICODE)),
        SectionSpec::new("all unicode", sources::unicode_all()),
    ];

    let nerd_tab = vec![
        SectionSpec::new("common", sources::nerd_pick(COMMON_NERD_NAMES)),
        SectionSpec::new("all nerd font", sources::nerd_all()),
    ];

    IconCatalogData::new(vec![emoji_tab, unicode_tab, nerd_tab])
}
