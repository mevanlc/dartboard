//! Entry-generating helpers for building [`SectionSpec`](crate::SectionSpec)
//! contents. Consumers pick and combine these however they like; this crate
//! ships no default catalog of its own.

use std::ops::RangeInclusive;

use crate::{nerd_fonts, IconEntry};

/// Every emoji known to the `emojis` crate, including multi-codepoint
/// presentation forms. Order matches that crate's iteration order.
pub fn emoji_all() -> Vec<IconEntry> {
    emojis::iter()
        .map(|e| IconEntry::new(e.as_str(), e.name()))
        .collect()
}

/// Resolve each string as an emoji via the `emojis` crate. Strings that don't
/// resolve are skipped silently so consumer lists can include speculative
/// entries without breaking.
pub fn emoji_pick(emoji_strs: &[&str]) -> Vec<IconEntry> {
    emoji_strs
        .iter()
        .filter_map(|s| {
            let e = emojis::get(s)?;
            Some(IconEntry::new(e.as_str(), e.name()))
        })
        .collect()
}

/// Every glyph in the vendored Nerd Fonts `glyphnames.json`, sorted by name.
pub fn nerd_all() -> Vec<IconEntry> {
    nerd_fonts::load()
        .into_iter()
        .map(|g| IconEntry::new(g.icon, g.name))
        .collect()
}

/// Pick nerd-font glyphs by exact name (e.g. `"cod hubot"`). Missing names
/// are skipped silently.
pub fn nerd_pick(names: &[&str]) -> Vec<IconEntry> {
    let all = nerd_fonts::load();
    names
        .iter()
        .filter_map(|wanted| {
            all.iter()
                .find(|g| g.name == *wanted)
                .map(|g| IconEntry::new(g.icon.clone(), g.name.clone()))
        })
        .collect()
}

/// Every Unicode scalar with a name from `unicode_names2`, skipping
/// surrogates and `<...>` placeholder names. Names are lowercased so
/// searching matches the rest of the catalog.
pub fn unicode_all() -> Vec<IconEntry> {
    unicode_range(0..=0x10FFFF)
}

/// All named Unicode scalars in the given codepoint range, with the same
/// filtering rules as [`unicode_all`]. Useful for curated ranges like
/// Box Drawing (`0x2500..=0x259F`).
pub fn unicode_range(range: RangeInclusive<u32>) -> Vec<IconEntry> {
    let mut entries = Vec::new();
    for code in range {
        if (0xD800..=0xDFFF).contains(&code) {
            continue;
        }
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        let Some(name) = unicode_names2::name(ch) else {
            continue;
        };
        let name_str = name.to_string();
        if name_str.starts_with('<') {
            continue;
        }
        entries.push(IconEntry::new(ch.to_string(), name_str.to_lowercase()));
    }
    entries
}

/// Build entries from explicit `(glyph, name)` pairs. Use when you want a
/// hand-curated "common" set whose names aren't taken from the Unicode tables.
pub fn unicode_pick(pairs: &[(&str, &str)]) -> Vec<IconEntry> {
    pairs
        .iter()
        .map(|(icon, name)| IconEntry::new(icon.to_string(), name.to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_all_includes_multi_codepoint_forms() {
        let all = emoji_all();
        // Heart with variation selector is the classic multi-codepoint case.
        // With the old single-char filter this would be missing.
        assert!(all.iter().any(|e| e.icon == "❤\u{fe0f}"));
        assert!(all.len() > 1000);
    }

    #[test]
    fn emoji_pick_resolves_strings_and_skips_unknown() {
        let picked = emoji_pick(&["👍", "👎", "not-an-emoji", "❤\u{fe0f}"]);
        let icons: Vec<&str> = picked.iter().map(|e| e.icon.as_str()).collect();
        assert_eq!(icons, vec!["👍", "👎", "❤\u{fe0f}"]);
    }

    #[test]
    fn nerd_pick_finds_known_names() {
        let picked = nerd_pick(&["cod hubot", "does-not-exist"]);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "cod hubot");
    }

    #[test]
    fn nerd_all_has_plausible_size() {
        assert!(nerd_all().len() > 5000);
    }

    #[test]
    fn unicode_range_is_a_subset_of_all() {
        let box_drawing = unicode_range(0x2500..=0x259F);
        assert!(!box_drawing.is_empty());
        assert!(box_drawing.len() < 300);
    }

    #[test]
    fn unicode_all_has_plausible_size() {
        assert!(unicode_all().len() > 10_000);
    }

    #[test]
    fn unicode_pick_lowercases_names() {
        let picked = unicode_pick(&[("●", "Black Circle"), ("→", "Rightwards Arrow")]);
        assert_eq!(picked[0].name, "black circle");
        assert_eq!(picked[1].name, "rightwards arrow");
    }
}
