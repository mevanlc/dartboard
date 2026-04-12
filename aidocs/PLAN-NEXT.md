# Next Up

## Shift+click to extend selection (simple mode)
- Shift+click sets the far end of a block selection from the current anchor/cursor
- Standard GUI convention, natural complement to shift+arrows

## ^J in replace mode
- Decide behavior: probably same as insert mode ^J (column push below cursor, cursor rides down)
- Keeps ^J consistent across modes as a canvas manipulation shortcut

## Start in simple mode by default
- Flip `simple_mode: false` to `true` in `App::new()`
- Vi mode is the power-user escape hatch via ^G

## Undo/redo
- Canvas-level undo stack (snapshots or operation log)
- `u` / `^R` in vi mode
- `^Z` / `^Shift+Z` in simple mode
