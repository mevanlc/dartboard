# KeyMap Then More Reuse Refactoring

This note captures the current state of the reuse work and the next refactoring direction after recognizing that `KeyMap` / `EditorAction` is now the right organizing seam.

It is meant to complement, not replace, [IMPROVE-REUSABILITY.md](./IMPROVE-REUSABILITY.md).

## Current state

`dartboard` has already gained several useful lower layers:

- `dartboard-tui` for reusable canvas rendering
- `dartboard-editor` for crossterm-free editor/session types and editor behavior
- public host-neutral input types:
  - `AppIntent`
  - `AppKey` / `AppKeyCode`
  - `AppPointerEvent` / `AppPointerKind`
- a minimal `HostEffect` surface

The current editor/input split looks like this:

- `dartboard-editor` owns the editor session model and many pure editor operations
- `dartboard::app::App` still owns standalone shell behavior, transport composition, undo/op submission, host-effect realization, help/picker UI, and hit-testing

The most recent extraction moved the non-shell keyboard dispatch path into `dartboard-editor`.

Relevant current code:

- input surface: [dartboard-editor/src/lib.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/lib.rs:80)
- current key dispatch: [dartboard-editor/src/lib.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/lib.rs:1180)
- remaining standalone key policy and pointer routing: [dartboard/src/app.rs](/Users/mclark/p/my/dartboard/dartboard/src/app.rs:1303)

So the architecture is better than before, but it still has an important weakness:

- editor behavior is now reusable
- key bindings are still hardcoded inside the editor layer

That makes further reuse work awkward, because the next extraction steps would otherwise keep following a "raw keys first, behavior second" shape.

## Why introduce `EditorAction` and `KeyMap` now

This is the right point to introduce them because the code has just reached a transitional state:

- the editor layer is large enough to deserve a stable abstract command surface
- the host-neutral input types already exist
- the default standalone bindings are now concentrated enough to extract cleanly

Without this step, more refactoring would continue to bake default keyboard choices into reusable editor code.

With this step, later refactors can follow a cleaner pattern:

1. host input arrives as `AppIntent`
2. a keymap turns raw keys into abstract actions
3. the editor executes actions
4. the host realizes effects and handles shell-only concerns

That is a better fit for:

- customizable key bindings
- automatically generated keyboard help
- alternate hosts like `late-sh`
- future default-layout changes without editor rewrites

## Proposed separation

### `dartboard-editor`

Should own:

- `EditorSession`
- editor state transitions
- canvas-editing behavior
- editor-facing effect emission
- abstract actions like `EditorAction`

Should not own:

- default keyboard layout choices
- help text tied to concrete bindings
- crossterm parsing
- standalone picker/help/window policy

### `KeyMap`

Should own:

- mapping from `AppKey` to abstract actions
- binding metadata for help text
- the default standalone key layout
- eventually user overrides / alternate layouts

Should not own:

- editor state mutation directly
- transport logic
- terminal/crossterm-specific event parsing

### `dartboard::app::App`

Should continue to own:

- crossterm event handling via adapter helpers
- standalone-only shell shortcuts and UI policy
- floating-specific behavior that still depends on local undo grouping
- pointer hit-testing for shell zones like help tabs, swatches, picker regions
- undo/redo stack ownership and op submission
- transport/session mirror composition
- host-effect realization

## Proposed core abstraction

Introduce an `EditorAction` enum as the editor-facing command surface.

Likely categories:

- movement actions
- selection actions
- clipboard/swatch actions
- fill/border/edit actions
- viewport pan actions
- floating actions
- text insertion / paste actions

Examples:

- `MoveLeft`
- `MoveRight`
- `MoveToLineStart`
- `BeginSelection`
- `ClearSelection`
- `CopySelection`
- `CutSelection`
- `PastePrimarySwatch`
- `SmartFill`
- `DrawBorder`
- `InsertChar(char)`
- `PasteText(String)`
- `TransposeSelectionCorner`
- `ActivateSwatch(usize)`
- `ToggleFloatingTransparency`

This should replace the current "execute raw key directly" entrypoint in `dartboard-editor`.

Instead of:

- `handle_editor_key_press(editor, canvas, key, color)`

the editor layer should move toward:

- `handle_editor_action(editor, canvas, action, color)` or similar

## Proposed keymap abstraction

After `EditorAction` exists, add a `KeyMap` layer above it.

Suggested shape:

- `KeyBinding`
- `KeyMap`
- `BindingContext` if needed later

The initial version can be simple:

- one default keymap
- map `AppKey` to one or more `EditorAction`s
- include enough metadata for help text rendering

Possible placement:

- `dartboard-keymap`
- or `dartboard-editor::keymap`

The package split is less important than the conceptual split.

If the code stays small, starting inside `dartboard-editor` as a `keymap` module is reasonable. If it grows into presets, overrides, serialization, and help metadata, a dedicated crate will make more sense.

## Help generation goal

The desired direction is:

- default help should be rendered from binding data, not maintained as separate prose tables
- standalone `dartboard` help UI should consume keymap metadata
- alternate hosts should be free to render their own help or ignore it

This does not require solving every documentation/UI issue up front. It only requires that the binding data become explicit and queryable.

## Recommended order of work

### 1. Introduce `EditorAction`

Refactor the current key-driven editor dispatch into abstract actions.

Goal:

- the editor executes actions, not keys

This is the key prerequisite for the rest.

### 2. Add default `KeyMap`

Move the current hardcoded keyboard binding choices out of the editor behavior path and into a default binding table.

Goal:

- current standalone behavior remains the same
- the binding decisions become data

### 3. Update standalone `App` to use keymap -> action -> editor execution

Goal:

- `App` stops knowing the default editor keyboard layout directly
- `App` still keeps shell-only shortcuts and policies where appropriate

### 4. Generate keyboard help from keymap metadata

Goal:

- help stops duplicating binding knowledge manually

### 5. Continue the remaining reuse refactors using `EditorAction` as the pattern

After that, continue with:

- more pointer/editor dispatch extraction where appropriate
- better editor-effect vs host-effect separation
- session mirror extraction around transport/client state

## What should wait

Do not try to solve all input abstraction at once.

Specifically, do not combine these into one giant refactor:

- keyboard keymap
- pointer mapping
- picker/help shell routing
- transport/session mirror extraction
- host-effect redesign

Keyboard is the cleanest next seam. Mouse/pointer routing is more contextual because some of it depends on UI hit regions and standalone undo grouping.

So the intended next move is:

- keyboard first
- pointer later

## Immediate next task

The next implementation task should be:

1. add `EditorAction`
2. refactor the current editor key-dispatch code to produce/execute actions
3. only then introduce `KeyMap`

That order keeps the design honest:

- `KeyMap` should target abstract actions
- not another raw-key dispatcher hidden in a different place
