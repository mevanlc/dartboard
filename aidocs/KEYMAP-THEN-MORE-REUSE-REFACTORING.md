# KeyMap Then More Reuse Refactoring

Status note: references in older notes to the standalone `dartboard` crate/path now mean `dartboard-cli` in this workspace. The binary is still named `dartboard`, but the standalone host code lives under `dartboard-cli/`.

This note complements [IMPROVE-REUSABILITY.md](./IMPROVE-REUSABILITY.md) and now reflects the current post-refactor state rather than the original pre-implementation plan.

## Done

### `EditorAction` exists
- `EditorAction` is now the editor-facing command surface in `dartboard-editor`.
- The editor executes actions instead of directly hard-coding all behavior behind raw keys.

### `KeyMap` exists
- A default `KeyMap` lives in `dartboard-editor::keymap`.
- It maps `AppKey` values to abstract editor actions.
- It already carries binding descriptions as metadata.

### Standalone flow is now key -> action -> editor execution
- `dartboard-cli` resolves keys through `KeyMap`.
- The result is passed to `handle_editor_action(...)`.
- This is the core seam the note originally argued for.

### The split is materially better than before
- `dartboard-editor` owns the reusable editor session model and many editor operations.
- `dartboard-cli` still owns shell behavior, transport composition, undo/op submission, host-effect realization, help/picker UI, and hit-testing.

Relevant code:
- input surface: [dartboard-editor/src/lib.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/lib.rs:130)
- `EditorAction`: [dartboard-editor/src/lib.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/lib.rs:161)
- current key dispatch entrypoints: [dartboard-editor/src/lib.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/lib.rs:1297)
- keymap definition: [dartboard-editor/src/keymap.rs](/Users/mclark/p/my/dartboard/dartboard-editor/src/keymap.rs:42)
- standalone key policy and action execution: [dartboard-cli/src/app.rs](/Users/mclark/p/my/dartboard/dartboard-cli/src/app.rs:1166)

## Next

### Generate help from keymap metadata
This is the largest clearly pending item from the original plan.

Current state:
- `KeyMap` has binding descriptions
- `dartboard-cli` help is still manually maintained

Target:
- default help rendered from keymap metadata
- standalone help stops duplicating binding knowledge by hand

### Continue the reuse refactor using `EditorAction` as the seam
The next extraction work should follow the same pattern:
- identify reusable behavior
- express it as actions/effects/state transitions in `dartboard-editor`
- keep shell-only policy in `dartboard-cli`

The most promising next areas are:
- more pointer/editor dispatch extraction where it is not tightly coupled to shell hit regions
- broader editor-effect vs host-effect separation
- further narrowing of `dartboard-cli::app::App`

### Keep `KeyMap` as data, not policy glue
The right direction remains:
- `KeyMap` owns bindings and binding metadata
- `dartboard-editor` owns editor behavior
- `dartboard-cli` owns standalone shell policy

If keymap complexity grows, likely future work includes:
- alternate layouts
- user overrides
- serialized keymap configuration

## Deferred

### Giant all-at-once input refactor
Still not worth doing in one pass:
- keyboard keymap changes
- pointer mapping
- picker/help shell routing
- transport/session extraction
- host-effect redesign

Keyboard was the cleanest first seam. That was the right call. Pointer and shell routing are still more contextual.

### Fully generalized help/binding UX
The immediate target is generated help from current keymap metadata, not a complete customization system.

These can wait:
- per-host help layouts
- keybinding override UX
- binding serialization/import/export

## Current Architecture

### `dartboard-editor`
Owns:
- `EditorSession`
- editor state transitions
- canvas-editing behavior
- `EditorAction`
- `KeyMap`
- editor-facing effect emission

Does not fully own yet:
- standalone-specific floating overrides that depend on local undo grouping
- shell hit-testing
- undo/redo orchestration
- host-effect realization

### `dartboard-cli`
Still owns:
- crossterm event handling via adapter helpers
- standalone-only shell shortcuts and UI policy
- floating-specific behavior tied to local undo grouping
- pointer hit-testing for help tabs, swatches, and picker regions
- undo/redo stack ownership and op submission
- transport/session mirror composition
- host-effect realization

## Immediate Next Task

The clearest next implementation step is:

1. generate `dartboard-cli` keyboard help from `KeyMap` metadata
2. remove duplicated manual binding tables
3. then continue the remaining reuse refactors with the new seam in place
