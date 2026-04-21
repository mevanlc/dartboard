mod nerd_fonts;
pub mod sources;

#[derive(Clone)]
pub struct IconEntry {
    pub icon: String,
    pub name: String,
    pub name_lower: String,
}

impl IconEntry {
    pub fn new(icon: impl Into<String>, name: impl Into<String>) -> Self {
        let icon = icon.into();
        let name = name.into();
        let name_lower = name.to_lowercase();
        Self {
            icon,
            name,
            name_lower,
        }
    }

    pub fn single_char(&self) -> Option<char> {
        let mut chars = self.icon.chars();
        let ch = chars.next()?;
        if chars.next().is_none() {
            Some(ch)
        } else {
            None
        }
    }
}

/// A consumer-defined section: a title and the entries that belong in it.
/// The catalog owns these; `SectionView` borrows from them.
pub struct SectionSpec {
    pub title: String,
    pub entries: Vec<IconEntry>,
}

impl SectionSpec {
    pub fn new(title: impl Into<String>, entries: Vec<IconEntry>) -> Self {
        Self {
            title: title.into(),
            entries,
        }
    }
}

/// A view over a section's entries: either a borrowed slice (unfiltered) or a
/// Vec of references (filtered by a search query). Either way no entry data is
/// cloned.
pub enum SectionEntries<'a> {
    Full(&'a [IconEntry]),
    Filtered(Vec<&'a IconEntry>),
}

impl<'a> SectionEntries<'a> {
    pub fn len(&self) -> usize {
        match self {
            Self::Full(s) => s.len(),
            Self::Filtered(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, i: usize) -> Option<&'a IconEntry> {
        match self {
            Self::Full(s) => s.get(i),
            Self::Filtered(v) => v.get(i).copied(),
        }
    }
}

pub struct SectionView<'a> {
    pub title: &'a str,
    pub entries: SectionEntries<'a>,
}

/// A catalog of tabs, where each tab owns an ordered list of sections. Tabs
/// are addressed by index; the consumer owns whatever enum or label scheme
/// they like and maps it to an index at call time.
pub struct IconCatalogData {
    tabs: Vec<Vec<SectionSpec>>,
}

impl IconCatalogData {
    pub fn new(tabs: Vec<Vec<SectionSpec>>) -> Self {
        Self { tabs }
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Return section views for the given tab, filtered by `query`. Empty
    /// sections are dropped so lone headers don't appear. Out-of-range
    /// `tab_idx` yields an empty Vec.
    pub fn sections(&self, tab_idx: usize, query: &str) -> Vec<SectionView<'_>> {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return Vec::new();
        };
        let query_lower = query.to_lowercase();
        let mut out = Vec::new();
        for spec in tab {
            if query_lower.is_empty() {
                if spec.entries.is_empty() {
                    continue;
                }
                out.push(SectionView {
                    title: &spec.title,
                    entries: SectionEntries::Full(&spec.entries),
                });
            } else {
                let filtered: Vec<&IconEntry> = spec
                    .entries
                    .iter()
                    .filter(|e| e.name_lower.contains(&query_lower))
                    .collect();
                if filtered.is_empty() {
                    continue;
                }
                out.push(SectionView {
                    title: &spec.title,
                    entries: SectionEntries::Filtered(filtered),
                });
            }
        }
        out
    }
}

