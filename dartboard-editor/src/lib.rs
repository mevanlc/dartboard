use std::collections::HashSet;

use dartboard_core::{ops::CellWrite, Canvas, CanvasOp, CellValue, Pos, RgbColor};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
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

#[cfg(test)]
mod tests {
    use super::{
        diff_canvas_op, Bounds, EditorSession, Selection, SelectionShape, Viewport,
    };
    use dartboard_core::{Canvas, CanvasOp, Pos, RgbColor};

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
}
