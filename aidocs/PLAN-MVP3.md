# MVP3

## Goal
- Add offline/document-oriented features after the multiplayer direction is established.

## Product Shape
- Single-player or offline mode with durable documents and more conventional editor affordances.

## In Scope

### File/document workflow
- Save canvas to plain text.
- Load canvas from plain text.
- Auto-save if it proves useful.

### Canvas lifecycle
- New canvas.
- Clear canvas.
- Resize canvas.

### Offline-oriented UI
- Status bar.
- Cursor coordinates.
- Dirty state.
- Current file name or document identity.

## Notes
- These features are intentionally deferred because they matter more for an offline editor than for the early multiplayer pitch.
- MVP3 should preserve the directness of the drawing workflow rather than turning dartboard into a generic text editor.
