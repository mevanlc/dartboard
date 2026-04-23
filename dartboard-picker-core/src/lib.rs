mod nerd_fonts;
pub mod sources;

use std::borrow::Cow;
use std::cell::RefCell;
use std::mem::size_of;

/// Lowercase `query` only when it actually contains ASCII uppercase or any
/// non-ASCII bytes. Pure-ASCII-lowercase queries — the common case for this
/// picker — borrow instead of allocating.
fn lowercased_query(query: &str) -> Cow<'_, str> {
    let needs_fold = query
        .bytes()
        .any(|b| b.is_ascii_uppercase() || !b.is_ascii());
    if needs_fold {
        Cow::Owned(query.to_lowercase())
    } else {
        Cow::Borrowed(query)
    }
}

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

    /// Heap bytes owned by this entry (string capacities; does not include the
    /// `size_of::<IconEntry>()` stack fields).
    pub fn heap_footprint(&self) -> usize {
        self.icon.capacity() + self.name.capacity() + self.name_lower.capacity()
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

    pub fn tabs(&self) -> &[Vec<SectionSpec>] {
        &self.tabs
    }

    /// Estimate heap bytes owned by the catalog. Sums string and Vec
    /// capacities; does not account for allocator overhead.
    pub fn heap_footprint(&self) -> usize {
        let mut total = self.tabs.capacity() * size_of::<Vec<SectionSpec>>();
        for tab in &self.tabs {
            total += tab.capacity() * size_of::<SectionSpec>();
            for spec in tab {
                total += spec.title.capacity();
                total += spec.entries.capacity() * size_of::<IconEntry>();
                for entry in &spec.entries {
                    total += entry.heap_footprint();
                }
            }
        }
        total
    }

    /// Return section views for the given tab, filtered by `query`. Empty
    /// sections are dropped so lone headers don't appear. Out-of-range
    /// `tab_idx` yields an empty Vec.
    pub fn sections(&self, tab_idx: usize, query: &str) -> Vec<SectionView<'_>> {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return Vec::new();
        };
        let query_lower = lowercased_query(query);
        let q = query_lower.as_ref();
        let mut out = Vec::new();
        for spec in tab {
            if q.is_empty() {
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
                    .filter(|e| e.name_lower.contains(q))
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

/// Wraps an [`IconCatalogData`] with a **trail-stack** cache: each successful
/// narrowing step pushes a new entry whose query is a strict extension of the
/// previous. Lookups pop entries whose query is not a prefix of the new query
/// (which handles backspaces naturally — the stack rewinds to the matching
/// prefix) and either return an exact hit, narrow from the longest cached
/// prefix, or fall all the way back to a cold scan.
///
/// Empty queries bypass the cache since the no-filter path already returns
/// borrowed slices. Tab switches invalidate the trail entirely.
pub struct MemoizedCatalog {
    inner: IconCatalogData,
    cache: RefCell<CacheState>,
}

#[derive(Default)]
struct CacheState {
    /// Which tab the current trail belongs to. A tab switch resets the trail.
    tab_idx: Option<usize>,
    /// Successive narrowings. Invariant: each entry's query strictly extends
    /// the previous entry's query.
    trail: Vec<CachedQuery>,
}

struct CachedQuery {
    query: String,
    sections: Vec<CachedSection>,
}

struct CachedSection {
    section_idx: u32,
    entries: Vec<u32>,
}

/// Soft cap on trail depth. Real human queries never get close; this only
/// matters to bound memory if something weird is happening.
const TRAIL_CAP: usize = 64;

impl MemoizedCatalog {
    pub fn new(inner: IconCatalogData) -> Self {
        Self {
            inner,
            cache: RefCell::default(),
        }
    }

    pub fn inner(&self) -> &IconCatalogData {
        &self.inner
    }

    pub fn into_inner(self) -> IconCatalogData {
        self.inner
    }

    pub fn tab_count(&self) -> usize {
        self.inner.tab_count()
    }

    /// Clear the memoization cache. Useful if the underlying catalog is ever
    /// mutated (not currently exposed, but kept for symmetry).
    pub fn invalidate(&self) {
        let mut cache = self.cache.borrow_mut();
        cache.tab_idx = None;
        cache.trail.clear();
    }

    /// Heap bytes owned by the catalog plus the cache state.
    pub fn heap_footprint(&self) -> usize {
        let cache = self.cache.borrow();
        let mut total = self.inner.heap_footprint();
        total += cache.trail.capacity() * size_of::<CachedQuery>();
        for step in &cache.trail {
            total += step.query.capacity();
            total += step.sections.capacity() * size_of::<CachedSection>();
            for section in &step.sections {
                total += section.entries.capacity() * size_of::<u32>();
            }
        }
        total
    }

    /// Filtered section views for `(tab_idx, query)`. Semantically identical
    /// to [`IconCatalogData::sections`] but reuses prior narrowing steps.
    pub fn sections(&self, tab_idx: usize, query: &str) -> Vec<SectionView<'_>> {
        // Empty-query path: no filter needed, borrowed slices only. Don't
        // mutate the trail — the user is likely mid-edit and will extend or
        // re-type.
        if query.is_empty() {
            return self.inner.sections(tab_idx, "");
        }

        let Some(tab) = self.inner.tabs.get(tab_idx) else {
            return Vec::new();
        };

        let query_lower = lowercased_query(query);
        let q = query_lower.as_ref();
        let layout: Vec<(usize, Vec<u32>)> = {
            let mut cache = self.cache.borrow_mut();

            // Tab switch invalidates the entire trail.
            if cache.tab_idx != Some(tab_idx) {
                cache.tab_idx = Some(tab_idx);
                cache.trail.clear();
            }

            // Pop any trail entries whose query isn't a prefix of the new
            // query. After this loop the top (if any) is either an exact
            // match or the longest cached prefix of `q`. This is the
            // backspace path.
            while cache
                .trail
                .last()
                .is_some_and(|top| !q.starts_with(top.query.as_str()))
            {
                cache.trail.pop();
            }

            let top_exact = cache
                .trail
                .last()
                .is_some_and(|top| top.query.as_str() == q);

            if !top_exact {
                let new_sections = match cache.trail.last() {
                    Some(top) => {
                        // Narrow from the longest cached prefix.
                        top.sections
                            .iter()
                            .filter_map(|cached| {
                                let spec = &tab[cached.section_idx as usize];
                                let entries: Vec<u32> = cached
                                    .entries
                                    .iter()
                                    .copied()
                                    .filter(|&i| spec.entries[i as usize].name_lower.contains(q))
                                    .collect();
                                (!entries.is_empty()).then_some(CachedSection {
                                    section_idx: cached.section_idx,
                                    entries,
                                })
                            })
                            .collect()
                    }
                    None => {
                        // Cold scan — no prior prefix to narrow from.
                        tab.iter()
                            .enumerate()
                            .filter_map(|(section_idx, spec)| {
                                let entries: Vec<u32> = spec
                                    .entries
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(i, entry)| {
                                        entry.name_lower.contains(q).then_some(i as u32)
                                    })
                                    .collect();
                                (!entries.is_empty()).then_some(CachedSection {
                                    section_idx: section_idx as u32,
                                    entries,
                                })
                            })
                            .collect()
                    }
                };

                cache.trail.push(CachedQuery {
                    query: q.to_owned(),
                    sections: new_sections,
                });

                // Soft cap — drop the oldest (shortest-prefix) entries first.
                while cache.trail.len() > TRAIL_CAP {
                    cache.trail.remove(0);
                }
            }

            cache
                .trail
                .last()
                .expect("just pushed or already present")
                .sections
                .iter()
                .map(|c| (c.section_idx as usize, c.entries.clone()))
                .collect()
        };

        layout
            .into_iter()
            .map(|(section_idx, indices)| {
                let spec = &tab[section_idx];
                let entries: Vec<&IconEntry> = indices
                    .into_iter()
                    .map(|i| &spec.entries[i as usize])
                    .collect();
                SectionView {
                    title: &spec.title,
                    entries: SectionEntries::Filtered(entries),
                }
            })
            .collect()
    }
}

