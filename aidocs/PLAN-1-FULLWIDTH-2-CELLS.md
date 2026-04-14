# Fullwidth Glyphs As 2 Canvas Cells

## Goal
- Support glyphs whose terminal display width is `2` as first-class canvas content.
- Keep the canvas visually stable across terminals by making wide glyph occupancy explicit in the data model instead of relying on terminal overlap behavior.
- Preserve the editor's current feel: simple movement, selection, copy/paste, floating selections, and picker insertion should still behave predictably.

## Non-Goals
- Do not turn the canvas into a grapheme-cluster editor in this pass.
- Do not support arbitrary shaping, ligatures, or combining-mark composition as a first-class layout system.
- Do not optimize for file/network serialization yet beyond leaving room for it.

## Current Problem
- The canvas currently stores exactly one `char` at each `Pos` in [`src/canvas.rs`](</Users/mclark/p/my/dartboard/src/canvas.rs:12>).
- Rendering in [`src/ui.rs`](</Users/mclark/p/my/dartboard/src/ui.rs:27>) assumes one canvas column maps to one terminal cell.
- Insertion and paste paths in [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:684>) and [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:910>) always advance by one canvas column per codepoint.
- That model works for ASCII and narrow Unicode, but it breaks for emoji and some symbols because they visually occupy two terminal columns.

## Decision
- Treat canvas `x` coordinates as terminal-cell columns, not codepoint slots.
- A fullwidth glyph occupies two adjacent canvas cells.
- The left cell is the owning cell.
- The right cell is a continuation cell and may not hold an independent glyph.

## Core Invariants
- Every visible glyph has exactly one owning cell.
- Narrow glyphs occupy one cell.
- Fullwidth glyphs occupy two cells: owner at `x`, continuation at `x + 1`.
- A continuation cell must always point back to an owner immediately to its left.
- No operation may leave an orphan continuation cell.
- No operation may place a fullwidth glyph at the last column unless it either clamps, rejects, or clears space according to a single explicit policy.
- Cursor positions remain cell-based, not glyph-index-based.

## Proposed Data Model

### Canvas cell representation
- Replace `HashMap<Pos, char>` with something like `HashMap<Pos, CellValue>`.
- `CellValue` should distinguish:
  - empty
  - narrow owner with `char`
  - wide owner with `char`
  - wide continuation

### Recommended shape
```rust
enum CellValue {
    Narrow(char),
    Wide(char),
    WideCont,
}
```

### Why this shape
- It is minimal.
- It keeps occupancy local to the row.
- It avoids hidden side tables or width recomputation during every edit.
- It gives rendering and edit code enough information to avoid drawing stray trailing halves.

### Width helper
- Add a single width classifier, probably based on `unicode-width`.
- Restrict this plan to width `1` and width `2`.
- Treat width `0` and ambiguous cases conservatively at first:
  - either reject them from picker insertion
  - or normalize them to width `1` only if that is defensible
- The important part is to have one canonical function that every edit path uses.

## Canvas API Changes

### Replace `set/get/clear` with occupancy-aware operations
- Current `Canvas::set`, `get`, and `clear` in [`src/canvas.rs`](</Users/mclark/p/my/dartboard/src/canvas.rs:28>) are too weak for wide-cell invariants.
- Introduce APIs with explicit semantics:
  - `put_glyph(pos, ch) -> PutResult`
  - `clear_cell(pos)`
  - `glyph_at(pos) -> Option<GlyphRef>`
  - `cell_at(pos) -> Option<&CellValue>`
  - `display_width(ch) -> usize`

### Semantics of `put_glyph`
- If `ch` is narrow:
  - clear any wide glyph overlap touching `pos`
  - write `Narrow(ch)` at `pos`
- If `ch` is wide:
  - require room for `pos` and `pos + 1`
  - clear any overlapping glyphs occupying either cell
  - write `Wide(ch)` at `pos`
  - write `WideCont` at `pos + 1`

### Semantics of `clear_cell`
- Clearing an owner clears its whole occupied span.
- Clearing a continuation clears both the continuation and its owner.

### Row/column shift operations
- Current `push_*` and `pull_*` implementations in [`src/canvas.rs`](</Users/mclark/p/my/dartboard/src/canvas.rs:49>) move individual cells and will corrupt wide glyphs.
- Rewrite these to move glyph spans, not raw cells.
- For horizontal operations, work in row-local glyph runs.
- For vertical operations, move whole glyph occupancy at a given column pair safely.

## App-Level Behavior Changes

### Cursor movement
- Keep cursor movement cell-based.
- Moving right from the owner of a wide glyph should move one cell unless we intentionally want glyph-wise motion.
- That keeps the rest of the app simpler.
- Add optional helpers later if we want "skip continuation" behavior for some commands.

### Typing and picker insertion
- `insert_char` in [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:684>) should:
  - insert via `Canvas::put_glyph`
  - advance by the glyph display width, not always by one
- Picker no-close insertion should use the same path.

### Backspace and delete
- `backspace` and `delete_at_cursor` in [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:941>) must become span-aware.
- Backspace from immediately after a wide glyph should delete the whole glyph.
- Delete on either half of a wide glyph should delete the whole glyph.

### Paste and text block insertion
- `paste_text_block` in [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:910>) must advance `x` by display width per inserted character.
- Clipboard pasting must preserve stored occupancy exactly when the clipboard is canvas-native.
- For plain pasted text, derive occupancy from character width during insertion.

### Selection model
- Selection bounds can remain cell-rectangles.
- Filling a selection with one glyph should tile by occupied width, not just by raw cell count, or we should explicitly document that fill writes at each cell and wide glyphs overwrite neighboring cells.
- Need one policy for what happens if a wide glyph lands in the final selected column.

