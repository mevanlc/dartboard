# Singleplayer In-Proc Refactor

Foundational refactor: dartboard standalone runs as integrated client/server in a single process. UI talks to the canonical canvas via a `Client` trait, not by mutating `App` directly. No network, no second process. This is the substrate for both WS multiplayer and late-sh integration.

## Goals
- 5-crate workspace established (sets the dependency boundary so late-sh never accidentally pulls `tokio-tungstenite`)
- Canvas state owned by `dartboard-local::ServerHandle`, not by the UI's `App`
- UI mutations go through the `dartboard-core::Client` trait (one impl in this plan: `LocalClient`)
- Wire protocol (`ClientMsg`/`ServerMsg`/`CanvasOp`) defined in `dartboard-core` and used even on the LocalClient path — protocol uniformity across transports
- Optimistic local apply on op submission (no observable latency for LocalClient, but the code path matches what WS will need)
- Existing single-user UX unchanged: undo, swatches, emoji picker, multi-local-user switching all still work
- Single-user undo stays — local snapshot stack on the client side, fine because there's no second writer in singleplayer

## Crate split (sets the layout for all subsequent plans)
- `dartboard-core` — `Canvas`, `Pos`, `CanvasOp`, snapshot/diff types, wire types (`ClientMsg`, `ServerMsg`), `Client` trait. Deps: `serde` only. No tokio. No async runtime.
- `dartboard-local` — in-proc `ServerHandle` (owns canonical `Canvas` + op log), `LocalClient` (channel-based, paired with the in-proc server), `CanvasStore` trait. No websocket deps.
- `dartboard-server` — websocket listener and headless `dartboardd`, built on `dartboard-local`.
- `dartboard-client-ws` — `WebsocketClient`. Deps: `tokio`, `tokio-tungstenite`, `dartboard-core`. **Stub crate in this plan** (Cargo.toml + empty `lib.rs`). Reserves the dependency boundary up front. Filled in by `PLAN-MULTIPLAYER-WS-DEMO.md`.
- `dartboard` — binary. UI + bindings. Deps: `dartboard-core`, `dartboard-server`, `ratatui`, `crossterm`. Spawns embedded server + LocalClient on startup.

late-sh will depend on `dartboard-core` + `dartboard-local` only — never on `dartboard-client-ws`. Establishing this layout up front avoids retrofit pain.

## Wire protocol
JSON via serde for v1 (debuggable). Switch to bincode/postcard later if size matters. Used by LocalClient (in-proc channels carrying these typed values, no serialization) and (later) WebsocketClient (same types serialized over ws).

- `ClientMsg`:
  - `Hello { user_id, name, color }`
  - `Op { client_op_id: u64, op: CanvasOp }` — id is client-assigned monotonic
- `ServerMsg`:
  - `Welcome { snapshot: Canvas, peers: Vec<Peer>, your_user_id: UserId }`
  - `Ack { client_op_id: u64, seq: u64 }` — seq is server-assigned global ordering
  - `OpBroadcast { from: UserId, op: CanvasOp, seq: u64 }`
  - `PeerJoined { peer: Peer }` / `PeerLeft { user_id: UserId }`
  - `Reject { client_op_id: u64, reason: String }`
- `CanvasOp`:
  - `PaintCell { pos, glyph, fg }`
  - `ClearCell { pos }`
  - `PaintRegion { origin, cells: Vec<(Pos, Glyph, Color)> }` — for paste/stamp/clear-region

LocalClient and WebsocketClient implement the same `Client` trait over these types. Protocol logic is identical regardless of transport.

## Optimistic local apply
- On user input: client applies op to a local canvas mirror immediately, pushes to outgoing queue with `client_op_id`, tracks in pending-set
- On `Ack`: remove from pending-set
- On `OpBroadcast`: apply to local mirror (LWW)
- On `Reject`: revert pending op locally
- For LocalClient the round-trip is microseconds, so the "optimistic" part is invisible — but the architecture matches what WS will need without changes.

## Server (in-proc API)
- `Server` owns `Canvas` behind `RwLock`, op log behind `Mutex` (op rate is low; contention is fine)
- `Server::spawn_local(store: impl CanvasStore) -> ServerHandle` — spawns the server task, returns a handle
- `ServerHandle::connect_local(hello: Hello) -> LocalClient` — creates a paired client; returns a `LocalClient` that impls the `Client` trait via mpsc channels in both directions
- `CanvasStore` trait + in-mem default impl (file-backed and other impls are follow-ups)

## Refactor steps
1. Convert `dartboard/` root into a Cargo workspace, members = `["dartboard-core", "dartboard-local", "dartboard-server", "dartboard-client-ws", "dartboard"]`. Move existing `src/` into `dartboard/src/`.
2. Carve `dartboard-core`:
   - Move `canvas.rs` → `dartboard-core/src/canvas.rs`; move `Pos`, color types
   - Define `CanvasOp` enum — find every site that mutates `Canvas` in current code, identify the op shape
   - Define wire types (`ClientMsg`, `ServerMsg`, `Peer`)
   - Define `Client` trait: `submit_op`, `next_event`, `current_snapshot` (or similar)
3. Build `dartboard-local`: `spawn_local`, `ServerHandle::connect_local`, `LocalClient` impl, `CanvasStore` trait + in-mem default
4. Create `dartboard-client-ws` as a stub crate (Cargo.toml + empty `lib.rs`). Reserves the boundary; the WS plan fills it in.
5. Refactor `dartboard` binary:
  - Startup: `let server = ServerHandle::spawn_local(InMemStore); let client = server.connect_local(Hello { ... });`
   - `App` no longer holds `Canvas` directly. Holds a snapshot mirror (updated from client events) + per-session UI state (viewport, cursor, swatches, etc.)
   - Every input handler that previously mutated canvas now: (a) applies op to the mirror immediately, (b) calls `client.submit_op(op)`. Round-trip is sub-millisecond.
6. Verify: drawing, selection, paste, swatches, emoji picker, undo/redo all work as before. Undo operates on the local mirror snapshot stack.

## Open question
- The current binary has 5 hardcoded local-user "sessions" you switch between with a key. After refactor, do they all share one `LocalClient` or get one each (each connecting to the same in-proc server)? Probably one each — that way multi-local-user switching naturally exercises the multi-client server path before real WS multiplayer arrives. Concrete impact: each "switch user" key changes which `LocalClient` is the active input target, but all clients talk to the same in-proc server.

## Out of scope (this plan)
- Network transport (`PLAN-MULTIPLAYER-WS-DEMO.md`)
- Multiple processes
- Persistence beyond in-memory
- late-sh integration (`PLAN-MULTIPLAYER-LATE-SH.md`)