impl From<IconCatalogData> for MemoizedCatalog {
    fn from(inner: IconCatalogData) -> Self {
        Self::new(inner)
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

    fn collect_names(sections: &[SectionView<'_>]) -> Vec<String> {
        let mut out = Vec::new();
        for s in sections {
            for i in 0..s.entries.len() {
                out.push(s.entries.get(i).unwrap().name.clone());
            }
        }
        out
    }

    #[test]
    fn memoized_matches_raw_across_tabs_and_queries() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        for (tab, query) in &[
            (0, ""),
            (0, "a"),
            (0, "arrow"),
            (0, "zzzzzz"),
            (1, ""),
            (1, "anything"),
            (99, ""),
        ] {
            let raw = memo.inner().sections(*tab, query);
            let cached = memo.sections(*tab, query);
            assert_eq!(
                collect_names(&raw),
                collect_names(&cached),
                "tab {tab} query {query:?}"
            );
        }
    }

    #[test]
    fn memoized_incremental_narrowing_produces_same_result() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        // "a" matches a0, a1, b0 arrow, b1 arrow; narrow to "arrow" should
        // match only b0 arrow and b1 arrow.
        let warm = memo.sections(0, "a");
        assert_eq!(
            collect_names(&warm),
            vec!["a0", "a1", "b0 arrow", "b1 arrow"]
        );
        let narrowed = memo.sections(0, "arrow");
        assert_eq!(collect_names(&narrowed), vec!["b0 arrow", "b1 arrow"]);
        // Compare against the un-memoized path to be sure.
        let reference = memo.inner().sections(0, "arrow");
        assert_eq!(collect_names(&narrowed), collect_names(&reference));
    }