### Floating selections
- `Clipboard` today stores a rectangular grid of chars in [`src/app.rs`](</Users/mclark/p/my/dartboard/src/app.rs:63>).
- That is insufficient for wide occupancy.
- Replace clipboard storage with rectangular `CellValue` data, or a compact glyph-span representation that can round-trip exactly.
- `stamp_floating`, `stamp_onto_canvas`, and rendering of floating previews all need occupancy-aware writes and draws.

## UI / Rendering Changes

### Main canvas rendering
- `CanvasWidget::render` in [`src/ui.rs`](</Users/mclark/p/my/dartboard/src/ui.rs:27>) must render only owner cells.
- Continuation cells should not independently call `set_char`.
- Styling should still apply to both occupied cells for selection/floating background.

### Cursor rendering
- The terminal cursor should never be placed on a continuation cell if we can avoid it.
- At minimum, map continuation-cell cursor positions back to the owner when setting the terminal cursor.
- Better: add a small helper `display_cursor_pos()` that normalizes cursor placement.

### Selection highlighting
- A selected wide glyph should visually highlight both cells.
- If only the continuation half falls inside the selection rectangle, we need a policy:
  - either highlight exactly the cells in the rectangle
  - or widen to include the owner
- Exact-rectangle semantics are simpler and more consistent with the current model.

### Floating preview rendering
- Preview should show wide glyphs only from owner cells but apply background to the full occupied span.

## Clipboard / Serialization

### Internal clipboard
- Must preserve occupancy exactly.
- Recommended format: rectangular `Vec<CellValue>` with explicit width/height.

### System clipboard export
- Export as plain text by walking owner glyphs left-to-right and skipping continuations.
- Preserve spaces for empty cells and continuation positions as needed to maintain text shape.
- We need to decide whether the plain-text export should emit one codepoint for a wide glyph and rely on terminal width, or pad with a trailing space.
- Recommendation: emit the glyph only once and skip continuation cells. Plain text should represent glyphs, not internal occupancy sentinels.

### File I/O
- Not yet implemented, but this plan should assume future persistence stores glyphs plus occupancy, not raw `char` per cell.

## Migration Strategy

### Phase 1: Introduce occupancy-aware primitives
- Add width classification and `CellValue`.
- Replace direct raw-cell mutations with safe canvas methods.
- Keep external behavior mostly unchanged except for wide glyph correctness.

### Phase 2: Update rendering and cursor normalization
- Render owners and continuations correctly.
- Normalize cursor placement for terminal output.
- Add focused rendering tests.

### Phase 3: Update edit operations
- Insert, delete, backspace, paste, picker insert, and fill.
- Then rewrite row/column push/pull to move glyph spans safely.

### Phase 4: Update clipboard and floating selections
- Convert clipboard storage to occupancy-aware data.
- Fix stamping and preview.
- Add regression tests for copy/cut/paste of wide glyphs.

### Phase 5: Audit all selection and export paths
- Selection painting
- OSC52 export
- future save/load assumptions

## Edge Cases To Decide Explicitly
- Inserting a wide glyph at the rightmost column:
  - reject
  - clamp
  - replace final cell with space and no-op
  - wrap
- Recommendation: no-op or reject cleanly. Do not wrap implicitly.

- Overwriting the continuation half of an existing wide glyph:
  - Recommendation: clear the whole old glyph, then insert the new glyph.

- Moving the cursor into a continuation cell via mouse click:
  - Recommendation: allow logical cell selection internally, but normalize for rendering and destructive edits.

- Selection rectangles that cut through half of a wide glyph:
  - Recommendation: keep rectangle semantics; editing routines decide how to resolve overlap.

- Width-zero combining marks:
  - Recommendation: defer; either reject for now or treat as unsupported in picker/text insertion.

## Testing Plan

### Canvas unit tests
- put narrow glyph
- put wide glyph
- overwrite wide owner with narrow
- overwrite continuation with narrow
- clear owner clears continuation
- clear continuation clears owner
- reject or clamp wide glyph at right edge
- push/pull operations preserve wide spans

### App tests
- wide glyph insert advances cursor by `2`
- backspace deletes full wide glyph
- delete deletes full wide glyph from either occupied cell
- picker insert writes wide occupancy correctly
- no-close repeated picker inserts chain correctly
- plain paste with mixed narrow/wide text lands at expected cell positions

### UI tests
- rendering skips continuations as independent glyphs
- selection background covers both cells of a wide glyph
- cursor normalizes away from continuation cells

### TTY verification
- verify in tmux and kitty alternate screen
- verify emoji picker insertion
- verify copy/cut/paste and floating previews

## Risks
- The biggest risk is partial migration: some paths using raw cell moves and others using glyph-span moves.
- The second risk is making clipboard and selection semantics inconsistent with rendering semantics.
- The third risk is off-by-one behavior at the right edge and around continuation cells.

## Recommended Implementation Order
1. Add `unicode-width` and a width helper.
2. Introduce `CellValue` and safe canvas mutation APIs.
3. Convert rendering to owner/continuation-aware drawing.
4. Convert insert/delete/backspace/paste paths.
5. Convert clipboard and floating selection storage.
6. Rewrite push/pull operations.
7. Add tmux-based regression checks for picker and canvas behavior.

## Open Questions
- Should cursor motion skip continuation cells or remain raw cell-based everywhere?
- Should system clipboard export preserve internal column occupancy with trailing spaces after wide glyphs?
- Should selection operations be cell-exact or glyph-normalized when a rectangle intersects only half of a wide glyph?

## Recommendation
- Proceed with the explicit two-cell occupancy model.
- Keep cursor and selection coordinates cell-based for now.
- Normalize only rendering and destructive edit behavior around continuation cells.
- Avoid a larger grapheme-cluster abstraction until the app actually needs it.
