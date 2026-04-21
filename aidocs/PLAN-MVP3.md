# MVP3

Status note: MVP3 is still mostly future work. The multiplayer/reuse direction advanced first; the offline/document-oriented feature set below has not been implemented as a user-facing workflow yet.

## Done

### Non-user-facing groundwork
- The server has a `CanvasStore` persistence boundary.
- The rest of the workspace now has cleaner reuse seams (`dartboard-editor`, `dartboard-tui`, `SessionMirror`, host-neutral input/effects), which lowers the cost of adding offline/document features later.

That groundwork matters, but it is not MVP3 from a user point of view.

## Next

### File/document workflow
The first MVP3 slice should probably be:

1. save canvas to plain text
2. load canvas from plain text
3. define a document identity in the standalone `dartboard-cli` host

This is the minimum step that turns the app into a document-oriented offline tool instead of only an in-memory session.

### Canvas lifecycle
After save/load exists, add explicit local document lifecycle commands:
- new canvas
- clear canvas
- resize canvas

These should be designed as local document actions, not multiplayer protocol features first.

### Offline-oriented UI
Once documents exist, add the corresponding standalone UI state:
- status bar
- cursor coordinates
- dirty state
- current file name or document identity

This should stay lightweight and preserve the directness of the drawing workflow.

## Deferred

### Auto-save
- Useful, but not necessary before basic save/load exists.
- It should wait until there is a clear document model and a better sense of failure/overwrite behavior.

### Richer offline affordances beyond MVP3
- version/history UX
- file browser or recent-documents UI
- generic-editor-style chrome

Those would risk turning the app into a generic text editor instead of a focused drawing tool.

## Current Gap

What is still missing today in the standalone `dartboard-cli` host:
- no save command
- no load command
- no local document identity
- no dirty-state indicator
- no status bar
- no cursor-coordinate display
- no exposed new/clear/resize document workflow

## Constraint

MVP3 should preserve the directness of the drawing workflow rather than turning `dartboard-cli` into a generic text editor.
