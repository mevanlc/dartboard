# Phase 1 Reusable Picker Pieces

Goal: make the richer `dartboard` glyph picker the source of truth for `late-sh` Artboard without prematurely freezing a giant "shared picker widget" API.

This phase is intentionally narrow. It extracts the reusable picker pieces that are already proving stable, keeps host-specific rendering and search editing local, and uses `late-sh` Artboard as the first real consumer of the fuller `dartboard` picker model.

## Goals

- `late-sh` Artboard gets the fuller `dartboard` picker data/model:
  - emoji
  - unicode
  - nerd font
  - unicode box-drawing section
  - current borrowed/filtering behavior for large unicode sets
- Shared picker logic lives inside the `dartboard` workspace, not in a new repo.
- `dartboard-cli` keeps its current picker UX and visuals.
- `late-sh` Artboard gets its own host-specific chrome and input wiring, but stops owning a forked catalog/query model.
- The extracted seam stays below `dartboard-cli` and outside `dartboard-editor`.

## Non-goals

- No separate repo in this phase.
- No attempt to share a full ratatui widget.
- No attempt to unify search editing widgets:
  - `dartboard-cli` currently uses `String` + cursor index.
  - `late-sh` currently uses `TextArea`.
- No attempt to replace the existing `late-sh` chat-composer picker in this phase.
- No attempt to move picker UI into `dartboard-editor`.

## Why this boundary

The current code already points to the right seam.

- `dartboard-editor` is deliberately host-neutral editor/session logic. Picker UI does not belong there.
- `dartboard-cli` picker code is split into:
  - catalog/query model
  - list-selection math
  - ratatui rendering
  - standalone input and hit-testing policy
- `late-sh` already diverged heavily in the host-facing parts:
  - different theme
  - different search widget
  - different footer/layout
  - different integration target

The stable part is the glyph catalog plus the list/selection math. The unstable part is the widget/chrome/search editor behavior.

That means phase 1 should extract the stable part only.

## Recommended shape

Add a new workspace crate:

- `dartboard-picker-core`

This crate should own picker-domain logic, not terminal UI.

It should depend on:

- `emojis`
- `unicode_names2`

It should not depend on:

- `ratatui`
- `crossterm`
- `dartboard-editor`

## Proposed contents of `dartboard-picker-core`

### Catalog/domain types

- `GlyphEntry`
  - `icon: String`
  - `name: String`
  - `name_lower: String`
- `PickerTab`
  - `Emoji`
  - `Unicode`
  - `NerdFont`
- `SectionEntries<'a>`
  - `Full(&'a [GlyphEntry])`
  - `Filtered(Vec<&'a GlyphEntry>)`
- `SectionView<'a>`
  - `title: &'static str`
  - `entries: SectionEntries<'a>`
- `GlyphCatalog`
  - owns the prebuilt emoji/unicode/nerd-font sections
  - exposes `sections(tab, query)`

### Pure picker helpers

- `selectable_count(...)`
- `flat_len(...)`
- `selectable_to_flat(...)`
- `flat_to_selectable(...)`
- `entry_at_selectable(...)`
- `adjust_scroll(...)` or equivalent pure helper, if we want to stop duplicating that logic too

### Artboard-oriented helper

Because `late-sh` Artboard ultimately wants a single brush glyph, the shared crate should expose a helper such as:

- `GlyphEntry::single_char() -> Option<char>`

That keeps the "is this entry usable as a single-cell artboard brush?" rule in one place instead of repeating `chars().count() == 1` checks in hosts.

## What stays local to each host

### Keep in `dartboard-cli`

- popup layout
- tab strip rendering
- search field rendering
- footer/keymap hints
- mouse hit-test rectangles
- string/cursor editing behavior
- "insert and keep open" host behavior

### Keep in `late-sh`

- Artboard-specific popup chrome
- `TextArea`-based search widget if still desired
- parser/input integration
- mapping selected glyph to Artboard brush state
- any Artboard-only affordances such as showing the active brush in the sidebar

## Why not a separate repo

Not yet.

Right now:

- there is only one serious source implementation
- the first consumer is still in the same local workspace ecosystem
- the API shape is still moving

