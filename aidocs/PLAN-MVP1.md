# MVP1

## Goal
- Deliver an offline demo of dartboard that shows the core drawing experience well enough to pitch it to a BBS owner.
- No persistence.
- No real networking yet.
- UI can include multiplayer-shaped placeholders if they help communicate the direction.

## Audience
- A BBS operator evaluating whether dartboard is compelling enough to integrate.

## Product Shape
- Single local user.
- Ephemeral session.
- Strong mouse and keyboard drawing workflow.
- Enough UI hints to make the future multiplayer direction obvious.

## In Scope

### Canvas navigation
- Panning via right-click drag.
- Keyboard panning via `Meta+arrows`.
- Keep `^W ^A ^S ^D` available as a likely fallback if `Meta+arrows` proves unreliable in practice.

### Selection ergonomics
- `Shift+click` extends the current selection, but only when a selection already exists.
- Preserve unmodified shift+click for possible future use when there is no active selection.

### Presence and multiplayer-shaped UI
- Right-side user list column as a placeholder.
- Remote presence rendering primitives.
- Mock/fake users and presence data are acceptable in MVP1.
- The point is to preview the multiplayer shape, not to ship the network stack yet.

### Export / clipboard
- OSC 52 copy on `Alt+C`.
- When a selection exists, copy the selection.
- When no selection exists, copy the full canvas.

### Drawing tools
- Flood fill on `Ctrl+F`.
- Fill uses the character under the cursor.

## Explicitly Out Of Scope
- Real client/server transport.
- Backend relay.
- Save/load.
- Canvas lifecycle features such as new, clear, resize.
- Offline document/status UX.

## Notes
- MVP1 is a demo milestone, not the first logically complete product slice.
- Some UI and behavior may intentionally anticipate multiplayer even though the build is offline.

## Suggested Implementation Order
1. Panning.
2. Shift+click extend existing selection.
3. Presence rendering primitives.
4. Right-side user list placeholder.
5. Flood fill with `Ctrl+F`.
6. OSC 52 copy on `Alt+C`.

