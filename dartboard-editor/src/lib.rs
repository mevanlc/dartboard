//! # dartboard-editor
//!
//! Host-neutral editor state and dispatch for the dartboard terminal drawing
//! tool. This crate has no terminal-I/O dependencies — hosts construct
//! [`AppIntent`] values from their own input layer and feed them to the
//! editor.
//!
//! ## Host wiring
//!
//! 1. Own an [`EditorSession`] and a [`dartboard_core::Canvas`].
//! 2. Translate your host's input events into [`AppKey`] and
//!    [`AppPointerEvent`]. Pointer coordinates are 0-based cell positions
//!    in the host's display grid; the host is responsible for any terminal
//!    protocol conversion (e.g., SGR 1-based → 0-based).
//! 3. Route key input through [`KeyMap::resolve`] plus [`handle_editor_action`]
//!    (or [`handle_editor_key_press`] for the default keymap path).
//! 4. Route pointer input through [`handle_editor_pointer`]; if your host
//!    has overlay UI (swatches, menus), hit-test those first and only
//!    forward events that should reach the editor. Use
//!    [`EditorSession::viewport_contains`] for canvas hit-testing.
//! 5. Inspect [`EditorPointerDispatch::outcome`]:
//!    [`PointerOutcome::Consumed`] means suppress outer UI;
//!    [`PointerOutcome::Passthrough`] means the editor did not act on the
//!    event and the host may bubble it to outer layers.
//!
//! ## Default pointer policy
//!
//! [`handle_editor_pointer`] implements the hover policy most hosts want:
//! passive [`AppPointerKind::Moved`] events over the canvas do **not**
//! move the caret when no floating preview is armed, and **do** follow
//! the cursor when one is (so brush/stamp previews track the pointer).
//! Layered hosts should simply forward every [`AppPointerEvent`] they
//! want the editor to see and rely on [`PointerOutcome`] to decide
//! whether to bubble.
//!
//! Crossterm adapters for the reference CLI live in the `dartboard-cli`
//! crate. Non-crossterm hosts (e.g., VTE-based shells) construct
//! [`AppIntent`] values directly from their own parsed events.

use std::collections::HashSet;

use dartboard_core::{ops::CellWrite, Canvas, CanvasOp, CellValue, Pos, RgbColor};

pub mod keymap;
pub mod session_mirror;

pub use keymap::{
    ActionSpec, BindingContext, EditorContext, HelpEntry, HelpSection, KeyBinding, KeyMap,
    KeyTrigger,
};
pub use session_mirror::{ConnectState, MirrorEvent, SessionMirror};

