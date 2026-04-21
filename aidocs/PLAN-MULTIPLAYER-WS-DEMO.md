# Multiplayer WS Demo

Add WebSocket transport to dartboard so multiple clients across processes can share one canvas. Builds on the integrated-server foundation from `PLAN-SINGLEPLAYER-INPROC.md`; the wire protocol and `Client` trait already exist there — this plan adds a second `Client` impl and a network listener on the server.

## Prereq
- `PLAN-SINGLEPLAYER-INPROC.md` — required. Establishes the workspace split, wire protocol (`ClientMsg`/`ServerMsg`/`CanvasOp`), `Client` trait, `LocalClient`, and in-proc server. The dartboard binary already runs as integrated client/server before this plan starts.

## Goals
- WebSocket transport for cross-process play
- `dartboard --connect ws://host:port` for joining a remote board
- `dartboard --listen <addr>` for hosting a board with no UI
- Drop multi-user undo entirely (single-user undo from SP plan stays — disabled at runtime when peers > 1; late-sh plan unbinds the key entirely since it's always shared)
- Validate the optimistic-apply path against real network latency

## Crate split (recap)
Established by `PLAN-SINGLEPLAYER-INPROC.md`:
- `dartboard-core` — types + `Client` trait + wire types. Pure, serde only.
- `dartboard-local` — in-proc `ServerHandle` + `LocalClient` + `CanvasStore`.
- `dartboard-server` — websocket listener built on `dartboard-local`. **This plan adds `ServerHandle::bind_ws(addr)`.**
- `dartboard-client-ws` — was a stub; **this plan fills in `WebsocketClient`.**
- `dartboard` — binary. **This plan adds `--connect` and `--listen` flags.**

## Server additions
- `ServerHandle::bind_ws(addr)` accepts incoming ws upgrade requests; for each connection, runs the same per-connection task that `connect_local` already runs. Only the framing differs (serde-json over ws frames vs in-memory `ClientMsg`/`ServerMsg` values on mpsc).
- Same canonical canvas, same op log, same broadcast fan-out as singleplayer.
- Per-connection task: read `ClientMsg`, validate (bounds, glyph allowed, rate-limit), apply, assign global `seq`, ack sender, broadcast to peers.
- Library choice: `tokio-tungstenite` directly, or `axum` with ws upgrade — pick whichever is cleanest for a non-HTTP use. (`axum` may be overkill if there are no HTTP routes.)

## WebsocketClient (in `dartboard-client-ws`)
- Connects to remote `ws://...`
- Sends `Hello`, expects `Welcome` (snapshot + peers + your_user_id)
- Spawns send/receive tasks; impls the `Client` trait identically to `LocalClient`
- Basic reconnect: on disconnect, exponential-backoff reconnect, replay `Hello`, receive new `Welcome` (snapshot replaces local mirror; pending ops re-issued — design call: lean toward re-issue, but document the alternative)

## Multi-user behavior + undo handling
- LWW per cell — server applies ops in receive order, broadcasts to peers
- Drawing is forgiving: a peer painting over your cell is a fine outcome, no rollback gymnastics needed
- Undo gets disabled at runtime when peers > 1. Tracking: client maintains a peer count from `Welcome`/`PeerJoined`/`PeerLeft`; if > 1 (i.e. someone else is connected), `^Z` becomes a no-op (or shows a toast: "undo disabled in multi-user"). Why: the local snapshot stack from SP plan can include cells another peer has painted; "undoing" would clobber their work.

## Phases
1. Add ws transport in `dartboard-server`: `ServerHandle::bind_ws(addr)`. Hand each new connection to the existing per-connection task in `dartboard-local` with a different framing wrapper.
2. Fill in `dartboard-client-ws`: `WebsocketClient` impls `Client`, basic reconnect with backoff.
3. Add `--connect <url>` and `--listen <addr>` flags. Default (no flag) = embedded server + LocalClient (already works from SP plan). `--connect` skips embedded server, uses `WebsocketClient`. `--listen` runs only `bind_ws` (no UI, no client).
4. Add the peers-aware undo gate in the binary's input handlers.
5. Test: `dartboard --listen 127.0.0.1:8080` in one terminal, two `dartboard --connect ws://127.0.0.1:8080` in two others. Verify ops flow, late join gets snapshot, peer presence broadcasts, undo disables on second connect.

## Out of scope (v1)
- Persistence beyond in-memory (file-backed `CanvasStore` is a small follow-up)
- Auth, room management, capacity limits, rate limiting
- Per-user cursors visible to peers (v2 — useful but adds protocol churn)
- Per-user paint attribution rendering across the network (already exists in single-process; needs `from: UserId` on `OpBroadcast` to be plumbed end-to-end)
- Compression / binary encoding
- TLS — assume reverse proxy or local network for v1