pub fn selectable_count(sections: &[SectionView<'_>]) -> usize {
    sections.iter().map(|s| s.entries.len()).sum()
}

pub fn flat_len(sections: &[SectionView<'_>]) -> usize {
    sections.iter().map(|s| s.entries.len() + 1).sum()
}

pub fn selectable_to_flat(sections: &[SectionView<'_>], sel: usize) -> Option<usize> {
    let mut flat = 0;
    let mut remaining = sel;
    for s in sections {
        flat += 1;
        let len = s.entries.len();
        if remaining < len {
            return Some(flat + remaining);
        }
        remaining -= len;
        flat += len;
    }
    None
}

pub fn flat_to_selectable(sections: &[SectionView<'_>], flat_idx: usize) -> Option<usize> {
    let mut flat = 0;
    let mut selectable = 0;
    for s in sections {
        if flat_idx == flat {
            return None;
        }
        flat += 1;
        let len = s.entries.len();
        if flat_idx < flat + len {
            return Some(selectable + (flat_idx - flat));
        }
        flat += len;
        selectable += len;
    }
    None
}

pub fn entry_at_selectable<'a>(
    sections: &'a [SectionView<'a>],
    sel: usize,
) -> Option<&'a IconEntry> {
    let mut remaining = sel;
    for s in sections {
        let len = s.entries.len();
        if remaining < len {
            return s.entries.get(remaining);
        }
        remaining -= len;
    }
    None
}

pub fn adjust_scroll_offset(current: usize, visible: usize, flat_idx: usize) -> usize {
    let visible = visible.max(1);
    if flat_idx < current {
        flat_idx
    } else if flat_idx >= current + visible {
        flat_idx.saturating_sub(visible - 1)
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> IconEntry {
        IconEntry::new("x", name)
    }

    fn fixture_catalog() -> IconCatalogData {
        IconCatalogData::new(vec![
            vec![
                SectionSpec::new("a", vec![entry("a0"), entry("a1")]),
                SectionSpec::new("b", vec![entry("b0 arrow"), entry("b1 arrow"), entry("b2")]),
            ],
            vec![SectionSpec::new("empty", vec![])],
        ])
    }

    #[test]
    fn empty_query_uses_borrowed_sections_and_query_uses_filtered_refs() {
        let catalog = fixture_catalog();
        let full = catalog.sections(0, "");
        assert!(matches!(full[0].entries, SectionEntries::Full(_)));

        let filtered = catalog.sections(0, "arrow");
        assert!(filtered
            .iter()
            .all(|section| matches!(section.entries, SectionEntries::Filtered(_))));
    }

    #[test]
    fn empty_sections_are_dropped_and_no_lone_headers() {
        let catalog = fixture_catalog();
        // Tab 1 contains one empty section — unfiltered view drops it entirely.
        assert!(catalog.sections(1, "").is_empty());
        // A query matching nothing also yields no sections (no lone headers).
        assert!(catalog.sections(0, "zzzzzzz").is_empty());
    }

    #[test]
    fn out_of_range_tab_returns_empty() {
        let catalog = fixture_catalog();
        assert!(catalog.sections(99, "").is_empty());
        assert_eq!(catalog.tab_count(), 2);
    }

    #[test]
    fn selectable_and_flat_indices_round_trip() {
        let entries_a = [entry("a0"), entry("a1")];
        let entries_b = [entry("b0"), entry("b1"), entry("b2")];
        let sections = [
            SectionView {
                title: "a",
                entries: SectionEntries::Full(&entries_a),
            },
            SectionView {
                title: "b",
                entries: SectionEntries::Full(&entries_b),
            },
        ];

        assert_eq!(selectable_to_flat(&sections, 0), Some(1));
        assert_eq!(selectable_to_flat(&sections, 2), Some(4));
        assert_eq!(flat_to_selectable(&sections, 0), None);
        assert_eq!(flat_to_selectable(&sections, 4), Some(2));
        assert_eq!(entry_at_selectable(&sections, 4).unwrap().name, "b2");
    }

    #[test]
    fn adjust_scroll_offset_keeps_selected_row_visible() {
        assert_eq!(adjust_scroll_offset(10, 5, 8), 8);
        assert_eq!(adjust_scroll_offset(10, 5, 10), 10);
        assert_eq!(adjust_scroll_offset(10, 5, 14), 10);
        assert_eq!(adjust_scroll_offset(10, 5, 15), 11);
    }

    #[test]
    fn single_char_reports_single_scalar_only() {
        let ascii = IconEntry::new("x", "ascii");
        let emoji = IconEntry::new("🔥", "fire");
        let multi = IconEntry::new("❤\u{fe0f}", "heart");

        assert_eq!(ascii.single_char(), Some('x'));
        assert_eq!(emoji.single_char(), Some('🔥'));
        assert_eq!(multi.single_char(), None);
    }
}