pub const SWATCH_CAPACITY: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Viewport {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl AppModifiers {
    pub fn has_alt_like(self) -> bool {
        self.alt || self.meta
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Esc,
    F(u8),
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppKey {
    pub code: AppKeyCode,
    pub modifiers: AppModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPointerKind {
    Down(AppPointerButton),
    Up(AppPointerButton),
    Drag(AppPointerButton),
    Moved,
    ScrollUp,
    ScrollDown,
}

/// A pointer event in the host's display grid.
///
/// `column` and `row` are 0-based cell coordinates; hosts that receive
/// 1-based terminal coordinates (e.g., SGR mouse reports) must normalize
/// before constructing this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppPointerEvent {
    pub column: u16,
    pub row: u16,
    pub kind: AppPointerKind,
    pub modifiers: AppModifiers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIntent {
    KeyPress(AppKey),
    Pointer(AppPointerEvent),
    Paste(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEffect {
    RequestQuit,
    CopyToClipboard(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditorKeyDispatch {
    pub handled: bool,
    pub effects: Vec<HostEffect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDir {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorAction {
    Move {
        dir: MoveDir,
        extend_selection: bool,
    },
    MoveDownLine,
    Pan {
        dx: isize,
        dy: isize,
    },
    ClearSelection,
    TransposeSelectionCorner,

    PushLeft,
    PushRight,
    PushUp,
    PushDown,
    PullFromLeft,
    PullFromRight,
    PullFromUp,
    PullFromDown,

    CopySelection,
    CutSelection,
    PastePrimarySwatch,
    ExportSystemClipboard,
    ActivateSwatch(usize),

    SmartFill,
    DrawBorder,
    FillSelectionOrCell(char),

    InsertChar(char),
    Backspace,
    Delete,

    ToggleFloatingTransparency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Draw,
    Select,
}

impl Mode {
    pub fn is_selecting(self) -> bool {
        matches!(self, Mode::Select)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionShape {
    #[default]
    Rect,
    Ellipse,
}

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
    pub shape: SelectionShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min_x: usize,
    pub max_x: usize,
    pub min_y: usize,
    pub max_y: usize,
}

impl Bounds {
    pub fn from_points(a: Pos, b: Pos) -> Self {
        Self {
            min_x: a.x.min(b.x),
            max_x: a.x.max(b.x),
            min_y: a.y.min(b.y),
            max_y: a.y.max(b.y),
        }
    }

    pub fn single(pos: Pos) -> Self {
        Self::from_points(pos, pos)
    }

    pub fn width(self) -> usize {
        self.max_x - self.min_x + 1
    }

    pub fn height(self) -> usize {
        self.max_y - self.min_y + 1
    }

    pub fn normalized_for_canvas(self, canvas: &Canvas) -> Self {
        let mut bounds = self;
        for y in self.min_y..=self.max_y {
            if bounds.min_x > 0 && canvas.is_continuation(Pos { x: bounds.min_x, y }) {
                bounds.min_x -= 1;
            }
            if matches!(
                canvas.cell(Pos { x: bounds.max_x, y }),
                Some(CellValue::Wide(_))
            ) && bounds.max_x + 1 < canvas.width
            {
                bounds.max_x += 1;
            }
        }
        bounds
    }
}

impl Selection {
    pub fn bounds(self) -> Bounds {
        Bounds::from_points(self.anchor, self.cursor)
    }

    pub fn contains(self, pos: Pos) -> bool {
        let bounds = self.bounds();
        if pos.x < bounds.min_x
            || pos.x > bounds.max_x
            || pos.y < bounds.min_y
            || pos.y > bounds.max_y
        {
            return false;
        }

        match self.shape {
            SelectionShape::Rect => true,
            SelectionShape::Ellipse => {
                if bounds.width() <= 1 || bounds.height() <= 1 {
                    return true;
                }

                let px = pos.x as f64 + 0.5;
                let py = pos.y as f64 + 0.5;
                let cx = (bounds.min_x + bounds.max_x + 1) as f64 / 2.0;
                let cy = (bounds.min_y + bounds.max_y + 1) as f64 / 2.0;
                let rx = bounds.width() as f64 / 2.0;
                let ry = bounds.height() as f64 / 2.0;
                let dx = (px - cx) / rx;
                let dy = (py - cy) / ry;
                dx * dx + dy * dy <= 1.0
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clipboard {
    pub width: usize,
    pub height: usize,
    cells: Vec<Option<CellValue>>,
}

impl Clipboard {
    pub fn new(width: usize, height: usize, cells: Vec<Option<CellValue>>) -> Self {
        Self {
            width,
            height,
            cells,
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<CellValue> {
        self.cells[y * self.width + x]
    }

    pub fn cells(&self) -> &[Option<CellValue>] {
        &self.cells
    }
}

#[derive(Debug, Clone)]
pub struct Swatch {
    pub clipboard: Clipboard,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct FloatingSelection {
    pub clipboard: Clipboard,
    pub transparent: bool,
    pub source_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwatchActivation {
    Ignored,
    ToggledTransparency,
    ActivatedFloating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanDrag {
    pub col: u16,
    pub row: u16,
    pub origin: Pos,
}

#[derive(Debug, Clone)]
pub struct EditorSession {
    pub cursor: Pos,
    pub mode: Mode,
    pub viewport: Viewport,
    pub viewport_origin: Pos,
    pub selection_anchor: Option<Pos>,
    pub selection_shape: SelectionShape,
    pub drag_origin: Option<Pos>,
    pub pan_drag: Option<PanDrag>,
    pub swatches: [Option<Swatch>; SWATCH_CAPACITY],
    pub floating: Option<FloatingSelection>,
    pub paint_stroke_anchor: Option<Pos>,
    pub paint_stroke_last: Option<Pos>,
}

impl Default for EditorSession {
    fn default() -> Self {
        Self {
            cursor: Pos { x: 0, y: 0 },
            mode: Mode::Draw,
            viewport: Viewport::default(),
            viewport_origin: Pos { x: 0, y: 0 },
            selection_anchor: None,
            selection_shape: SelectionShape::Rect,
            drag_origin: None,
            pan_drag: None,
            swatches: Default::default(),
            floating: None,
            paint_stroke_anchor: None,
            paint_stroke_last: None,
        }
    }
}

impl EditorSession {
    pub fn selection(&self) -> Option<Selection> {
        self.selection_anchor.map(|anchor| Selection {
            anchor,
            cursor: self.cursor,
            shape: self.selection_shape,
        })
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_shape = SelectionShape::Rect;
        self.mode = Mode::Draw;
    }

    pub fn begin_selection_with_shape(&mut self, shape: SelectionShape) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.selection_shape = shape;
        self.mode = Mode::Select;
    }

    pub fn begin_selection(&mut self) {
        self.begin_selection_with_shape(SelectionShape::Rect);
    }

    pub fn visible_bounds(&self, canvas: &Canvas) -> Bounds {
        if self.viewport.width == 0 || self.viewport.height == 0 {
            return self.full_canvas_bounds(canvas);
        }

        let min_x = self.viewport_origin.x.min(canvas.width.saturating_sub(1));
        let min_y = self.viewport_origin.y.min(canvas.height.saturating_sub(1));
        let max_x = (self.viewport_origin.x + self.viewport.width.saturating_sub(1) as usize)
            .min(canvas.width.saturating_sub(1));
        let max_y = (self.viewport_origin.y + self.viewport.height.saturating_sub(1) as usize)
            .min(canvas.height.saturating_sub(1));

        Bounds {
            min_x,
            max_x,
            min_y,
            max_y,
        }
    }

    pub fn clamp_cursor_to_visible_bounds(&mut self, canvas: &Canvas) {
        let bounds = self.visible_bounds(canvas);
        self.cursor.x = self.cursor.x.clamp(bounds.min_x, bounds.max_x);
        self.cursor.y = self.cursor.y.clamp(bounds.min_y, bounds.max_y);
    }

    pub fn move_left(&mut self, canvas: &Canvas) {
        if self.cursor.x == 0 {
            return;
        }
        self.cursor.x -= 1;
        self.scroll_viewport_to_cursor(canvas);
    }

    pub fn move_right(&mut self, canvas: &Canvas) {
        if self.cursor.x + 1 >= canvas.width {
            return;
        }
        self.cursor.x += 1;
        self.scroll_viewport_to_cursor(canvas);
    }

    pub fn move_up(&mut self, canvas: &Canvas) {
        if self.cursor.y == 0 {
            return;
        }
        self.cursor.y -= 1;
        self.scroll_viewport_to_cursor(canvas);
    }

    pub fn move_down(&mut self, canvas: &Canvas) {
        if self.cursor.y + 1 >= canvas.height {
            return;
        }
        self.cursor.y += 1;
        self.scroll_viewport_to_cursor(canvas);
    }

    pub fn scroll_viewport_to_cursor(&mut self, canvas: &Canvas) {
        let bounds = self.visible_bounds(canvas);
        if self.cursor.x < bounds.min_x {
            self.viewport_origin.x -= bounds.min_x - self.cursor.x;
        } else if self.cursor.x > bounds.max_x {
            self.viewport_origin.x += self.cursor.x - bounds.max_x;
        }
        if self.cursor.y < bounds.min_y {
            self.viewport_origin.y -= bounds.min_y - self.cursor.y;
        } else if self.cursor.y > bounds.max_y {
            self.viewport_origin.y += self.cursor.y - bounds.max_y;
        }
        self.clamp_viewport_origin(canvas);
    }

    pub fn clamp_viewport_origin(&mut self, canvas: &Canvas) {
        let max_x = canvas
            .width
            .saturating_sub(self.viewport.width.max(1) as usize);
        let max_y = canvas
            .height
            .saturating_sub(self.viewport.height.max(1) as usize);
        self.viewport_origin.x = self.viewport_origin.x.min(max_x);
        self.viewport_origin.y = self.viewport_origin.y.min(max_y);
    }

    pub fn set_viewport(&mut self, viewport: Viewport, canvas: &Canvas) {
        self.viewport = viewport;
        self.clamp_viewport_origin(canvas);
        self.clamp_cursor_to_visible_bounds(canvas);
    }

    pub fn pan_by(&mut self, canvas: &Canvas, dx: isize, dy: isize) {
        self.viewport_origin.x = self.viewport_origin.x.saturating_add_signed(dx);
        self.viewport_origin.y = self.viewport_origin.y.saturating_add_signed(dy);
        self.clamp_viewport_origin(canvas);
        self.clamp_cursor_to_visible_bounds(canvas);
    }

    pub fn begin_pan(&mut self, col: u16, row: u16) {
        self.pan_drag = Some(PanDrag {
            col,
            row,
            origin: self.viewport_origin,
        });
    }

    pub fn drag_pan(&mut self, canvas: &Canvas, col: u16, row: u16) {
        let Some(pan_drag) = self.pan_drag else {
            return;
        };
        let dx = pan_drag.col as i32 - col as i32;
        let dy = pan_drag.row as i32 - row as i32;
        self.viewport_origin.x = pan_drag.origin.x.saturating_add_signed(dx as isize);
        self.viewport_origin.y = pan_drag.origin.y.saturating_add_signed(dy as isize);
        self.clamp_viewport_origin(canvas);
        self.clamp_cursor_to_visible_bounds(canvas);
    }

    pub fn end_pan(&mut self) {
        self.pan_drag = None;
    }

    pub fn viewport_contains(&self, col: u16, row: u16) -> bool {
        let col = col as usize;
        let row = row as usize;
        let vx = self.viewport.x as usize;
        let vy = self.viewport.y as usize;
        let vw = self.viewport.width as usize;
        let vh = self.viewport.height as usize;
        col >= vx && row >= vy && col < vx + vw && row < vy + vh
    }

    pub fn canvas_pos_for_pointer(&self, col: u16, row: u16, canvas: &Canvas) -> Option<Pos> {
        if !self.viewport_contains(col, row) {
            return None;
        }
        let col = col as usize;
        let row = row as usize;
        let vx = self.viewport.x as usize;
        let vy = self.viewport.y as usize;
        let cx = self.viewport_origin.x + col - vx;
        let cy = self.viewport_origin.y + row - vy;
        if cx < canvas.width && cy < canvas.height {
            Some(Pos { x: cx, y: cy })
        } else {
            None
        }
    }

    pub fn clamp_cursor(&mut self, canvas: &Canvas) {
        self.cursor.x = self.cursor.x.min(canvas.width.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(canvas.height.saturating_sub(1));
        self.clamp_cursor_to_visible_bounds(canvas);
    }

    pub fn selection_bounds(&self) -> Option<Bounds> {
        self.selection().map(Selection::bounds)
    }

    pub fn selection_or_cursor_bounds(&self) -> Bounds {
        self.selection_bounds()
            .unwrap_or_else(|| Bounds::single(self.cursor))
    }

    pub fn full_canvas_bounds(&self, canvas: &Canvas) -> Bounds {
        Bounds {
            min_x: 0,
            max_x: canvas.width.saturating_sub(1),
            min_y: 0,
            max_y: canvas.height.saturating_sub(1),
        }
    }

    pub fn system_clipboard_bounds(&self, canvas: &Canvas) -> Bounds {
        self.selection_bounds()
            .unwrap_or_else(|| self.full_canvas_bounds(canvas))
            .normalized_for_canvas(canvas)
    }

    pub fn push_swatch(&mut self, clipboard: Clipboard) {
        let unpinned_slots: Vec<usize> = (0..SWATCH_CAPACITY)
            .filter(|&i| !matches!(&self.swatches[i], Some(swatch) if swatch.pinned))
            .collect();
        if unpinned_slots.is_empty() {
            return;
        }

        let mut queue: Vec<Swatch> = unpinned_slots
            .iter()
            .filter_map(|&i| self.swatches[i].take())
            .collect();
        queue.insert(
            0,
            Swatch {
                clipboard,
                pinned: false,
            },
        );
        queue.truncate(unpinned_slots.len());

        for (slot_idx, swatch) in unpinned_slots.iter().zip(queue.into_iter()) {
            self.swatches[*slot_idx] = Some(swatch);
        }
    }

    #[cfg(test)]
    pub fn populated_swatch_count(&self) -> usize {
        self.swatches
            .iter()
            .filter(|swatch| swatch.is_some())
            .count()
    }

    pub fn toggle_pin(&mut self, idx: usize) {
        if idx >= SWATCH_CAPACITY {
            return;
        }
        if let Some(swatch) = self.swatches[idx].as_mut() {
            swatch.pinned = !swatch.pinned;
        }
    }

    pub fn activate_swatch(&mut self, idx: usize) -> SwatchActivation {
        if idx >= SWATCH_CAPACITY {
            return SwatchActivation::Ignored;
        }
        let Some(swatch) = self.swatches[idx].as_ref() else {
            return SwatchActivation::Ignored;
        };
        match self.floating.as_mut() {
            Some(floating) if floating.source_index == Some(idx) => {
                floating.transparent = !floating.transparent;
                SwatchActivation::ToggledTransparency
            }
            _ => {
                self.floating = Some(FloatingSelection {
                    clipboard: swatch.clipboard.clone(),
                    transparent: false,
                    source_index: Some(idx),
                });
                self.clear_selection();
                SwatchActivation::ActivatedFloating
            }
        }
    }

    pub fn toggle_float_transparency(&mut self) {
        if let Some(floating) = self.floating.as_mut() {
            floating.transparent = !floating.transparent;
        }
    }

    pub fn floating_brush_width(&self) -> usize {
        self.floating
            .as_ref()
            .map(|floating| floating.clipboard.width.max(1))
            .unwrap_or(1)
    }
}

pub fn diff_canvas_op(before: &Canvas, after: &Canvas, default_fg: RgbColor) -> Option<CanvasOp> {
    let mut origins: HashSet<Pos> = HashSet::new();
    for (pos, cell) in before.iter() {
        if matches!(cell, CellValue::Narrow(_) | CellValue::Wide(_)) {
            origins.insert(*pos);
        }
    }
    for (pos, cell) in after.iter() {
        if matches!(cell, CellValue::Narrow(_) | CellValue::Wide(_)) {
            origins.insert(*pos);
        }
    }

    let mut origins: Vec<Pos> = origins.into_iter().collect();
    origins.sort_by_key(|p| (p.y, p.x));

    let mut writes: Vec<CellWrite> = Vec::new();
    for pos in origins {
        let a_cell = after.cell(pos);
        let b_cell = before.cell(pos);
        let a_fg = after.fg(pos);
        let b_fg = before.fg(pos);
        if a_cell == b_cell && a_fg == b_fg {
            continue;
        }
        match a_cell {
            Some(CellValue::Narrow(ch)) | Some(CellValue::Wide(ch)) => {
                writes.push(CellWrite::Paint {
                    pos,
                    ch,
                    fg: a_fg.unwrap_or(default_fg),
                });
            }
            _ => writes.push(CellWrite::Clear { pos }),
        }
    }

    match writes.len() {
        0 => None,
        1 => Some(match writes.remove(0) {
            CellWrite::Paint { pos, ch, fg } => CanvasOp::PaintCell { pos, ch, fg },
            CellWrite::Clear { pos } => CanvasOp::ClearCell { pos },
        }),
        _ => Some(CanvasOp::PaintRegion { cells: writes }),
    }
}

pub fn fill_bounds(canvas: &mut Canvas, bounds: Bounds, ch: char, fg: RgbColor) {
    for y in bounds.min_y..=bounds.max_y {
        let mut x = bounds.min_x;
        while x <= bounds.max_x {
            if ch == ' ' {
                canvas.clear(Pos { x, y });
                x += 1;
                continue;
            }

            let width = Canvas::display_width(ch);
            if width == 2 && x == bounds.max_x {
                break;
            }
            let _ = canvas.put_glyph_colored(Pos { x, y }, ch, fg);
            x += width;
        }
    }
}

pub fn fill_selection(
    canvas: &mut Canvas,
    selection: Selection,
    bounds: Bounds,
    ch: char,
    fg: RgbColor,
) {
    if selection.shape == SelectionShape::Rect {
        fill_bounds(canvas, bounds, ch, fg);
        return;
    }

    let glyph_width = Canvas::display_width(ch);
    for y in bounds.min_y..=bounds.max_y {
        let mut x = bounds.min_x;
        while x <= bounds.max_x {
            let pos = Pos { x, y };
            if !selection.contains(pos) {
                x += 1;
                continue;
            }

            if ch == ' ' {
                canvas.clear(pos);
                x += 1;
                continue;
            }

            if glyph_width == 1 {
                canvas.set_colored(pos, ch, fg);
                x += 1;
                continue;
            }

            if x < bounds.max_x && selection.contains(Pos { x: x + 1, y }) {
                let _ = canvas.put_glyph_colored(pos, ch, fg);
                x += glyph_width;
            } else {
                x += 1;
            }
        }
    }
}

fn selection_has_unselected_neighbor(selection: Selection, pos: Pos) -> bool {
    let neighbors = [
        pos.x.checked_sub(1).map(|x| Pos { x, y: pos.y }),
        Some(Pos {
            x: pos.x + 1,
            y: pos.y,
        }),
        pos.y.checked_sub(1).map(|y| Pos { x: pos.x, y }),
        Some(Pos {
            x: pos.x,
            y: pos.y + 1,
        }),
    ];
    neighbors
        .into_iter()
        .flatten()
        .any(|neighbor| !selection.contains(neighbor))
}

pub fn draw_border(canvas: &mut Canvas, selection: Selection, color: RgbColor) {
    let bounds = selection.bounds();
    if selection.shape == SelectionShape::Ellipse {
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                let pos = Pos { x, y };
                if selection.contains(pos) && selection_has_unselected_neighbor(selection, pos) {
                    canvas.set_colored(pos, '*', color);
                }
            }
        }
        return;
    }

    if bounds.width() == 1 && bounds.height() == 1 {
        canvas.set_colored(
            Pos {
                x: bounds.min_x,
                y: bounds.min_y,
            },
            '*',
            color,
        );
        return;
    }

    if bounds.height() == 1 {
        canvas.set_colored(
            Pos {
                x: bounds.min_x,
                y: bounds.min_y,
            },
            '.',
            color,
        );
        for x in (bounds.min_x + 1)..bounds.max_x {
            canvas.set_colored(Pos { x, y: bounds.min_y }, '-', color);
        }
        canvas.set_colored(
            Pos {
                x: bounds.max_x,
                y: bounds.min_y,
            },
            '.',
            color,
        );
        return;
    }

    if bounds.width() == 1 {
        canvas.set_colored(
            Pos {
                x: bounds.min_x,
                y: bounds.min_y,
            },
            '.',
            color,
        );
        for y in (bounds.min_y + 1)..bounds.max_y {
            canvas.set_colored(Pos { x: bounds.min_x, y }, '|', color);
        }
        canvas.set_colored(
            Pos {
                x: bounds.min_x,
                y: bounds.max_y,
            },
            '`',
            color,
        );
        return;
    }

    canvas.set_colored(
        Pos {
            x: bounds.min_x,
            y: bounds.min_y,
        },
        '.',
        color,
    );
    canvas.set_colored(
        Pos {
            x: bounds.max_x,
            y: bounds.min_y,
        },
        '.',
        color,
    );
    canvas.set_colored(
        Pos {
            x: bounds.min_x,
            y: bounds.max_y,
        },
        '`',
        color,
    );
    canvas.set_colored(
        Pos {
            x: bounds.max_x,
            y: bounds.max_y,
        },
        '\'',
        color,
    );

    for x in (bounds.min_x + 1)..bounds.max_x {
        canvas.set_colored(Pos { x, y: bounds.min_y }, '-', color);
        canvas.set_colored(Pos { x, y: bounds.max_y }, '-', color);
    }

    for y in (bounds.min_y + 1)..bounds.max_y {
        canvas.set_colored(Pos { x: bounds.min_x, y }, '|', color);
        canvas.set_colored(Pos { x: bounds.max_x, y }, '|', color);
    }
}

pub fn capture_bounds(canvas: &Canvas, bounds: Bounds) -> Clipboard {
    let mut cells = Vec::with_capacity(bounds.width() * bounds.height());
    for y in bounds.min_y..=bounds.max_y {
        for x in bounds.min_x..=bounds.max_x {
            cells.push(canvas.cell(Pos { x, y }));
        }
    }
    Clipboard::new(bounds.width(), bounds.height(), cells)
}

pub fn capture_selection(canvas: &Canvas, selection: Selection) -> Clipboard {
    let bounds = selection.bounds().normalized_for_canvas(canvas);
    let mut cells = Vec::with_capacity(bounds.width() * bounds.height());
    for y in bounds.min_y..=bounds.max_y {
        for x in bounds.min_x..=bounds.max_x {
            let pos = Pos { x, y };
            let include = selection.contains(pos)
                || canvas
                    .glyph_origin(pos)
                    .is_some_and(|origin| selection.contains(origin));
            cells.push(include.then(|| canvas.cell(pos)).flatten());
        }
    }
    Clipboard::new(bounds.width(), bounds.height(), cells)
}

pub fn export_bounds_as_text(canvas: &Canvas, bounds: Bounds) -> String {
    let mut text = String::with_capacity(bounds.width() * bounds.height() + bounds.height());
    for y in bounds.min_y..=bounds.max_y {
        for x in bounds.min_x..=bounds.max_x {
            match canvas.cell(Pos { x, y }) {
                Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => text.push(ch),
                Some(CellValue::WideCont) => {}
                None => text.push(' '),
            }
        }
        if y != bounds.max_y {
            text.push('\n');
        }
    }
    text
}

pub fn export_selection_as_text(canvas: &Canvas, selection: Selection) -> String {
    let bounds = selection.bounds().normalized_for_canvas(canvas);
    let mut text = String::with_capacity(bounds.width() * bounds.height() + bounds.height());
    for y in bounds.min_y..=bounds.max_y {
        for x in bounds.min_x..=bounds.max_x {
            let pos = Pos { x, y };
            if selection.contains(pos) {
                match canvas.cell(pos) {
                    Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => text.push(ch),
                    Some(CellValue::WideCont) => {}
                    None => text.push(' '),
                }
            } else {
                text.push(' ');
            }
        }
        if y != bounds.max_y {
            text.push('\n');
        }
    }
    text
}

pub fn stamp_clipboard(
    canvas: &mut Canvas,
    clipboard: &Clipboard,
    pos: Pos,
    color: RgbColor,
    transparent: bool,
) {
    for y in 0..clipboard.height {
        for x in 0..clipboard.width {
            let target_x = pos.x + x;
            let target_y = pos.y + y;
            if target_x >= canvas.width || target_y >= canvas.height {
                continue;
            }
            let target = Pos {
                x: target_x,
                y: target_y,
            };
            match clipboard.get(x, y) {
                Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => {
                    let _ = canvas.put_glyph_colored(target, ch, color);
                }
                Some(CellValue::WideCont) => {}
                None if !transparent => canvas.clear(target),
                None => {}
            }
        }
    }
}

pub fn smart_fill_glyph(bounds: Bounds) -> char {
    if bounds.width() == 1 && bounds.height() > 1 {
        '|'
    } else if bounds.height() == 1 && bounds.width() > 1 {
        '-'
    } else {
        '*'
    }
}

pub fn export_system_clipboard_text(editor: &EditorSession, canvas: &Canvas) -> String {
    match editor.selection() {
        Some(selection) => export_selection_as_text(canvas, selection),
        None => export_bounds_as_text(canvas, editor.system_clipboard_bounds(canvas)),
    }
}

pub fn copy_selection_or_cell(editor: &mut EditorSession, canvas: &Canvas) -> bool {
    if editor.floating.is_some() {
        return false;
    }

    let clipboard = match editor.selection() {
        Some(selection) => capture_selection(canvas, selection),
        None => capture_bounds(
            canvas,
            editor
                .selection_or_cursor_bounds()
                .normalized_for_canvas(canvas),
        ),
    };
    editor.push_swatch(clipboard);
    true
}

pub fn cut_selection_or_cell(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    color: RgbColor,
) -> bool {
    if editor.floating.is_some() {
        return false;
    }

    let selection = editor.selection();
    let bounds = editor
        .selection_or_cursor_bounds()
        .normalized_for_canvas(canvas);
    let clipboard = selection
        .map(|selection| capture_selection(canvas, selection))
        .unwrap_or_else(|| capture_bounds(canvas, bounds));
    editor.push_swatch(clipboard);
    match selection {
        Some(selection) => fill_selection(canvas, selection, bounds, ' ', color),
        None => fill_bounds(canvas, bounds, ' ', color),
    }
    true
}

pub fn paste_primary_swatch(editor: &EditorSession, canvas: &mut Canvas, color: RgbColor) -> bool {
    let Some(clipboard) = editor.swatches[0]
        .as_ref()
        .map(|swatch| swatch.clipboard.clone())
    else {
        return false;
    };

    stamp_clipboard(canvas, &clipboard, editor.cursor, color, false);
    true
}

pub fn smart_fill(editor: &EditorSession, canvas: &mut Canvas, color: RgbColor) {
    let selection = editor.selection();
    let bounds = editor.selection_or_cursor_bounds();
    let ch = smart_fill_glyph(bounds);
    match selection {
        Some(selection) => fill_selection(canvas, selection, bounds, ch, color),
        None => fill_bounds(canvas, bounds, ch, color),
    }
}

pub fn draw_selection_border(editor: &EditorSession, canvas: &mut Canvas, color: RgbColor) -> bool {
    let Some(selection) = editor.selection() else {
        return false;
    };

    draw_border(canvas, selection, color);
    true
}

pub fn fill_selection_or_cell(
    editor: &EditorSession,
    canvas: &mut Canvas,
    ch: char,
    color: RgbColor,
) {
    let selection = editor.selection();
    let bounds = editor
        .selection_or_cursor_bounds()
        .normalized_for_canvas(canvas);
    match selection {
        Some(selection) => fill_selection(canvas, selection, bounds, ch, color),
        None => fill_bounds(canvas, bounds, ch, color),
    }
}

fn move_to_left_edge(editor: &mut EditorSession, canvas: &Canvas) {
    editor.cursor.x = editor.visible_bounds(canvas).min_x;
}

fn move_to_right_edge(editor: &mut EditorSession, canvas: &Canvas) {
    editor.cursor.x = editor.visible_bounds(canvas).max_x;
}

fn move_to_top_edge(editor: &mut EditorSession, canvas: &Canvas) {
    editor.cursor.y = editor.visible_bounds(canvas).min_y;
}

fn move_to_bottom_edge(editor: &mut EditorSession, canvas: &Canvas) {
    editor.cursor.y = editor.visible_bounds(canvas).max_y;
}

fn move_for_dir(editor: &mut EditorSession, canvas: &Canvas, dir: MoveDir) {
    match dir {
        MoveDir::Up => editor.move_up(canvas),
        MoveDir::Down => editor.move_down(canvas),
        MoveDir::Left => editor.move_left(canvas),
        MoveDir::Right => editor.move_right(canvas),
        MoveDir::LineStart => move_to_left_edge(editor, canvas),
        MoveDir::LineEnd => move_to_right_edge(editor, canvas),
        MoveDir::PageUp => move_to_top_edge(editor, canvas),
        MoveDir::PageDown => move_to_bottom_edge(editor, canvas),
    }
}

fn glyph_anchor(editor: &EditorSession, canvas: &Canvas) -> Pos {
    canvas.glyph_origin(editor.cursor).unwrap_or(editor.cursor)
}

pub fn paste_text_block(
    editor: &EditorSession,
    canvas: &mut Canvas,
    text: &str,
    color: RgbColor,
) -> bool {
    if text.is_empty() {
        return false;
    }

    let origin = editor.cursor;
    let mut changed = false;
    let mut x = origin.x;
    let mut y = origin.y;

    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                x = origin.x;
                y += 1;
                if y >= canvas.height {
                    break;
                }
            }
            _ => {
                if x < canvas.width && y < canvas.height {
                    let before = canvas.cell(Pos { x, y });
                    let _ = canvas.put_glyph_colored(Pos { x, y }, ch, color);
                    changed |= before != canvas.cell(Pos { x, y });
                }
                x += Canvas::display_width(ch);
            }
        }
    }

    changed
}

pub fn insert_char(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    ch: char,
    color: RgbColor,
) -> bool {
    let cursor = editor.cursor;
    let width = Canvas::display_width(ch);
    let before = canvas.cell(cursor);
    let _ = canvas.put_glyph_colored(cursor, ch, color);
    for _ in 0..width {
        editor.move_right(canvas);
    }
    before != canvas.cell(cursor)
}

pub fn backspace(editor: &mut EditorSession, canvas: &mut Canvas) -> bool {
    editor.move_left(canvas);
    let origin = canvas.glyph_origin(editor.cursor);
    let cursor = editor.cursor;
    let before = canvas.cell(cursor);
    canvas.clear(cursor);
    if let Some(origin) = origin {
        editor.cursor = origin;
    }
    before != canvas.cell(cursor)
}

pub fn delete_at_cursor(editor: &mut EditorSession, canvas: &mut Canvas) -> bool {
    if let Some(origin) = canvas.glyph_origin(editor.cursor) {
        editor.cursor = origin;
    }
    let cursor = editor.cursor;
    let before = canvas.cell(cursor);
    canvas.clear(cursor);
    before != canvas.cell(cursor)
}

pub fn push_left(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.push_left(anchor.y, anchor.x);
}

pub fn push_down(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.push_down(anchor.x, anchor.y);
}

pub fn push_up(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.push_up(anchor.x, anchor.y);
}

pub fn push_right(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.push_right(anchor.y, anchor.x);
}

pub fn pull_from_left(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.pull_from_left(anchor.y, anchor.x);
}

pub fn pull_from_down(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.pull_from_down(anchor.x, anchor.y);
}

pub fn pull_from_up(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.pull_from_up(anchor.x, anchor.y);
}

pub fn pull_from_right(editor: &EditorSession, canvas: &mut Canvas) {
    let anchor = glyph_anchor(editor, canvas);
    canvas.pull_from_right(anchor.y, anchor.x);
}

pub fn transpose_selection_corner(editor: &mut EditorSession) -> bool {
    if !editor.mode.is_selecting() {
        return false;
    }

    let Some(anchor) = editor.selection_anchor else {
        return false;
    };

    editor.selection_anchor = Some(editor.cursor);
    editor.cursor = anchor;
    true
}

pub fn handle_editor_key_press(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    key: AppKey,
    color: RgbColor,
) -> EditorKeyDispatch {
    let ctx = keymap::EditorContext {
        mode: editor.mode,
        has_selection_anchor: editor.selection_anchor.is_some(),
        is_floating: editor.floating.is_some(),
    };
    match KeyMap::default_standalone().resolve(key, ctx) {
        Some(action) => handle_editor_action(editor, canvas, action, color),
        None => EditorKeyDispatch::default(),
    }
}

pub fn handle_editor_action(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    action: EditorAction,
    color: RgbColor,
) -> EditorKeyDispatch {
    let mut effects = Vec::new();
    match action {
        EditorAction::Move {
            dir,
            extend_selection,
        } => {
            if extend_selection {
                editor.begin_selection();
            } else if editor.mode.is_selecting() {
                editor.clear_selection();
            }
            move_for_dir(editor, canvas, dir);
        }
        EditorAction::MoveDownLine => editor.move_down(canvas),
        EditorAction::Pan { dx, dy } => editor.pan_by(canvas, dx, dy),
        EditorAction::ClearSelection => editor.clear_selection(),
        EditorAction::TransposeSelectionCorner => {
            return EditorKeyDispatch {
                handled: transpose_selection_corner(editor),
                effects: Vec::new(),
            };
        }
        EditorAction::PushLeft => push_left(editor, canvas),
        EditorAction::PushRight => push_right(editor, canvas),
        EditorAction::PushUp => push_up(editor, canvas),
        EditorAction::PushDown => push_down(editor, canvas),
        EditorAction::PullFromLeft => pull_from_left(editor, canvas),
        EditorAction::PullFromRight => pull_from_right(editor, canvas),
        EditorAction::PullFromUp => pull_from_up(editor, canvas),
        EditorAction::PullFromDown => pull_from_down(editor, canvas),
        EditorAction::CopySelection => {
            let _ = copy_selection_or_cell(editor, canvas);
        }
        EditorAction::CutSelection => {
            let _ = cut_selection_or_cell(editor, canvas, color);
        }
        EditorAction::PastePrimarySwatch => {
            let _ = paste_primary_swatch(editor, canvas, color);
        }
        EditorAction::ExportSystemClipboard => {
            effects.push(HostEffect::CopyToClipboard(export_system_clipboard_text(
                editor, canvas,
            )));
        }
        EditorAction::ActivateSwatch(idx) => {
            editor.activate_swatch(idx);
        }
        EditorAction::SmartFill => smart_fill(editor, canvas, color),
        EditorAction::DrawBorder => {
            let _ = draw_selection_border(editor, canvas, color);
        }
        EditorAction::FillSelectionOrCell(ch) => {
            fill_selection_or_cell(editor, canvas, ch, color);
        }
        EditorAction::InsertChar(ch) => {
            let _ = insert_char(editor, canvas, ch, color);
        }
        EditorAction::Backspace => {
            let _ = backspace(editor, canvas);
        }
        EditorAction::Delete => {
            let _ = delete_at_cursor(editor, canvas);
        }
        EditorAction::ToggleFloatingTransparency => editor.toggle_float_transparency(),
    }
    EditorKeyDispatch {
        handled: true,
        effects,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerStrokeHint {
    Begin,
    End,
}

/// Whether [`handle_editor_pointer`] consumed the event or left it for the
/// host to bubble to outer UI layers.
///
/// Hosts that embed the editor as a widget alongside other clickable UI
/// (swatches, menus, other panes) should treat `Passthrough` as a signal
/// that the pointer event is still available for outer routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerOutcome {
    /// The editor acted on the event. Suppress outer UI routing.
    Consumed,
    /// The editor did not act on the event (e.g., click outside the
    /// canvas viewport, scroll event today, mid-drag sample without an
    /// active drag origin). The host may bubble this event to outer UI.
    #[default]
    Passthrough,
}

impl PointerOutcome {
    pub fn is_consumed(self) -> bool {
        matches!(self, PointerOutcome::Consumed)
    }

    pub fn is_passthrough(self) -> bool {
        matches!(self, PointerOutcome::Passthrough)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorPointerDispatch {
    pub outcome: PointerOutcome,
    pub stroke_hint: Option<PointerStrokeHint>,
}

impl EditorPointerDispatch {
    fn consumed() -> Self {
        Self {
            outcome: PointerOutcome::Consumed,
            stroke_hint: None,
        }
    }

    fn consumed_with(stroke_hint: PointerStrokeHint) -> Self {
        Self {
            outcome: PointerOutcome::Consumed,
            stroke_hint: Some(stroke_hint),
        }
    }
}

/// Apply a pointer event to the editor and return the dispatch outcome
/// plus any paint-stroke grouping hint the host should honor for undo
/// bookkeeping.
///
/// Hosts should run their own UI hit-testing first (swatch/help/picker
/// regions) — this handler only drives canvas-level pointer behavior
/// (floating paint drag, selection drag, viewport pan).
///
/// The returned [`PointerOutcome`] distinguishes events the editor
/// consumed (suppress outer routing) from events that should bubble.
///
/// **Hover policy.** Passive [`AppPointerKind::Moved`] events only move
/// the cursor when a floating selection is armed (so brush/stamp
/// previews follow the pointer); outside of that, passive motion is a
/// no-op and passes through. This is the conditional policy layered
/// hosts typically want — there is no separate knob to toggle it.
pub fn handle_editor_pointer(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    mouse: AppPointerEvent,
    color: RgbColor,
) -> EditorPointerDispatch {
    let canvas_pos = editor.canvas_pos_for_pointer(mouse.column, mouse.row, canvas);

    if editor.floating.is_some() {
        match mouse.kind {
            AppPointerKind::Moved => {
                if let Some(pos) = canvas_pos {
                    editor.cursor = pos;
                    return EditorPointerDispatch::consumed();
                }
                return EditorPointerDispatch::default();
            }
            AppPointerKind::Down(AppPointerButton::Left) => {
                if let Some(pos) = canvas_pos {
                    editor.cursor = pos;
                    begin_paint_stroke(editor);
                    paint_floating_drag(editor, canvas, pos, color);
                    return EditorPointerDispatch::consumed_with(PointerStrokeHint::Begin);
                }
                return EditorPointerDispatch::default();
            }
            AppPointerKind::Drag(AppPointerButton::Left) => {
                if let Some(pos) = canvas_pos {
                    paint_floating_drag(editor, canvas, pos, color);
                    return EditorPointerDispatch::consumed();
                }
                // Mid-stroke sample outside the canvas: the stroke is still
                // active, so keep the event from bubbling.
                if editor.paint_stroke_anchor.is_some() {
                    return EditorPointerDispatch::consumed();
                }
                return EditorPointerDispatch::default();
            }
            AppPointerKind::Up(AppPointerButton::Left) => {
                let had_stroke = editor.paint_stroke_anchor.is_some();
                end_paint_stroke(editor);
                if had_stroke {
                    return EditorPointerDispatch::consumed_with(PointerStrokeHint::End);
                }
                return EditorPointerDispatch::default();
            }
            AppPointerKind::Down(AppPointerButton::Right) => {
                // `dismiss_floating` calls `end_paint_stroke` internally.
                dismiss_floating(editor);
                return EditorPointerDispatch::consumed_with(PointerStrokeHint::End);
            }
            _ => {
                return EditorPointerDispatch::default();
            }
        }
    }

    match mouse.kind {
        AppPointerKind::Down(AppPointerButton::Right) => {
            if editor.viewport_contains(mouse.column, mouse.row) {
                editor.begin_pan(mouse.column, mouse.row);
                return EditorPointerDispatch::consumed();
            }
            EditorPointerDispatch::default()
        }
        AppPointerKind::Down(AppPointerButton::Left) => {
            let Some(pos) = canvas_pos else {
                return EditorPointerDispatch::default();
            };
            let extend_selection = mouse.modifiers.alt && editor.selection_anchor.is_some();
            let ellipse_drag = mouse.modifiers.ctrl && !extend_selection;

            if extend_selection {
                if let Some(anchor) = editor.selection_anchor {
                    editor.mode = Mode::Select;
                    editor.cursor = pos;
                    editor.drag_origin = Some(anchor);
                }
            } else {
                if editor.mode.is_selecting() {
                    editor.clear_selection();
                }
                editor.cursor = pos;
                editor.selection_shape = if ellipse_drag {
                    SelectionShape::Ellipse
                } else {
                    SelectionShape::Rect
                };
                editor.drag_origin = Some(pos);
            }
            EditorPointerDispatch::consumed()
        }
        AppPointerKind::Drag(AppPointerButton::Left) => {
            if let (Some(origin), Some(pos)) = (editor.drag_origin, canvas_pos) {
                if pos != origin || editor.mode.is_selecting() {
                    editor.selection_anchor = Some(origin);
                    editor.mode = Mode::Select;
                    editor.cursor = pos;
                }
                return EditorPointerDispatch::consumed();
            }
            EditorPointerDispatch::default()
        }
        AppPointerKind::Drag(AppPointerButton::Right) => {
            if editor.pan_drag.is_some() {
                editor.drag_pan(canvas, mouse.column, mouse.row);
                return EditorPointerDispatch::consumed();
            }
            EditorPointerDispatch::default()
        }
        AppPointerKind::Up(AppPointerButton::Left) => {
            if editor.drag_origin.take().is_some() {
                return EditorPointerDispatch::consumed();
            }
            EditorPointerDispatch::default()
        }
        AppPointerKind::Up(AppPointerButton::Right) => {
            if editor.pan_drag.is_some() {
                editor.end_pan();
                return EditorPointerDispatch::consumed();
            }
            EditorPointerDispatch::default()
        }
        _ => EditorPointerDispatch::default(),
    }
}

pub fn begin_paint_stroke(editor: &mut EditorSession) {
    editor.paint_stroke_anchor = Some(editor.cursor);
    editor.paint_stroke_last = None;
}

pub fn end_paint_stroke(editor: &mut EditorSession) {
    editor.paint_stroke_anchor = None;
    editor.paint_stroke_last = None;
}

pub fn dismiss_floating(editor: &mut EditorSession) {
    end_paint_stroke(editor);
    editor.floating = None;
}

pub fn stamp_floating(editor: &EditorSession, canvas: &mut Canvas, color: RgbColor) -> bool {
    let Some(floating) = editor.floating.as_ref() else {
        return false;
    };

    stamp_clipboard(
        canvas,
        &floating.clipboard,
        editor.cursor,
        color,
        floating.transparent,
    );
    true
}

fn snap_horizontal_brush_x(anchor_x: usize, raw_x: usize, brush_width: usize) -> usize {
    if brush_width <= 1 {
        return raw_x;
    }

    if raw_x >= anchor_x {
        anchor_x + ((raw_x - anchor_x) / brush_width) * brush_width
    } else {
        anchor_x - ((anchor_x - raw_x) / brush_width) * brush_width
    }
}

fn line_points(start: Pos, end: Pos) -> Vec<Pos> {
    let mut points = Vec::new();
    let mut x = start.x as isize;
    let mut y = start.y as isize;
    let target_x = end.x as isize;
    let target_y = end.y as isize;
    let dx = (target_x - x).abs();
    let sx = if x < target_x { 1 } else { -1 };
    let dy = -(target_y - y).abs();
    let sy = if y < target_y { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        points.push(Pos {
            x: x as usize,
            y: y as usize,
        });

        if x == target_x && y == target_y {
            break;
        }

        let twice_err = 2 * err;
        if twice_err >= dy {
            err += dy;
            x += sx;
        }
        if twice_err <= dx {
            err += dx;
            y += sy;
        }
    }

    points
}

fn paint_floating_at_cursor(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    color: RgbColor,
) -> bool {
    if !stamp_floating(editor, canvas, color) {
        return false;
    }
    editor.paint_stroke_last = Some(editor.cursor);
    true
}

fn paint_floating_diagonal_segment(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    start: Pos,
    end: Pos,
    brush_width: usize,
    color: RgbColor,
) -> bool {
    let mut changed = false;
    let mut last_stamped = start;
    for point in line_points(start, end).into_iter().skip(1) {
        let should_stamp =
            point.y != last_stamped.y || point.x.abs_diff(last_stamped.x) >= brush_width;
        if !should_stamp {
            continue;
        }

        editor.cursor = point;
        changed |= paint_floating_at_cursor(editor, canvas, color);
        last_stamped = point;
    }

    let should_stamp_end = end.y != last_stamped.y || end.x.abs_diff(last_stamped.x) >= brush_width;
    if should_stamp_end {
        editor.cursor = end;
        changed |= paint_floating_at_cursor(editor, canvas, color);
    }
    changed
}

pub fn paint_floating_drag(
    editor: &mut EditorSession,
    canvas: &mut Canvas,
    raw_pos: Pos,
    color: RgbColor,
) -> bool {
    let Some(last) = editor.paint_stroke_last else {
        editor.cursor = raw_pos;
        return paint_floating_at_cursor(editor, canvas, color);
    };

    let anchor = editor.paint_stroke_anchor.unwrap_or(last);
    let brush_width = editor.floating_brush_width();
    let is_pure_horizontal =
        brush_width > 1 && raw_pos.y == last.y && raw_pos.y == anchor.y && last.y == anchor.y;

    if is_pure_horizontal {
        let target = Pos {
            x: snap_horizontal_brush_x(anchor.x, raw_pos.x, brush_width),
            y: raw_pos.y,
        };
        if target == last {
            return false;
        }
        editor.cursor = target;
        return paint_floating_at_cursor(editor, canvas, color);
    }

    if brush_width > 1 && raw_pos.y == last.y {
        if raw_pos.x.abs_diff(last.x) < brush_width {
            return false;
        }

        editor.cursor = raw_pos;
        return paint_floating_at_cursor(editor, canvas, color);
    }

    if brush_width > 1 && raw_pos.y != last.y {
        return paint_floating_diagonal_segment(editor, canvas, last, raw_pos, brush_width, color);
    }

    if raw_pos == last {
        return false;
    }

    editor.cursor = raw_pos;
    paint_floating_at_cursor(editor, canvas, color)
}

#[cfg(test)]
mod tests {
    use super::{
        backspace, begin_paint_stroke, capture_bounds, capture_selection, copy_selection_or_cell,
        cut_selection_or_cell, delete_at_cursor, diff_canvas_op, dismiss_floating, draw_border,
        draw_selection_border, export_selection_as_text, export_system_clipboard_text,
        fill_selection, fill_selection_or_cell, handle_editor_action, handle_editor_key_press,
        handle_editor_pointer, insert_char, paint_floating_drag, paste_primary_swatch,
        paste_text_block, smart_fill, smart_fill_glyph, stamp_clipboard,
        transpose_selection_corner, AppKey, AppKeyCode, AppModifiers, AppPointerButton,
        AppPointerEvent, AppPointerKind, Bounds, Clipboard, EditorAction, EditorKeyDispatch,
        EditorSession, FloatingSelection, HostEffect, Mode, MoveDir, PointerOutcome,
        PointerStrokeHint, Selection, SelectionShape, SwatchActivation, Viewport,
    };
    use dartboard_core::{Canvas, CanvasOp, CellValue, Pos, RgbColor};

    #[test]
    fn ellipse_contains_degenerate_line() {
        let selection = Selection {
            anchor: Pos { x: 2, y: 4 },
            cursor: Pos { x: 2, y: 8 },
            shape: SelectionShape::Ellipse,
        };

        assert!(selection.contains(Pos { x: 2, y: 6 }));
        assert!(!selection.contains(Pos { x: 3, y: 6 }));
    }

    #[test]
    fn bounds_normalize_wide_glyph_edges() {
        let mut canvas = Canvas::with_size(8, 4);
        let _ = canvas.put_glyph(Pos { x: 2, y: 1 }, '🌱');

        let bounds = Bounds {
            min_x: 3,
            max_x: 3,
            min_y: 1,
            max_y: 1,
        }
        .normalized_for_canvas(&canvas);

        assert_eq!(bounds.min_x, 2);
        assert_eq!(bounds.max_x, 3);
    }

    #[test]
    fn diff_canvas_op_uses_default_fg_for_uncolored_cells() {
        let before = Canvas::with_size(4, 2);
        let mut after = before.clone();
        after.set(Pos { x: 1, y: 0 }, 'X');

        let op = diff_canvas_op(&before, &after, RgbColor::new(9, 8, 7)).unwrap();
        match op {
            CanvasOp::PaintCell { fg, .. } => assert_eq!(fg, RgbColor::new(9, 8, 7)),
            other => panic!("expected PaintCell, got {other:?}"),
        }
    }

    #[test]
    fn editor_session_selection_tracks_cursor() {
        let mut session = EditorSession {
            viewport: Viewport {
                width: 20,
                height: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        session.cursor = Pos { x: 3, y: 4 };
        session.begin_selection();
        session.cursor = Pos { x: 8, y: 6 };

        let selection = session.selection().unwrap();
        assert_eq!(selection.anchor, Pos { x: 3, y: 4 });
        assert_eq!(selection.cursor, Pos { x: 8, y: 6 });
        assert_eq!(selection.shape, SelectionShape::Rect);
    }

    #[test]
    fn set_viewport_clamps_origin_and_cursor() {
        let canvas = Canvas::with_size(40, 20);
        let mut session = EditorSession {
            cursor: Pos { x: 39, y: 19 },
            viewport_origin: Pos { x: 25, y: 18 },
            ..Default::default()
        };

        session.set_viewport(
            Viewport {
                x: 2,
                y: 3,
                width: 10,
                height: 5,
            },
            &canvas,
        );

        assert_eq!(session.viewport_origin, Pos { x: 25, y: 15 });
        assert_eq!(session.cursor, Pos { x: 34, y: 19 });
    }

    #[test]
    fn move_right_scrolls_viewport_to_keep_cursor_visible() {
        let canvas = Canvas::with_size(40, 10);
        let mut session = EditorSession {
            cursor: Pos { x: 3, y: 2 },
            viewport: Viewport {
                width: 4,
                height: 3,
                ..Default::default()
            },
            ..Default::default()
        };

        session.move_right(&canvas);

        assert_eq!(session.cursor, Pos { x: 4, y: 2 });
        assert_eq!(session.viewport_origin, Pos { x: 1, y: 0 });
    }

    #[test]
    fn drag_pan_clamps_to_canvas_bounds() {
        let canvas = Canvas::with_size(20, 10);
        let mut session = EditorSession {
            cursor: Pos { x: 6, y: 5 },
            viewport: Viewport {
                width: 6,
                height: 4,
                ..Default::default()
            },
            viewport_origin: Pos { x: 8, y: 4 },
            ..Default::default()
        };

        session.begin_pan(12, 8);
        session.drag_pan(&canvas, 0, 0);

        assert_eq!(session.viewport_origin, Pos { x: 14, y: 6 });
        assert_eq!(session.cursor, Pos { x: 14, y: 6 });
        session.end_pan();
        assert!(session.pan_drag.is_none());
    }

    #[test]
    fn system_clipboard_bounds_falls_back_to_canvas() {
        let canvas = Canvas::with_size(8, 4);
        let session = EditorSession::default();

        assert_eq!(
            session.system_clipboard_bounds(&canvas),
            Bounds {
                min_x: 0,
                max_x: 7,
                min_y: 0,
                max_y: 3,
            }
        );
    }

    #[test]
    fn push_swatch_rotates_unpinned_history_only() {
        let clipboard_a = Clipboard::new(1, 1, vec![Some(CellValue::Narrow('A'))]);
        let clipboard_b = Clipboard::new(1, 1, vec![Some(CellValue::Narrow('B'))]);
        let clipboard_c = Clipboard::new(1, 1, vec![Some(CellValue::Narrow('C'))]);
        let mut session = EditorSession::default();

        session.push_swatch(clipboard_a.clone());
        session.push_swatch(clipboard_b.clone());
        session.toggle_pin(1);
        session.push_swatch(clipboard_c.clone());

        assert_eq!(session.populated_swatch_count(), 3);
        assert_eq!(
            session.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('C'))
        );
        assert!(session.swatches[1].as_ref().unwrap().pinned);
        assert_eq!(
            session.swatches[1].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(
            session.swatches[2].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
    }

    #[test]
    fn activating_same_swatch_toggles_transparency() {
        let clipboard = Clipboard::new(1, 1, vec![Some(CellValue::Narrow('X'))]);
        let mut session = EditorSession::default();
        session.push_swatch(clipboard);

        assert_eq!(
            session.activate_swatch(0),
            SwatchActivation::ActivatedFloating
        );
        assert_eq!(
            session.activate_swatch(0),
            SwatchActivation::ToggledTransparency
        );
        assert!(session.floating.as_ref().unwrap().transparent);
        assert_eq!(session.floating_brush_width(), 1);
    }

    #[test]
    fn capture_and_export_selection_respects_mask_shape() {
        let mut canvas = Canvas::with_size(5, 3);
        canvas.set(Pos { x: 0, y: 0 }, 'A');
        canvas.set(Pos { x: 1, y: 0 }, 'B');
        canvas.set(Pos { x: 2, y: 0 }, 'C');
        canvas.set(Pos { x: 0, y: 1 }, 'D');
        canvas.set(Pos { x: 1, y: 1 }, 'E');
        canvas.set(Pos { x: 2, y: 1 }, 'F');

        let selection = Selection {
            anchor: Pos { x: 0, y: 0 },
            cursor: Pos { x: 2, y: 1 },
            shape: SelectionShape::Ellipse,
        };

        let clipboard = capture_selection(&canvas, selection);
        assert_eq!(clipboard.width, 3);
        assert_eq!(clipboard.height, 2);
        assert_eq!(clipboard.get(0, 0), Some(CellValue::Narrow('A')));
        assert_eq!(clipboard.get(1, 0), Some(CellValue::Narrow('B')));
        assert_eq!(clipboard.get(2, 0), Some(CellValue::Narrow('C')));
        assert_eq!(clipboard.get(0, 1), Some(CellValue::Narrow('D')));
        assert_eq!(clipboard.get(1, 1), Some(CellValue::Narrow('E')));
        assert_eq!(clipboard.get(2, 1), Some(CellValue::Narrow('F')));
        assert_eq!(export_selection_as_text(&canvas, selection), "ABC\nDEF");
    }

    #[test]
    fn fill_selection_masks_ellipse_edges() {
        let mut canvas = Canvas::with_size(5, 5);
        let selection = Selection {
            anchor: Pos { x: 0, y: 0 },
            cursor: Pos { x: 4, y: 4 },
            shape: SelectionShape::Ellipse,
        };
        let bounds = selection.bounds();

        fill_selection(&mut canvas, selection, bounds, 'x', RgbColor::new(1, 2, 3));

        assert_eq!(canvas.cell(Pos { x: 0, y: 0 }), None);
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 0 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 0, y: 2 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 2 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 4, y: 2 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 4 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(canvas.cell(Pos { x: 4, y: 4 }), None);
    }

    #[test]
    fn draw_border_writes_ascii_frame_for_rect_selection() {
        let mut canvas = Canvas::with_size(6, 4);
        let selection = Selection {
            anchor: Pos { x: 1, y: 1 },
            cursor: Pos { x: 3, y: 2 },
            shape: SelectionShape::Rect,
        };

        draw_border(&mut canvas, selection, RgbColor::new(7, 8, 9));

        let captured = capture_bounds(
            &canvas,
            Bounds {
                min_x: 1,
                max_x: 3,
                min_y: 1,
                max_y: 2,
            },
        );
        assert_eq!(captured.get(0, 0), Some(CellValue::Narrow('.')));
        assert_eq!(captured.get(1, 0), Some(CellValue::Narrow('-')));
        assert_eq!(captured.get(2, 0), Some(CellValue::Narrow('.')));
        assert_eq!(captured.get(0, 1), Some(CellValue::Narrow('`')));
        assert_eq!(captured.get(1, 1), Some(CellValue::Narrow('-')));
        assert_eq!(captured.get(2, 1), Some(CellValue::Narrow('\'')));
    }

    #[test]
    fn stamp_clipboard_honors_transparency() {
        let clipboard = Clipboard::new(
            2,
            2,
            vec![
                Some(CellValue::Narrow('A')),
                None,
                None,
                Some(CellValue::Narrow('B')),
            ],
        );
        let mut canvas = Canvas::with_size(4, 4);
        canvas.set(Pos { x: 2, y: 1 }, 'z');
        canvas.set(Pos { x: 1, y: 2 }, 'y');

        stamp_clipboard(
            &mut canvas,
            &clipboard,
            Pos { x: 1, y: 1 },
            RgbColor::new(5, 6, 7),
            true,
        );
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 1 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 1 }),
            Some(CellValue::Narrow('z'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 2 }),
            Some(CellValue::Narrow('y'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 2 }),
            Some(CellValue::Narrow('B'))
        );

        stamp_clipboard(
            &mut canvas,
            &clipboard,
            Pos { x: 1, y: 1 },
            RgbColor::new(5, 6, 7),
            false,
        );
        assert_eq!(canvas.cell(Pos { x: 2, y: 1 }), None);
        assert_eq!(canvas.cell(Pos { x: 1, y: 2 }), None);
    }

    #[test]
    fn smart_fill_glyph_matches_bounds_shape() {
        assert_eq!(
            smart_fill_glyph(Bounds {
                min_x: 0,
                max_x: 0,
                min_y: 0,
                max_y: 2,
            }),
            '|'
        );
        assert_eq!(
            smart_fill_glyph(Bounds {
                min_x: 0,
                max_x: 2,
                min_y: 0,
                max_y: 0,
            }),
            '-'
        );
        assert_eq!(
            smart_fill_glyph(Bounds {
                min_x: 0,
                max_x: 1,
                min_y: 0,
                max_y: 1,
            }),
            '*'
        );
    }

    #[test]
    fn copy_and_cut_commands_update_swatches_and_canvas() {
        let mut canvas = Canvas::with_size(4, 2);
        canvas.set(Pos { x: 1, y: 0 }, 'Q');
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 0 },
            ..Default::default()
        };

        assert!(copy_selection_or_cell(&mut editor, &canvas));
        assert_eq!(
            editor.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('Q'))
        );

        assert!(cut_selection_or_cell(
            &mut editor,
            &mut canvas,
            RgbColor::new(1, 2, 3)
        ));
        assert_eq!(canvas.cell(Pos { x: 1, y: 0 }), None);
        assert_eq!(
            editor.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('Q'))
        );
    }

    #[test]
    fn paste_and_fill_commands_use_editor_state() {
        let mut canvas = Canvas::with_size(6, 4);
        let mut editor = EditorSession {
            cursor: Pos { x: 2, y: 1 },
            ..Default::default()
        };
        editor.push_swatch(Clipboard::new(1, 1, vec![Some(CellValue::Narrow('P'))]));

        assert!(paste_primary_swatch(
            &editor,
            &mut canvas,
            RgbColor::new(4, 5, 6)
        ));
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 1 }),
            Some(CellValue::Narrow('P'))
        );

        fill_selection_or_cell(&editor, &mut canvas, 'x', RgbColor::new(7, 8, 9));
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 1 }),
            Some(CellValue::Narrow('x'))
        );
    }

    #[test]
    fn smart_fill_border_and_export_commands_follow_selection() {
        let mut canvas = Canvas::with_size(6, 4);
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 1 },
            ..Default::default()
        };
        editor.begin_selection();
        editor.cursor = Pos { x: 3, y: 2 };

        smart_fill(&editor, &mut canvas, RgbColor::new(1, 2, 3));
        assert_eq!(export_system_clipboard_text(&editor, &canvas), "***\n***");

        assert!(draw_selection_border(
            &editor,
            &mut canvas,
            RgbColor::new(9, 8, 7)
        ));
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 1 }),
            Some(CellValue::Narrow('.'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 3, y: 2 }),
            Some(CellValue::Narrow('\''))
        );
    }

    #[test]
    fn floating_drag_updates_cursor_and_stroke_state() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 1 },
            floating: Some(FloatingSelection {
                clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('F'))]),
                transparent: false,
                source_index: Some(0),
            }),
            ..Default::default()
        };

        begin_paint_stroke(&mut editor);
        assert!(paint_floating_drag(
            &mut editor,
            &mut canvas,
            Pos { x: 1, y: 1 },
            RgbColor::new(3, 4, 5)
        ));
        assert_eq!(editor.paint_stroke_last, Some(Pos { x: 1, y: 1 }));
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 1 }),
            Some(CellValue::Narrow('F'))
        );

        assert!(paint_floating_drag(
            &mut editor,
            &mut canvas,
            Pos { x: 3, y: 1 },
            RgbColor::new(3, 4, 5)
        ));
        assert_eq!(editor.cursor, Pos { x: 3, y: 1 });
        assert_eq!(editor.paint_stroke_last, Some(Pos { x: 3, y: 1 }));
        assert_eq!(
            canvas.cell(Pos { x: 3, y: 1 }),
            Some(CellValue::Narrow('F'))
        );
    }

    #[test]
    fn dismiss_floating_clears_float_and_stroke_tracking() {
        let mut editor = EditorSession {
            cursor: Pos { x: 2, y: 2 },
            floating: Some(FloatingSelection {
                clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('X'))]),
                transparent: true,
                source_index: None,
            }),
            paint_stroke_anchor: Some(Pos { x: 1, y: 1 }),
            paint_stroke_last: Some(Pos { x: 2, y: 2 }),
            ..Default::default()
        };

        dismiss_floating(&mut editor);

        assert!(editor.floating.is_none());
        assert!(editor.paint_stroke_anchor.is_none());
        assert!(editor.paint_stroke_last.is_none());
    }

    #[test]
    fn insert_and_delete_commands_mutate_canvas_and_cursor() {
        let mut canvas = Canvas::with_size(8, 3);
        let mut editor = EditorSession::default();

        assert!(insert_char(
            &mut editor,
            &mut canvas,
            'A',
            RgbColor::new(1, 2, 3)
        ));
        assert_eq!(
            canvas.cell(Pos { x: 0, y: 0 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(editor.cursor, Pos { x: 1, y: 0 });

        assert!(backspace(&mut editor, &mut canvas));
        assert_eq!(canvas.cell(Pos { x: 0, y: 0 }), None);
        assert_eq!(editor.cursor, Pos { x: 0, y: 0 });

        let _ = canvas.put_glyph_colored(Pos { x: 2, y: 1 }, 'Z', RgbColor::new(4, 5, 6));
        editor.cursor = Pos { x: 2, y: 1 };
        assert!(delete_at_cursor(&mut editor, &mut canvas));
        assert_eq!(canvas.cell(Pos { x: 2, y: 1 }), None);
    }

    #[test]
    fn paste_text_block_uses_cursor_origin() {
        let mut canvas = Canvas::with_size(6, 4);
        let editor = EditorSession {
            cursor: Pos { x: 1, y: 1 },
            ..Default::default()
        };

        assert!(paste_text_block(
            &editor,
            &mut canvas,
            "AB\nC",
            RgbColor::new(7, 8, 9)
        ));
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 1 }),
            Some(CellValue::Narrow('A'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 1 }),
            Some(CellValue::Narrow('B'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 2 }),
            Some(CellValue::Narrow('C'))
        );
    }

    #[test]
    fn transpose_selection_corner_swaps_anchor_and_cursor() {
        let mut editor = EditorSession {
            cursor: Pos { x: 4, y: 3 },
            selection_anchor: Some(Pos { x: 1, y: 2 }),
            mode: super::Mode::Select,
            ..Default::default()
        };

        assert!(transpose_selection_corner(&mut editor));
        assert_eq!(editor.selection_anchor, Some(Pos { x: 4, y: 3 }));
        assert_eq!(editor.cursor, Pos { x: 1, y: 2 });
    }

    #[test]
    fn handle_editor_key_press_returns_clipboard_effect_for_alt_c() {
        let mut canvas = Canvas::with_size(4, 2);
        canvas.set(Pos { x: 0, y: 0 }, 'A');
        let mut editor = EditorSession::default();

        let dispatch = handle_editor_key_press(
            &mut editor,
            &mut canvas,
            AppKey {
                code: AppKeyCode::Char('c'),
                modifiers: AppModifiers {
                    alt: true,
                    ..Default::default()
                },
            },
            RgbColor::new(1, 2, 3),
        );

        assert_eq!(
            dispatch,
            EditorKeyDispatch {
                handled: true,
                effects: vec![HostEffect::CopyToClipboard("A   \n    ".to_string())],
            }
        );
    }

    #[test]
    fn handle_editor_key_press_handles_selection_fill_and_ctrl_commands() {
        let mut canvas = Canvas::with_size(6, 3);
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 1 },
            ..Default::default()
        };

        let fill_dispatch = handle_editor_key_press(
            &mut editor,
            &mut canvas,
            AppKey {
                code: AppKeyCode::Right,
                modifiers: AppModifiers {
                    shift: true,
                    ..Default::default()
                },
            },
            RgbColor::new(1, 2, 3),
        );
        assert!(fill_dispatch.handled);
        assert!(editor.mode.is_selecting());

        let fill_dispatch = handle_editor_key_press(
            &mut editor,
            &mut canvas,
            AppKey {
                code: AppKeyCode::Char('x'),
                modifiers: AppModifiers::default(),
            },
            RgbColor::new(1, 2, 3),
        );
        assert!(fill_dispatch.handled);
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 1 }),
            Some(CellValue::Narrow('x'))
        );
        assert_eq!(
            canvas.cell(Pos { x: 2, y: 1 }),
            Some(CellValue::Narrow('x'))
        );

        let copy_dispatch = handle_editor_key_press(
            &mut editor,
            &mut canvas,
            AppKey {
                code: AppKeyCode::Char('c'),
                modifiers: AppModifiers {
                    ctrl: true,
                    ..Default::default()
                },
            },
            RgbColor::new(1, 2, 3),
        );
        assert!(copy_dispatch.handled);
        assert_eq!(
            editor.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('x'))
        );
    }

    #[test]
    fn handle_editor_action_move_extends_selection_when_requested() {
        let mut canvas = Canvas::with_size(6, 3);
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 1 },
            ..Default::default()
        };

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::Move {
                dir: MoveDir::Right,
                extend_selection: true,
            },
            RgbColor::new(0, 0, 0),
        );

        assert!(dispatch.handled);
        assert!(dispatch.effects.is_empty());
        assert!(editor.mode.is_selecting());
        assert_eq!(editor.selection_anchor, Some(Pos { x: 1, y: 1 }));
        assert_eq!(editor.cursor, Pos { x: 2, y: 1 });
    }

    #[test]
    fn handle_editor_action_move_clears_selection_when_not_extending() {
        let mut canvas = Canvas::with_size(6, 3);
        let mut editor = EditorSession {
            cursor: Pos { x: 2, y: 1 },
            selection_anchor: Some(Pos { x: 1, y: 1 }),
            mode: Mode::Select,
            ..Default::default()
        };

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::Move {
                dir: MoveDir::Right,
                extend_selection: false,
            },
            RgbColor::new(0, 0, 0),
        );

        assert!(dispatch.handled);
        assert!(editor.selection_anchor.is_none());
        assert!(!editor.mode.is_selecting());
        assert_eq!(editor.cursor, Pos { x: 3, y: 1 });
    }

    #[test]
    fn handle_editor_action_export_system_clipboard_emits_effect() {
        let mut canvas = Canvas::with_size(4, 2);
        canvas.set(Pos { x: 0, y: 0 }, 'A');
        let mut editor = EditorSession::default();

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::ExportSystemClipboard,
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(
            dispatch,
            EditorKeyDispatch {
                handled: true,
                effects: vec![HostEffect::CopyToClipboard("A   \n    ".to_string())],
            }
        );
    }

    #[test]
    fn handle_editor_action_insert_char_writes_cell() {
        let mut canvas = Canvas::with_size(4, 2);
        let mut editor = EditorSession {
            cursor: Pos { x: 1, y: 0 },
            ..Default::default()
        };

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::InsertChar('Z'),
            RgbColor::new(9, 9, 9),
        );

        assert!(dispatch.handled);
        assert_eq!(
            canvas.cell(Pos { x: 1, y: 0 }),
            Some(CellValue::Narrow('Z'))
        );
    }

    #[test]
    fn handle_editor_action_transpose_reports_unhandled_without_anchor() {
        let mut canvas = Canvas::with_size(4, 2);
        let mut editor = EditorSession::default();

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::TransposeSelectionCorner,
            RgbColor::new(0, 0, 0),
        );

        assert!(!dispatch.handled);
    }

    #[test]
    fn handle_editor_action_pan_shifts_viewport_origin() {
        let mut canvas = Canvas::with_size(40, 20);
        let mut editor = EditorSession::default();
        editor.set_viewport(
            Viewport {
                x: 0,
                y: 0,
                width: 10,
                height: 5,
            },
            &canvas,
        );
        editor.viewport_origin = Pos { x: 5, y: 5 };
        let origin_before = editor.viewport_origin;

        let dispatch = handle_editor_action(
            &mut editor,
            &mut canvas,
            EditorAction::Pan { dx: 1, dy: -1 },
            RgbColor::new(0, 0, 0),
        );

        assert!(dispatch.handled);
        assert_eq!(
            editor.viewport_origin,
            Pos {
                x: origin_before.x + 1,
                y: origin_before.y - 1,
            }
        );
    }

    fn pointer(col: u16, row: u16, kind: AppPointerKind) -> AppPointerEvent {
        AppPointerEvent {
            column: col,
            row,
            kind,
            modifiers: AppModifiers::default(),
        }
    }

    fn viewport_editor(canvas: &Canvas) -> EditorSession {
        let mut editor = EditorSession::default();
        editor.set_viewport(
            Viewport {
                x: 0,
                y: 0,
                width: canvas.width as u16,
                height: canvas.height as u16,
            },
            canvas,
        );
        editor
    }

    #[test]
    fn pointer_left_down_outside_viewport_passes_through() {
        let mut canvas = Canvas::with_size(4, 2);
        let mut editor = viewport_editor(&canvas);

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(99, 99, AppPointerKind::Down(AppPointerButton::Left)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Passthrough);
        assert_eq!(dispatch.stroke_hint, None);
        assert!(editor.drag_origin.is_none());
    }

    #[test]
    fn pointer_non_floating_left_down_arms_selection_drag() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 2, AppPointerKind::Down(AppPointerButton::Left)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert_eq!(dispatch.stroke_hint, None);
        assert_eq!(editor.cursor, Pos { x: 3, y: 2 });
        assert_eq!(editor.drag_origin, Some(Pos { x: 3, y: 2 }));
    }

    #[test]
    fn pointer_non_floating_right_down_begins_pan_inside_viewport() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(2, 1, AppPointerKind::Down(AppPointerButton::Right)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert!(editor.pan_drag.is_some());
    }

    #[test]
    fn pointer_right_down_outside_viewport_passes_through() {
        let mut canvas = Canvas::with_size(4, 2);
        let mut editor = viewport_editor(&canvas);

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(99, 99, AppPointerKind::Down(AppPointerButton::Right)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Passthrough);
        assert!(editor.pan_drag.is_none());
    }

    #[test]
    fn pointer_scroll_event_passes_through() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 2, AppPointerKind::ScrollUp),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Passthrough);
    }

    #[test]
    fn pointer_moved_without_floating_does_not_move_caret() {
        // Default host policy: passive hover over the canvas must not drag
        // the caret around when no floating preview is armed.
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);
        let initial_cursor = editor.cursor;

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 2, AppPointerKind::Moved),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Passthrough);
        assert_eq!(editor.cursor, initial_cursor);
    }

    #[test]
    fn pointer_floating_hover_tracks_cursor_by_default() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);
        editor.floating = Some(FloatingSelection {
            clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('x'))]),
            transparent: false,
            source_index: None,
        });

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(4, 2, AppPointerKind::Moved),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert_eq!(editor.cursor, Pos { x: 4, y: 2 });
    }

    #[test]
    fn pointer_non_floating_left_drag_establishes_selection() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);

        handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(2, 1, AppPointerKind::Down(AppPointerButton::Left)),
            RgbColor::new(0, 0, 0),
        );
        handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(5, 2, AppPointerKind::Drag(AppPointerButton::Left)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(editor.selection_anchor, Some(Pos { x: 2, y: 1 }));
        assert_eq!(editor.cursor, Pos { x: 5, y: 2 });
        assert!(editor.mode.is_selecting());
    }

    #[test]
    fn pointer_floating_left_down_begins_stroke_and_paints() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);
        editor.floating = Some(FloatingSelection {
            clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('x'))]),
            transparent: false,
            source_index: None,
        });

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 1, AppPointerKind::Down(AppPointerButton::Left)),
            RgbColor::new(1, 2, 3),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert_eq!(dispatch.stroke_hint, Some(PointerStrokeHint::Begin));
        assert_eq!(editor.cursor, Pos { x: 3, y: 1 });
        assert!(editor.paint_stroke_anchor.is_some());
        assert_eq!(
            canvas.cell(Pos { x: 3, y: 1 }),
            Some(CellValue::Narrow('x'))
        );
    }

    #[test]
    fn pointer_floating_left_up_ends_stroke() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);
        editor.floating = Some(FloatingSelection {
            clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('x'))]),
            transparent: false,
            source_index: None,
        });
        editor.paint_stroke_anchor = Some(Pos { x: 0, y: 0 });

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 1, AppPointerKind::Up(AppPointerButton::Left)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert_eq!(dispatch.stroke_hint, Some(PointerStrokeHint::End));
        assert!(editor.paint_stroke_anchor.is_none());
    }

    #[test]
    fn pointer_floating_right_down_dismisses_and_ends_stroke() {
        let mut canvas = Canvas::with_size(8, 4);
        let mut editor = viewport_editor(&canvas);
        editor.floating = Some(FloatingSelection {
            clipboard: Clipboard::new(1, 1, vec![Some(CellValue::Narrow('x'))]),
            transparent: false,
            source_index: None,
        });

        let dispatch = handle_editor_pointer(
            &mut editor,
            &mut canvas,
            pointer(3, 1, AppPointerKind::Down(AppPointerButton::Right)),
            RgbColor::new(0, 0, 0),
        );

        assert_eq!(dispatch.outcome, PointerOutcome::Consumed);
        assert_eq!(dispatch.stroke_hint, Some(PointerStrokeHint::End));
        assert!(editor.floating.is_none());
    }
}
