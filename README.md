# dartboard

`dartboard` is a terminal drawing board written in Rust.

The repo currently contains:

- `dartboard`: a ratatui/crossterm TUI client
- `dartboardd`: a headless websocket server
- reusable crates for canvas state, editor logic, rendering, and websocket transport

## Current State

What exists in this codebase today:

- local embedded mode with an in-process server and multiple local users
- remote multiplayer over websocket
- terminal UI with keyboard and mouse editing
- reusable crates for embedding the canvas/editor/server pieces elsewhere

What does not exist yet:

- persistent storage beyond the in-memory store
- authentication or access control
- a published network service; you run the server yourself

## Workspace Layout

- `dartboard-core`: canvas model, operations, wire types
- `dartboard-editor`: host-neutral editor/session logic and keymap
- `dartboard-tui`: reusable ratatui canvas widget
- `dartboard-local`: in-process server, local client, store trait
- `dartboard-server`: websocket listener and `dartboardd`, built on `dartboard-local`
- `dartboard-client-ws`: websocket client transport
- `dartboard-cli`: the `dartboard` terminal application

## Build

```bash
cargo build --workspace --all-targets
```

Or use the provided `justfile`:

```bash
just build
just test
just lint
```

## Running

### Embedded TUI demo

Runs the TUI with an in-process server and the built-in local-user demo:

```bash
cargo run --bin dartboard
```

### Host a shared websocket session from the TUI binary

```bash
cargo run --bin dartboard -- --listen 127.0.0.1:9199
```

Then connect another client with:

```bash
cargo run --bin dartboard -- --connect ws://127.0.0.1:9199
```

You can also set identity when connecting:

```bash
cargo run --bin dartboard -- --connect ws://127.0.0.1:9199 --user alice --user-color ff6633
```

### Run the headless server

`dartboardd` serves the same websocket protocol without the TUI dependency footprint:

```bash
cargo run --bin dartboardd -- 127.0.0.1:9199
```

Examples:

```bash
cargo run --bin dartboardd
cargo run --bin dartboardd -- 0.0.0.0
cargo run --bin dartboardd -- 127.0.0.1:0
```

Notes:

- default server address is `127.0.0.1:9199`
- if you pass port `0`, the resolved websocket URL is printed on startup
- the server currently uses `InMemStore`, so canvas state is lost when the process exits

## Controls

The TUI exposes these core controls in its built-in help:

- type to draw
- arrow keys to move
- `Shift` + arrows to create or extend a selection
- mouse drag to select
- right-drag or `Alt` + arrows / `Ctrl` + `Shift` + arrows to pan
- `Ctrl+C`, `Ctrl+X`, `Ctrl+V` for swatch-based copy/cut/paste
- `Alt+C` to export text to the system clipboard
- `Ctrl+Z` / `Ctrl+R` for undo / redo
- `Ctrl+P` to toggle help
- `Ctrl+Q` to quit

Advanced editing features currently implemented include:

- rectangular and elliptical selections
- floating selections with transparency toggle
- swatch slots with pinning
- border drawing
- smart fill
- row and column push/pull transforms
- support for wide Unicode glyphs and per-cell colors

## Protocol / Limits

- websocket messages are JSON-encoded `ClientMsg` / `ServerMsg` values
- default canvas size is `256 x 128`
- the shared server currently allows up to 8 concurrent players

## License

MIT
