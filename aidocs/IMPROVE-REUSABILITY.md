# Improve Reusability

Notes on seams already introduced in `dartboard`, plus the next abstractions worth adding so hosts like `late-sh` can embed it without `dartboard` depending on host code.

## Introduced

### `dartboard-tui`
- New reusable ratatui canvas widget crate
- Owns read-only rendering concerns:
  - `CanvasWidget`
  - `CanvasWidgetState`
  - `CanvasStyle`
  - `SelectionView`
  - `FloatingView`
- This is the right seam for any host that wants to render a dartboard canvas inside its own ratatui frame without reusing standalone chrome

### Library entrypoint for `dartboard`
- `dartboard/src/lib.rs` now exports the app/theme/ui modules
- The standalone binary now consumes the library instead of being the only entrypoint
- This is the minimum required step for another crate to depend on `dartboard` as code instead of shelling out to the binary

### Host-neutral input/effect surface
- `AppIntent`
- `AppKey` / `AppKeyCode`
- `AppPointerEvent` / `AppPointerKind`
- `HostEffect`
- `App::handle_intent(...)`
- `App::tick()`

This is the key non-UI seam added so far. A host can now:
- translate its own input model into `AppIntent`
- drive the editor/session logic
- handle returned `HostEffect`s in a host-specific way

The important property is that the app no longer needs crossterm event types as its only control surface.

### Public crossterm adapter module
- `dartboard::input`
- `app_intent_from_crossterm(...)`
- `app_key_from_crossterm(...)`
- `app_pointer_event_from_crossterm(...)`

This finishes the first half of the input-boundary cleanup:
- standalone `dartboard` now consumes the same adapter helpers it exposes
- embedders do not need to reach into `app.rs` internals to reuse the crossterm mapping

The remaining gap is a second adapter surface for hosts that do their own VT/input parsing.

### `dartboard-editor`
- New crossterm-free crate for reusable editor-facing types
- Owns:
  - host-neutral input types (`AppIntent`, `AppKey`, pointer types)
  - reusable per-user editor session state (`EditorSession`, `Viewport`, `PanDrag`)
  - editor model types (`Mode`, `Selection`, `Clipboard`, `Swatch`, `FloatingSelection`)
  - `HostEffect`
  - pure canvas diff helper used by editor/host layers

This is the first real move toward a reusable editor crate:
- `late-sh` can target a lower-level crate for input/effect/model types
- `dartboard` now consumes those shared definitions instead of owning them all locally
- standalone `App` now stores/restores per-user state through `EditorSession` instead of a fully local-only shape
- `dartboard-editor` now also owns the reusable viewport/cursor/selection state transitions (`set_viewport`, cursor motion, pan/clamp, selection bounds helpers)
- `dartboard-editor` now owns swatch history rotation/pinning plus floating-selection activation/transparency toggles
- `dartboard-editor` now owns pure canvas helpers for selection capture/export and selection-aware fill/border drawing
- `dartboard-editor` now owns clipboard stamping and smart-fill glyph selection helpers
- `dartboard-editor` now owns local cut/copy/paste/fill/border command helpers that operate on `EditorSession` + canvas
- `dartboard-editor` now owns floating paint/stamp state transitions (`begin/end`, dismiss, drag/stamp behavior) against session + canvas
- `dartboard-editor` now owns the non-shell key dispatch path for editor behavior (movement, selection growth/clear, alt pan/copy, control editing commands, text insertion/deletion)

What it does not own yet:
- pointer intent routing and floating-specific key/pointer dispatch that still depends on standalone undo grouping
- canvas ownership and undo/redo orchestration
- the broader host-effect policy beyond direct editor clipboard export

### Explicit full-session handshake rejection
- `ServerMsg::ConnectRejected`
- server-side capacity gate
- ws client fails fast on rejected connect

This is primarily a transport/session improvement, but it also makes embedding cleaner because hosts can treat connect failure as an explicit state instead of a timeout or partial session.

## Good direction, but not done

The code is more reusable than before, but `dartboard/src/app.rs` still bundles too many concerns:
- editor state and editor commands
- session transport + peer mirror logic
- standalone app-shell behavior
- crossterm-specific compatibility shims

That means `late-sh` can integrate more cleanly than before, but it still has to choose between:
- depending on a fairly heavyweight `App`
- or continuing to duplicate part of the state/input/session stack

## Next abstractions to add

### 1. Extract a pure editor/session-state crate
Suggested shape:
- `dartboard-editor` or `dartboard-session`

Current status:
- `dartboard-editor` now exists and owns the reusable session model
- the main remaining work is to move the state machine itself out of `dartboard/src/app.rs`

It should own:
- cursor
- viewport
- draw/select mode
- selection state
- floating selection state
- swatch history
- undo/redo policy
- clipboard export helpers
- editor commands that mutate local state and produce canvas ops / host effects

It should not own:
- crossterm event parsing
- terminal cursor shape changes
- standalone help modal chrome
- process-level startup / shutdown

This is the biggest remaining seam.

### 2. Extract a reusable session mirror around `Client`
Suggested shape:
- `SessionMirror`
- `ClientSession`
- `CanvasSession`

It should maintain:
- latest canvas snapshot
- peer list
- `your_user_id`
- `your_color`
- last seen seq
- connect rejection / transport error state

This is the reusable concept currently split across:
- remote handling in standalone `App`
- `late-sh`'s `DartboardService`

The goal is for any host to say:
- "give me a `Client`"
- "I get a mirrored session view plus event stream"

without rewriting snapshot/event plumbing.

### 3. Separate host effects from editor effects more cleanly
`HostEffect` exists now, but is still minimal.

It likely needs to grow into a small, explicit set such as:
- `RequestQuit`
- `CopyToClipboard(String)`
- `SetCursorStyle(...)`
- `ShowNotice(String)`

The rule should be:
- reusable/editor layers emit effects
- host layer decides how to realize them

This is especially important for `late-sh`, which cannot always realize effects the same way as standalone `dartboard`.

### 4. Add a public adapter layer for host input
The crossterm adapter exists now, but the boundary is still incomplete.

Useful additions:
- maybe a tiny "VT-ish" adapter surface for hosts that do their own parsing

The main goal is not to force one parser, but to make the translation boundary explicit and documented.

### 5. Narrow the standalone `App`
Once the above exists, `dartboard::app::App` should become the standalone composition root, not the reusable core.

That composition root would own:
- crossterm event loop
- emoji/help/picker shell glue
- host effect realization
- composition of:
  - session mirror
  - editor state
  - `dartboard-tui`

This would leave embedded hosts free to reuse the lower layers without inheriting standalone assumptions.

## Probably not worth extracting yet

- standalone outer frame / title bar / help panel chrome
- `late-sh`-specific input parser details
- terminal startup / teardown logic
- SSH-specific cursor-shape management

Those should stay host-specific until there is a second concrete consumer that needs the same abstraction.

## Practical target for `late-sh`

The medium-term target should look like:

1. `late-sh` owns its own parser and app shell
2. `late-sh` translates input into `AppIntent`
3. `late-sh` drives a reusable editor/session object
4. `late-sh` renders canvas state with `dartboard-tui`
5. `late-sh` handles returned `HostEffect`s in its own environment

At that point:
- `dartboard` does not depend on `late-sh`
- `late-sh` does not need to fork dartboard logic
- standalone `dartboard` remains just another host built on the same lower layers