    #[test]
    fn memoized_handles_query_shrink_via_trail_pop() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        // Type "a" → "arrow". Now backspace back to "a".
        let _ = memo.sections(0, "a");
        let _ = memo.sections(0, "ar");
        let _ = memo.sections(0, "arrow");
        let shrunk = memo.sections(0, "a");
        let reference = memo.inner().sections(0, "a");
        assert_eq!(collect_names(&shrunk), collect_names(&reference));
    }

    #[test]
    fn memoized_handles_cold_shrink_without_trail() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        // Jump straight to a long query, then shrink. The trail has only
        // "arrow"; backspacing to "a" should pop everything and cold-scan.
        let _ = memo.sections(0, "arrow");
        let shrunk = memo.sections(0, "a");
        let reference = memo.inner().sections(0, "a");
        assert_eq!(collect_names(&shrunk), collect_names(&reference));
    }

    #[test]
    fn memoized_tab_switch_invalidates_trail() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        let _ = memo.sections(0, "a");
        // Switch to tab 1 (only has an empty section — everything drops).
        let other = memo.sections(1, "anything");
        let reference = memo.inner().sections(1, "anything");
        assert_eq!(collect_names(&other), collect_names(&reference));
        // Switch back — must still produce correct results.
        let back = memo.sections(0, "arrow");
        let ref2 = memo.inner().sections(0, "arrow");
        assert_eq!(collect_names(&back), collect_names(&ref2));
    }

    #[test]
    fn memoized_non_prefix_edit_discards_trail() {
        let memo = MemoizedCatalog::new(fixture_catalog());
        // "arrow" then "b" — "b" isn't a prefix of "arrow", trail pops empty,
        // cold scan.
        let _ = memo.sections(0, "a");
        let _ = memo.sections(0, "arrow");
        let shifted = memo.sections(0, "b");
        let reference = memo.inner().sections(0, "b");
        assert_eq!(collect_names(&shifted), collect_names(&reference));
    }
}
