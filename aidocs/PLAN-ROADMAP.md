# Roadmap

## Line drawing tools
- Shift+mouse drag for diagonal lines (Bresenham)
- Character selection for line segments: `/` `\` `|` `-` or box-drawing chars
- Possibly a "line mode" toggle

## Box drawing
- Draw rectangles with box-drawing characters (single/double/rounded)
- Could be a block-selection operation: select region, apply box style

## Networking (shared canvas)
- Canvas data model is already separated for this
- WebSocket or similar for real-time sync
- CRDT or OT for conflict resolution on concurrent edits
- Per-user insertion points and selections (see other users' cursors/selections)
- Room/session management

## Fill tool
- Flood fill from cursor position with a character
- Bounded by non-space characters

## Copy/paste
- Copy the current selection, or the current cell when nothing is selected, to a clipboard buffer
- Paste buffer at cursor position
- System clipboard integration (OSC 52)

## Canvas operations
- Scroll/pan when canvas is larger than viewport
- Resize canvas
- Clear canvas
- Canvas coordinates display in title or status

## File I/O
- Save canvas to file (plain text)
- Load canvas from file
- Auto-save

## Color support
- Per-cell foreground/background colors
- Color picker or palette
- ANSI color codes in export

## Templates / stamps
- Predefined ASCII art snippets
- Paste from a library of common shapes

## Multiple canvases / tabs
- Switch between canvases
- Split view