A separate repo would add release/versioning overhead before the abstraction is stable. The right first step is a workspace-local crate. If this seam proves stable after `dartboard-cli` and `late-sh` both consume it, moving it later is straightforward.

## Why not share the full widget

Because the widget is still host-shaped.

Concrete examples:

- `dartboard-cli` stores tab/list rects for pointer hit-testing.
- `late-sh` uses `TextArea` rather than raw string editing.
- both apps want different colors, borders, titles, and footer hints.

Trying to generalize all of that now would produce a fake abstraction and slow down both consumers.

The fuller picker can still become "the `late-sh` Artboard picker" without sharing the exact widget type. The important part is that both are driven by the same catalog, sectioning, and selection model.

## Phase 1 implementation steps

1. Create `dartboard-picker-core` in the `dartboard` workspace.
2. Move the reusable catalog/query code out of `dartboard-cli/src/emoji/catalog.rs` into the new crate.
3. Move the reusable list/selection helpers out of `dartboard-cli/src/emoji/picker.rs` into the new crate.
4. Keep `dartboard-cli` rendering code local, but convert it to consume the new crate types.
5. Add focused tests in the new crate for:
   - unicode catalog count sanity
   - borrowed filtered views
   - `Unicode` tab section ordering
   - flat/selectable index conversions
   - `single_char()` behavior for Artboard-safe glyphs
6. In `late-sh`, add a new Artboard-facing picker module that consumes `dartboard-picker-core`.
7. Wire the selected glyph into Artboard brush selection:
   - selected entry must resolve to `char`
   - `Enter` applies and closes
   - optional keep-open behavior can mirror `dartboard-cli`
8. Leave the existing `late-sh` chat-composer picker untouched for now.
9. After Artboard is working, decide whether the chat-composer picker should also migrate to the shared crate or remain intentionally separate.

## Suggested late-sh scope for this phase

The first `late-sh` consumer should be Artboard only.

That keeps scope under control because:

- Artboard has a clear single-glyph result shape.
- It directly benefits from the unicode and box-drawing sections.
- It avoids mixing this work with the current chat-composer picker behavior.

This should be a new Artboard picker flow, not a forced rewrite of the existing global `late-sh` picker on the same pass.

## Compatibility notes

`late-sh` Artboard currently uses single-cell glyph brushes, so the shared picker data needs to preserve the current `dartboard` discipline around entry shape.

Implications:

- emoji entries should remain restricted to single-character selections suitable for one cell
- unicode entries are already naturally single-character
- nerd-font entries should continue to be treated as single glyph selections

If we ever want multi-scalar emoji or grapheme clusters later, that is a different plan because it changes Artboard brush and canvas assumptions.

## Verification

### `dartboard`

- `cargo test -p dartboard-picker-core`
- `cargo test -p dartboard-cli`
- manually verify the standalone picker still behaves exactly as before

### `late-sh`

- Artboard opens the new picker
- unicode tab is present
- box-drawing section is present
- mouse and keyboard selection work
- selected glyph becomes the active Artboard brush
- no regression to existing canvas editing flow

## Out of scope

- shared ratatui renderer
- shared search editing state
- shared pointer hit-testing state
- replacing the current `late-sh` chat-composer picker
- publishing a reusable crate outside this repo set
- grapheme-cluster or multi-cell glyph support

## Open questions for review

1. Crate name: `dartboard-picker-core` vs `dartboard-glyphs`.
2. Whether `adjust_scroll(...)` belongs in the shared crate or stays host-local for one more step.
3. Whether phase 1 should include a minimal shared "picker state core" with:
   - `tab`
   - `selected_index`
   - `scroll_offset`
   or keep even that local and only share pure helpers.
4. Whether `late-sh` should keep its `TextArea` search editor for Artboard or switch the Artboard picker to the simpler `dartboard-cli` string/cursor model.

## Recommendation

Approve a workspace-local extraction of the catalog + selection seam first.

Do not:

- create a separate repo
- move picker UI into `dartboard-editor`
- try to generalize the full widget yet

Do:

- make `dartboard-picker-core` the shared source of truth
- use `late-sh` Artboard as the first consumer of the fuller picker model
- leave host-specific rendering and input policy where it belongs

