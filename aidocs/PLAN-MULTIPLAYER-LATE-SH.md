# late-sh Dartboard Integration

Integrate dartboard into late-sh as a shared multi-user activity in the Games arcade. Each SSH session is one dartboard client; one in-proc `dartboard-local` server runs for the lifetime of the late-sh process and owns the canonical canvas everyone draws on.

## Prereqs
- `PLAN-SINGLEPLAYER-INPROC.md` — **required**. Establishes the workspace split, wire protocol, `Client` trait, `LocalClient`, and in-proc server. Late-sh's integration is essentially "host one shared `ServerHandle`, give each SSH session a `LocalClient`."
- `PLAN-MULTIPLAYER-WS-DEMO.md` — **not required**. Late-sh only uses `LocalClient` (no `dartboard-client-ws` dep). The WS plan is useful as a validation step that the multi-client paths through the server work before adding real users via late-sh, but it isn't a blocker.

## Workspace integration
- Add `dartboard-core` and `dartboard-local` as deps in late-sh root `Cargo.toml` (path or git initially; published crates eventually)
- Late-sh does **not** depend on `dartboard-client-ws`. That's the whole point of the upfront crate split — no `tokio-tungstenite` or related transitive deps in the late-sh binary.
- `LocalClient` (from `dartboard-local`) is the only client impl late-sh uses.

## Shared server
- One `dartboard_local::ServerHandle` instance, spawned at late-sh startup (likely in `late-ssh/src/main.rs` next to other shared service init)
- Held in `Arc<ServerHandle>`; threaded into `SessionConfig` (see `late-ssh/src/ssh.rs:633-688`)
- `CanvasStore` impl for v1: in-memory. v2: postgres-backed via `late-core` (snapshot table + op log table; periodic snapshot, replay log on boot). v2 is its own plan, don't block on it.

## Per-session module layout
Mirrors the established late-sh game pattern (sudoku, minesweeper, etc.):

`late-ssh/src/app/games/dartboard/`
- `mod.rs` — re-exports
- `svc.rs` — `DartboardService`. Owns a `LocalClient` connected to the shared server. Spawns one task that reads server messages and pushes:
  - latest canvas snapshot into a `tokio::sync::watch::Sender<CanvasSnapshot>`
  - peer events / acks / rejects into a `tokio::sync::broadcast::Sender<PeerEvent>`
  - Public methods are fire-and-forget op submitters: `submit_op_task(op)` spawns and returns immediately
- `state.rs` — `State`. Per-session UI state: viewport, cursor, mode (Draw/Select), active swatches, selection anchor, floating selection, emoji picker open/closed, last-known peer list. Drains watch + broadcast each `tick()`. **No undo stack.**
- `input.rs` — Keypress → state mutation + svc op submission. `^Z`/`^Shift+Z` are unbound. Otherwise mirrors dartboard's standalone bindings (mode toggle, navigation, paint, selection, paste, emoji picker).
- `ui.rs` — Pure ratatui draw. Reads `State` + latest snapshot. Reuses canvas widget from dartboard (see Rendering below).

## Wiring (mirrors sudoku)
- Add `pub mod dartboard` to `late-ssh/src/app/games/mod.rs`
- Instantiate `DartboardService` in `SessionConfig` builder (`ssh.rs:633-688`); pass shared `Arc<ServerHandle>` in
- Add `dartboard_state: dartboard::State` to `App` (`late-ssh/src/app/state.rs:185-194`)
- Add dartboard to the arcade selector list and `game_selection` index handling
- Wire input routing in `app/input.rs` when `Screen::Games` + dartboard is the active game
- Wire render dispatch in `app/render.rs`

## Sync/async invariants (per `CONTRIBUTING.md:74-92`)
- `svc.rs` is the only file allowed to `.await`
- `state.rs`, `input.rs`, `ui.rs` are pure sync
- Op submission: `state.svc.submit_op_task(op)` spawns; the optimistic local apply happens synchronously in `state.rs` before the task is spawned
- Snapshot drained each tick (~66ms): server-side broadcasts visible within one frame for in-proc clients

## Rendering
Two options, lean toward A:

- **A. Reusable widget from dartboard** — extract `CanvasWidget` from `dartboard/src/ui.rs` into either `dartboard-core` (if pure) or a new `dartboard-tui` crate. Both standalone and late-sh use it. Forces a clean read-only widget API, which is good hygiene.
- **B. Fork into `late-ssh/src/app/games/dartboard/ui.rs`** and let it diverge. Faster initially but the two will drift.

Defer the choice until the standalone refactor (`PLAN-SINGLEPLAYER-INPROC.md` step 5) stabilizes the widget API.

## Input bindings — late-sh-specific
- `^Z` / `^Shift+Z` — unbound (no undo)
- All other dartboard bindings carry over
- Reserve a key for "leave dartboard" / back to arcade per late-sh convention (probably `Esc` or a chord; check existing games for the pattern)

## Tests
Mirror `late-ssh/tests/games/sudoku.rs`:
- Setup test server + two service instances (simulating two SSH sessions)
- Session A submits a paint op
- Tick session B; assert the op appears in B's snapshot
- Test rejects (out-of-bounds), test peer-join broadcast

## Out of scope (v1)
- Persistence across late-sh process restarts (in-mem only)
- Per-user cursors visible to other players (peer presence is in scope; cursor positions are not)
- Rate limiting / anti-grief tooling
- Postgres-backed `CanvasStore` impl (v2)
- Web frontend (`late-web`) viewer for the canvas (would reuse `dartboard-core` snapshot but render to HTML)

## Open questions
- Where does the `Server` instance live in code? Top-level service alongside chat/votes? Inside `late-core` so `late-web` could later host its own viewer?
- Single shared canvas, or one canvas per "room"? v1 = single shared. Rooms is v2+.
- How does dartboard's `UserId` map to late-sh's user id? Direct passthrough (late-sh user → dartboard user) is simplest.
