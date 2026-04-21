# Improve Reusability

Status note: references in older notes to the standalone `dartboard` crate/path now mean `dartboard-cli` in this workspace. The binary is still named `dartboard`, but the standalone host code lives under `dartboard-cli/`.

## Done

### `dartboard-tui`
- Reusable ratatui canvas rendering lives in `dartboard-tui`.
- It owns read-only rendering concerns such as:
  - `CanvasWidget`
  - `CanvasWidgetState`
  - `CanvasStyle`
  - `SelectionView`
  - `FloatingView`
- This is the rendering seam for hosts that want to embed the canvas without reusing standalone chrome.

### Library entrypoint for `dartboard-cli`
- `dartboard-cli/src/lib.rs` exports the standalone app/theme/input/ui modules.
- The standalone binary now consumes that library instead of being the only entrypoint.
- This is the minimum required step for another crate to depend on the standalone host code as code instead of shelling out to the binary.

### Host-neutral input and effect surface
- `AppIntent`
- `AppKey` / `AppKeyCode`
- `AppPointerEvent` / `AppPointerKind`
- `HostEffect`
- `App::handle_intent(...)`
- `App::tick()`

This is the key non-UI seam already in place. A host can now:
- translate its own input model into `AppIntent`
- drive editor/session logic
- handle returned `HostEffect`s in a host-specific way

### Public crossterm adapter module
- `dartboard-cli` exposes the crossterm adapter helpers via its `input` module
- `app_intent_from_crossterm(...)`
- `app_key_from_crossterm(...)`
- `app_pointer_event_from_crossterm(...)`

This means:
- the standalone `dartboard-cli` host consumes the same adapter helpers it exposes
- embedders no longer need to reach into standalone app internals to reuse the crossterm mapping

### `dartboard-editor`
- `dartboard-editor` exists as a crossterm-free reusable editor crate.
- It owns:
  - host-neutral input types
  - reusable per-user editor session state
  - editor model types
  - `EditorAction`
  - `KeyMap`
  - `HostEffect`
  - many pure canvas/editor helpers
  - `SessionMirror`

This is the biggest completed reuse step so far. In practice:
- reusable viewport/cursor/selection state lives there
- swatch and floating-selection state transitions live there
- cut/copy/paste/fill/border helpers live there
- non-shell keyboard dispatch now goes through `EditorAction` and `KeyMap`

### Explicit full-session handshake rejection
- `ServerMsg::ConnectRejected`
- server-side capacity gate
- websocket client rejection path

This is both a transport improvement and a cleaner embedding surface because hosts can treat connect failure as explicit state instead of timeout-like behavior.

## Next

### Narrow `dartboard-cli::app::App`
`dartboard-cli/src/app.rs` is still the main composition bottleneck. It still owns:
- canvas ownership
- undo/redo orchestration
- transport composition
- floating-specific policy
- shell-only help/picker behavior
- host-effect realization

The next real reuse win is to keep `dartboard-cli` as a composition root and move more reusable state-machine behavior below it.

### Finish the editor/session split
`dartboard-editor` owns the reusable session model, but not all of the state machine yet.

The remaining candidates are:
- more pointer/editor dispatch extraction where it is not tightly coupled to shell hit-testing
- clearer ownership of undo/redo policy
- more host-independent canvas mutation orchestration

### Broaden `HostEffect`
`HostEffect` exists, but it is still intentionally small.

Likely next additions if another host needs them:
- `SetCursorStyle(...)`
- `ShowNotice(String)`
- other small host-realized effects that should not be hard-coded in `dartboard-cli`

### Complete the input-boundary cleanup
The crossterm adapter exists. The main remaining boundary work is for hosts that do their own VT/input parsing.

The likely next step is:
- document and maybe add a small VT-oriented adapter surface

### Use keymap metadata for help
`KeyMap` now carries binding metadata, but `dartboard-cli` help is still hand-maintained prose.

That means one of the intended reuse payoffs is still pending:
- default help generated from binding data instead of duplicated tables

## Deferred

### Host-specific chrome
These are still better left in `dartboard-cli` or in host-specific code for now:
- outer frame/title bar/help panel chrome
- picker UI details
- terminal startup and teardown
- SSH-specific cursor-shape handling
- `late-sh`-specific parser details

### Bigger host abstractions without a second concrete consumer
These should wait until a second host genuinely needs them:
- a large generalized host-effect system
- a heavyweight shared input framework
- extraction of every shell policy from `dartboard-cli`

## Practical Target

The medium-term target is still:

1. `late-sh` owns its own parser and shell
2. `late-sh` translates input into `AppIntent`
3. `late-sh` drives reusable editor/session objects
4. `late-sh` renders with `dartboard-tui`
5. `late-sh` realizes returned `HostEffect`s in its own environment

At that point:
- `dartboard-cli` remains just one host
- reusable logic lives below it
- `late-sh` does not need to fork the editor stack
