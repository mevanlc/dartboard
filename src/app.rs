use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::canvas::{Canvas, Pos};

const UNDO_DEPTH_CAP: usize = 500;

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

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
}

impl Bounds {
    fn from_points(a: Pos, b: Pos) -> Self {
        Self {
            min_x: a.x.min(b.x),
            max_x: a.x.max(b.x),
            min_y: a.y.min(b.y),
            max_y: a.y.max(b.y),
        }
    }

    fn single(pos: Pos) -> Self {
        Self::from_points(pos, pos)
    }

    fn width(self) -> usize {
        self.max_x - self.min_x + 1
    }

    fn height(self) -> usize {
        self.max_y - self.min_y + 1
    }
}

#[derive(Debug, Clone)]
pub struct Clipboard {
    pub width: usize,
    pub height: usize,
    cells: Vec<char>,
}

impl Clipboard {
    pub fn get(&self, x: usize, y: usize) -> char {
        self.cells[y * self.width + x]
    }
}

#[derive(Debug, Clone)]
pub struct FloatingSelection {
    pub clipboard: Clipboard,
    pub transparent: bool,
}

#[derive(Debug, Clone, Copy)]
struct PanDrag {
    col: u16,
    row: u16,
    origin: Pos,
}

pub struct App {
    pub canvas: Canvas,
    pub cursor: Pos,
    pub mode: Mode,
    pub should_quit: bool,
    pub show_help: bool,
    pub viewport: Rect,
    pub viewport_origin: Pos,
    pub selection_anchor: Option<Pos>,
    drag_origin: Option<Pos>,
    pan_drag: Option<PanDrag>,
    clipboard: Option<Clipboard>,
    pub floating: Option<FloatingSelection>,
    last_clip_bounds: Option<Bounds>,
    paint_canvas_before: Option<Canvas>,
    undo_stack: Vec<Canvas>,
    redo_stack: Vec<Canvas>,
}

impl App {
    pub fn new() -> Self {
        Self {
            canvas: Canvas::new(),
            cursor: Pos { x: 0, y: 0 },
            mode: Mode::Draw,
            should_quit: false,
            show_help: false,
            viewport: Rect::default(),
            viewport_origin: Pos { x: 0, y: 0 },
            selection_anchor: None,
            drag_origin: None,
            pan_drag: None,
            clipboard: None,
            floating: None,
            last_clip_bounds: None,
            paint_canvas_before: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn apply_canvas_edit(&mut self, edit: impl FnOnce(&mut Canvas)) {
        let before = self.canvas.clone();
        edit(&mut self.canvas);
        if self.canvas != before {
            self.undo_stack.push(before);
            if self.undo_stack.len() > UNDO_DEPTH_CAP {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    fn undo(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.canvas, previous);
        self.redo_stack.push(current);
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        let current = std::mem::replace(&mut self.canvas, next);
        self.undo_stack.push(current);
    }

    fn visible_bounds(&self) -> Bounds {
        if self.viewport.width == 0 || self.viewport.height == 0 {
            return Bounds {
                min_x: 0,
                max_x: self.canvas.width.saturating_sub(1),
                min_y: 0,
                max_y: self.canvas.height.saturating_sub(1),
            };
        }

        let min_x = self
            .viewport_origin
            .x
            .min(self.canvas.width.saturating_sub(1));
        let min_y = self
            .viewport_origin
            .y
            .min(self.canvas.height.saturating_sub(1));
        let max_x = (self.viewport_origin.x + self.viewport.width.saturating_sub(1) as usize)
            .min(self.canvas.width.saturating_sub(1));
        let max_y = (self.viewport_origin.y + self.viewport.height.saturating_sub(1) as usize)
            .min(self.canvas.height.saturating_sub(1));

        Bounds {
            min_x,
            max_x,
            min_y,
            max_y,
        }
    }

    fn clamp_cursor_to_visible_bounds(&mut self) {
        let bounds = self.visible_bounds();
        self.cursor.x = self.cursor.x.clamp(bounds.min_x, bounds.max_x);
        self.cursor.y = self.cursor.y.clamp(bounds.min_y, bounds.max_y);
    }

    fn move_left(&mut self) {
        let bounds = self.visible_bounds();
        if self.cursor.x > bounds.min_x {
            self.cursor.x -= 1;
        } else if self.cursor.y > bounds.min_y {
            self.cursor.y -= 1;
            self.cursor.x = bounds.max_x;
        }
    }

    fn move_right(&mut self) {
        let bounds = self.visible_bounds();
        if self.cursor.x < bounds.max_x {
            self.cursor.x += 1;
        } else if self.cursor.y < bounds.max_y {
            self.cursor.y += 1;
            self.cursor.x = bounds.min_x;
        }
    }

    fn move_up(&mut self) {
        let bounds = self.visible_bounds();
        if self.cursor.y > bounds.min_y {
            self.cursor.y -= 1;
        } else {
            self.cursor.y = bounds.max_y;
        }
    }

    fn move_down(&mut self) {
        let bounds = self.visible_bounds();
        if self.cursor.y < bounds.max_y {
            self.cursor.y += 1;
        } else {
            self.cursor.y = bounds.min_y;
        }
    }

    fn mouse_to_canvas(&self, col: u16, row: u16) -> Option<Pos> {
        let col = col as usize;
        let row = row as usize;
        let vx = self.viewport.x as usize;
        let vy = self.viewport.y as usize;
        let vw = self.viewport.width as usize;
        let vh = self.viewport.height as usize;

        if col >= vx && row >= vy && col < vx + vw && row < vy + vh {
            let cx = self.viewport_origin.x + col - vx;
            let cy = self.viewport_origin.y + row - vy;
            if cx < self.canvas.width && cy < self.canvas.height {
                return Some(Pos { x: cx, y: cy });
            }
        }
        None
    }

    fn viewport_contains(&self, col: u16, row: u16) -> bool {
        let col = col as usize;
        let row = row as usize;
        let vx = self.viewport.x as usize;
        let vy = self.viewport.y as usize;
        let vw = self.viewport.width as usize;
        let vh = self.viewport.height as usize;

        col >= vx && row >= vy && col < vx + vw && row < vy + vh
    }

    fn clamp_viewport_origin(&mut self) {
        let max_x = self
            .canvas
            .width
            .saturating_sub(self.viewport.width.max(1) as usize);
        let max_y = self
            .canvas
            .height
            .saturating_sub(self.viewport.height.max(1) as usize);
        self.viewport_origin.x = self.viewport_origin.x.min(max_x);
        self.viewport_origin.y = self.viewport_origin.y.min(max_y);
    }

    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.clamp_viewport_origin();
        self.clamp_cursor_to_visible_bounds();
    }

    fn pan_by(&mut self, dx: isize, dy: isize) {
        let next_x = self.viewport_origin.x.saturating_add_signed(dx);
        let next_y = self.viewport_origin.y.saturating_add_signed(dy);
        self.viewport_origin.x = next_x;
        self.viewport_origin.y = next_y;
        self.clamp_viewport_origin();
        self.clamp_cursor_to_visible_bounds();
    }

    fn begin_pan(&mut self, col: u16, row: u16) {
        self.pan_drag = Some(PanDrag {
            col,
            row,
            origin: self.viewport_origin,
        });
    }

    fn drag_pan(&mut self, col: u16, row: u16) {
        let Some(pan_drag) = self.pan_drag else {
            return;
        };
        let dx = pan_drag.col as i32 - col as i32;
        let dy = pan_drag.row as i32 - row as i32;
        self.viewport_origin.x = pan_drag.origin.x.saturating_add_signed(dx as isize);
        self.viewport_origin.y = pan_drag.origin.y.saturating_add_signed(dy as isize);
        self.clamp_viewport_origin();
        self.clamp_cursor_to_visible_bounds();
    }

    fn end_pan(&mut self) {
        self.pan_drag = None;
    }

    fn clamp_cursor(&mut self) {
        self.cursor.x = self.cursor.x.min(self.canvas.width.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(self.canvas.height.saturating_sub(1));
        self.clamp_cursor_to_visible_bounds();
    }

    fn begin_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.mode = Mode::Select;
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.mode = Mode::Draw;
    }

    fn selection(&self) -> Option<Selection> {
        self.selection_anchor.map(|anchor| Selection {
            anchor,
            cursor: self.cursor,
        })
    }

    fn selection_bounds(&self) -> Option<Bounds> {
        self.selection()
            .map(|selection| Bounds::from_points(selection.anchor, selection.cursor))
    }

    fn selection_or_cursor_bounds(&self) -> Bounds {
        self.selection_bounds()
            .unwrap_or_else(|| Bounds::single(self.cursor))
    }

    fn fill_bounds_on(canvas: &mut Canvas, bounds: Bounds, ch: char) {
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                canvas.set(Pos { x, y }, ch);
            }
        }
    }

    fn capture_bounds(&self, bounds: Bounds) -> Clipboard {
        let mut cells = Vec::with_capacity(bounds.width() * bounds.height());
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                cells.push(self.canvas.get(Pos { x, y }));
            }
        }
        Clipboard {
            width: bounds.width(),
            height: bounds.height(),
            cells,
        }
    }

    fn copy_selection_or_cell(&mut self) {
        if self.floating.is_some() {
            self.toggle_float_transparency();
            return;
        }
        let bounds = self.selection_or_cursor_bounds();
        if self.clipboard.is_some() && self.last_clip_bounds == Some(bounds) {
            self.enter_floating(bounds, false);
            return;
        }
        self.clipboard = Some(self.capture_bounds(bounds));
        self.last_clip_bounds = Some(bounds);
    }

    fn cut_selection_or_cell(&mut self) {
        if self.floating.is_some() {
            self.toggle_float_transparency();
            return;
        }
        let bounds = self.selection_or_cursor_bounds();
        if self.clipboard.is_some() && self.last_clip_bounds == Some(bounds) {
            self.enter_floating(bounds, false);
            return;
        }
        self.clipboard = Some(self.capture_bounds(bounds));
        self.last_clip_bounds = Some(bounds);
        self.apply_canvas_edit(|canvas| Self::fill_bounds_on(canvas, bounds, ' '));
    }

    fn enter_floating(&mut self, bounds: Bounds, transparent: bool) {
        if let Some(ref clipboard) = self.clipboard {
            self.floating = Some(FloatingSelection {
                clipboard: clipboard.clone(),
                transparent,
            });
            self.cursor = Pos {
                x: bounds.min_x,
                y: bounds.min_y,
            };
            self.clear_selection();
            self.last_clip_bounds = None;
        }
    }

    fn toggle_float_transparency(&mut self) {
        if let Some(ref mut floating) = self.floating {
            floating.transparent = !floating.transparent;
        }
    }

    fn stamp_floating(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        let pos = self.cursor;
        let transparent = floating.transparent;
        let clipboard = floating.clipboard.clone();
        self.apply_canvas_edit(|canvas| {
            for y in 0..clipboard.height {
                for x in 0..clipboard.width {
                    let ch = clipboard.get(x, y);
                    if transparent && ch == ' ' {
                        continue;
                    }
                    let target_x = pos.x + x;
                    let target_y = pos.y + y;
                    if target_x >= canvas.width || target_y >= canvas.height {
                        continue;
                    }
                    canvas.set(Pos { x: target_x, y: target_y }, ch);
                }
            }
        });
        self.floating = Some(floating);
    }

    fn stamp_onto_canvas(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        {
            let pos = self.cursor;
            let cb = &floating.clipboard;
            let transparent = floating.transparent;
            for y in 0..cb.height {
                for x in 0..cb.width {
                    let ch = cb.get(x, y);
                    if transparent && ch == ' ' {
                        continue;
                    }
                    let tx = pos.x + x;
                    let ty = pos.y + y;
                    if tx < self.canvas.width && ty < self.canvas.height {
                        self.canvas.set(Pos { x: tx, y: ty }, ch);
                    }
                }
            }
        }
        self.floating = Some(floating);
    }

    fn begin_paint_stroke(&mut self) {
        self.paint_canvas_before = Some(self.canvas.clone());
    }

    fn end_paint_stroke(&mut self) {
        if let Some(before) = self.paint_canvas_before.take() {
            if self.canvas != before {
                self.undo_stack.push(before);
                if self.undo_stack.len() > UNDO_DEPTH_CAP {
                    self.undo_stack.remove(0);
                }
                self.redo_stack.clear();
            }
        }
    }

    fn dismiss_floating(&mut self) {
        self.end_paint_stroke();
        self.floating = None;
    }

    fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.clipboard.clone() else {
            return;
        };

        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| {
            for y in 0..clipboard.height {
                for x in 0..clipboard.width {
                    let target_x = cursor.x + x;
                    let target_y = cursor.y + y;
                    if target_x >= canvas.width || target_y >= canvas.height {
                        continue;
                    }
                    canvas.set(
                        Pos {
                            x: target_x,
                            y: target_y,
                        },
                        clipboard.get(x, y),
                    );
                }
            }
        });
    }

    fn smart_fill(&mut self) {
        let bounds = self.selection_or_cursor_bounds();
        let ch = if bounds.width() == 1 && bounds.height() > 1 {
            '|'
        } else if bounds.height() == 1 && bounds.width() > 1 {
            '-'
        } else {
            '*'
        };
        self.apply_canvas_edit(|canvas| Self::fill_bounds_on(canvas, bounds, ch));
    }

    fn draw_border(&mut self) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };

        self.apply_canvas_edit(|canvas| {
            if bounds.width() == 1 && bounds.height() == 1 {
                canvas.set(
                    Pos {
                        x: bounds.min_x,
                        y: bounds.min_y,
                    },
                    '*',
                );
                return;
            }

            if bounds.height() == 1 {
                canvas.set(
                    Pos {
                        x: bounds.min_x,
                        y: bounds.min_y,
                    },
                    '.',
                );
                for x in (bounds.min_x + 1)..bounds.max_x {
                    canvas.set(Pos { x, y: bounds.min_y }, '-');
                }
                canvas.set(
                    Pos {
                        x: bounds.max_x,
                        y: bounds.min_y,
                    },
                    '.',
                );
                return;
            }

            if bounds.width() == 1 {
                canvas.set(
                    Pos {
                        x: bounds.min_x,
                        y: bounds.min_y,
                    },
                    '.',
                );
                for y in (bounds.min_y + 1)..bounds.max_y {
                    canvas.set(Pos { x: bounds.min_x, y }, '|');
                }
                canvas.set(
                    Pos {
                        x: bounds.min_x,
                        y: bounds.max_y,
                    },
                    '`',
                );
                return;
            }

            canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.min_y,
                },
                '.',
            );
            canvas.set(
                Pos {
                    x: bounds.max_x,
                    y: bounds.min_y,
                },
                '.',
            );
            canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.max_y,
                },
                '`',
            );
            canvas.set(
                Pos {
                    x: bounds.max_x,
                    y: bounds.max_y,
                },
                '\'',
            );

            for x in (bounds.min_x + 1)..bounds.max_x {
                canvas.set(Pos { x, y: bounds.min_y }, '-');
                canvas.set(Pos { x, y: bounds.max_y }, '-');
            }

            for y in (bounds.min_y + 1)..bounds.max_y {
                canvas.set(Pos { x: bounds.min_x, y }, '|');
                canvas.set(Pos { x: bounds.max_x, y }, '|');
            }
        });
    }

    fn fill_selection_or_cell(&mut self, ch: char) {
        let bounds = self.selection_or_cursor_bounds();
        self.apply_canvas_edit(|canvas| Self::fill_bounds_on(canvas, bounds, ch));
    }

    fn insert_char(&mut self, ch: char) {
        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| canvas.set(cursor, ch));
        self.move_right();
    }

    fn paste_text_block(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let origin = self.cursor;
        self.apply_canvas_edit(|canvas| {
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
                            canvas.set(Pos { x, y }, ch);
                        }
                        x += 1;
                    }
                }
            }
        });
    }

    fn backspace(&mut self) {
        self.move_left();
        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| canvas.clear(cursor));
    }

    fn delete_at_cursor(&mut self) {
        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| canvas.clear(cursor));
    }

    fn push_left(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_left(y, x));
    }

    fn push_down(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_down(x, y));
    }

    fn push_up(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_up(x, y));
    }

    fn push_right(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_right(y, x));
    }

    fn pull_from_left(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_left(y, x));
    }

    fn pull_from_down(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_down(x, y));
    }

    fn pull_from_up(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_up(x, y));
    }

    fn pull_from_right(&mut self) {
        let x = self.cursor.x;
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_right(y, x));
    }

    fn transpose_selection_corner(&mut self) -> bool {
        if !self.mode.is_selecting() {
            return false;
        }

        let Some(anchor) = self.selection_anchor else {
            return false;
        };

        self.selection_anchor = Some(self.cursor);
        self.cursor = anchor;
        true
    }

    fn handle_control_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('h') => self.push_left(),
            KeyCode::Char('j') => self.push_down(),
            KeyCode::Char('k') => self.push_up(),
            KeyCode::Char('l') => self.push_right(),
            KeyCode::Char('y') => self.pull_from_left(),
            KeyCode::Char('u') => self.pull_from_down(),
            KeyCode::Tab | KeyCode::Char('i') => self.pull_from_up(),
            KeyCode::Char('o') => self.pull_from_right(),
            KeyCode::Char('c') => self.copy_selection_or_cell(),
            KeyCode::Char('x') => self.cut_selection_or_cell(),
            KeyCode::Char('v') => self.paste_clipboard(),
            KeyCode::Char('b') => self.draw_border(),
            KeyCode::Char('r') => self.redo(),
            KeyCode::Char('t') => return self.transpose_selection_corner(),
            KeyCode::Char('z') => self.undo(),
            KeyCode::Char(' ') | KeyCode::Null => self.smart_fill(),
            _ => return false,
        }

        true
    }

    fn handle_alt_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Left => self.pan_by(-1, 0),
            KeyCode::Right => self.pan_by(1, 0),
            KeyCode::Up => self.pan_by(0, -1),
            KeyCode::Down => self.pan_by(0, 1),
            _ => return false,
        }

        true
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.should_quit = true;
                    return;
                }

                if self.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::F(1) => self.show_help = false,
                        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.show_help = false
                        }
                        _ => {}
                    }
                    return;
                }

                if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.show_help = !self.show_help;
                } else if key.code == KeyCode::F(1) {
                    self.show_help = !self.show_help;
                } else {
                    self.handle_key(key);
                }
            }
            Event::Mouse(mouse) => {
                if self.show_help {
                    return;
                }

                let canvas_pos = self.mouse_to_canvas(mouse.column, mouse.row);

                if self.floating.is_some() {
                    match mouse.kind {
                        MouseEventKind::Moved => {
                            if let Some(pos) = canvas_pos {
                                self.cursor = pos;
                            }
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(pos) = canvas_pos {
                                self.cursor = pos;
                                self.begin_paint_stroke();
                                self.stamp_onto_canvas();
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(pos) = canvas_pos {
                                self.cursor = pos;
                                self.stamp_onto_canvas();
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            self.end_paint_stroke();
                        }
                        MouseEventKind::Down(MouseButton::Right) => {
                            self.dismiss_floating();
                        }
                        _ => {}
                    }
                } else {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Right) => {
                            if self.viewport_contains(mouse.column, mouse.row) {
                                self.begin_pan(mouse.column, mouse.row);
                            }
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(pos) = canvas_pos {
                                let extend_selection =
                                    mouse.modifiers.contains(KeyModifiers::ALT)
                                        && self.selection_anchor.is_some();

                                if extend_selection {
                                    if let Some(anchor) = self.selection_anchor {
                                        self.mode = Mode::Select;
                                        self.cursor = pos;
                                        self.drag_origin = Some(anchor);
                                    }
                                } else {
                                    if self.mode.is_selecting() {
                                        self.clear_selection();
                                    }
                                    self.cursor = pos;
                                    self.drag_origin = Some(pos);
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let (Some(origin), Some(pos)) = (self.drag_origin, canvas_pos) {
                                if pos != origin || self.mode.is_selecting() {
                                    self.selection_anchor = Some(origin);
                                    self.mode = Mode::Select;
                                    self.cursor = pos;
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Right) => {
                            self.drag_pan(mouse.column, mouse.row);
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            self.drag_origin = None;
                        }
                        MouseEventKind::Up(MouseButton::Right) => {
                            self.end_pan();
                        }
                        _ => {}
                    }
                }
            }
            Event::Paste(data) => {
                if self.show_help {
                    return;
                }
                self.paste_text_block(&data);
            }
            _ => {}
        }
        self.clamp_cursor();
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.floating.is_some() {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let alt = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::META);

            match key.code {
                KeyCode::Char('c') | KeyCode::Char('x') if ctrl => {
                    self.toggle_float_transparency();
                    return;
                }
                KeyCode::Char('v') if ctrl => {
                    self.stamp_floating();
                    return;
                }
                KeyCode::Esc => {
                    self.dismiss_floating();
                    return;
                }
                KeyCode::Up if !ctrl && !alt => {
                    self.cursor.y = self.cursor.y.saturating_sub(1);
                    return;
                }
                KeyCode::Down if !ctrl && !alt => {
                    if self.cursor.y < self.canvas.height.saturating_sub(1) {
                        self.cursor.y += 1;
                    }
                    return;
                }
                KeyCode::Left if !ctrl && !alt => {
                    self.cursor.x = self.cursor.x.saturating_sub(1);
                    return;
                }
                KeyCode::Right if !ctrl && !alt => {
                    if self.cursor.x < self.canvas.width.saturating_sub(1) {
                        self.cursor.x += 1;
                    }
                    return;
                }
                _ if alt => {} // Keep floating, let alt handler process (e.g. panning)
                _ => {
                    self.dismiss_floating();
                }
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && self.handle_control_key(key) {
            return;
        }

        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::META)
            && self.handle_alt_key(key)
        {
            return;
        }

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let is_move = matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
        );

        if is_move && shift {
            self.begin_selection();
            match key.code {
                KeyCode::Up => self.move_up(),
                KeyCode::Down => self.move_down(),
                KeyCode::Left => self.move_left(),
                KeyCode::Right => self.move_right(),
                KeyCode::Home => self.cursor.x = self.visible_bounds().min_x,
                KeyCode::End => self.cursor.x = self.visible_bounds().max_x,
                KeyCode::PageUp => self.cursor.y = self.visible_bounds().min_y,
                KeyCode::PageDown => self.cursor.y = self.visible_bounds().max_y,
                _ => {}
            }
            return;
        }

        if is_move && self.mode.is_selecting() {
            self.clear_selection();
        }

        match key.code {
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.cursor.x = self.visible_bounds().min_x,
            KeyCode::End => self.cursor.x = self.visible_bounds().max_x,
            KeyCode::PageUp => self.cursor.y = self.visible_bounds().min_y,
            KeyCode::PageDown => self.cursor.y = self.visible_bounds().max_y,
            KeyCode::Enter => self.move_down(),
            KeyCode::Esc => self.clear_selection(),
            _ if self.mode.is_selecting() && self.selection_anchor.is_some() => match key.code {
                KeyCode::Char(ch) => self.fill_selection_or_cell(ch),
                KeyCode::Backspace | KeyCode::Delete => self.fill_selection_or_cell(' '),
                _ => {}
            },
            _ => match key.code {
                KeyCode::Char(ch) => {
                    self.insert_char(ch);
                }
                KeyCode::Backspace => self.backspace(),
                KeyCode::Delete => self.delete_at_cursor(),
                _ => {}
            },
        }
    }

    pub fn is_selected(&self, pos: Pos) -> bool {
        let Some(bounds) = self.selection_bounds() else {
            return false;
        };

        pos.x >= bounds.min_x
            && pos.x <= bounds.max_x
            && pos.y >= bounds.min_y
            && pos.y <= bounds.max_y
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Mode};
    use crate::canvas::Pos;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;

    #[test]
    fn smart_fill_matches_selection_shape() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 2, y: 1 });
        app.cursor = Pos { x: 2, y: 3 };
        app.mode = Mode::Select;

        app.smart_fill();

        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 3 }), '|');
    }

    #[test]
    fn border_draws_ascii_frame() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 4, y: 3 };
        app.mode = Mode::Select;

        app.draw_border();

        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 4, y: 1 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 3 }), '`');
        assert_eq!(app.canvas.get(Pos { x: 4, y: 3 }), '\'');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), '-');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 2 }), '|');
    }

    #[test]
    fn cut_and_paste_work_for_selection() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.canvas.set(Pos { x: 1, y: 2 }, 'C');
        app.canvas.set(Pos { x: 2, y: 2 }, 'D');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 2 };
        app.mode = Mode::Select;

        app.cut_selection_or_cell();

        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), ' ');

        app.clear_selection();
        app.cursor = Pos { x: 5, y: 4 };
        app.paste_clipboard();

        assert_eq!(app.canvas.get(Pos { x: 5, y: 4 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 4 }), 'B');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), 'C');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 5 }), 'D');
    }

    #[test]
    fn undo_and_redo_restore_canvas_state() {
        let mut app = App::new();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), 'B');

        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), 'B');
    }

    #[test]
    fn new_edit_clears_redo_history() {
        let mut app = App::new();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 0 }), 'C');
    }

    #[test]
    fn bracketed_paste_preserves_multiline_shape() {
        let mut app = App::new();
        app.cursor = Pos { x: 3, y: 4 };

        app.handle_event(Event::Paste(".---.\n|   |\n`---'".to_string()));

        assert_eq!(app.canvas.get(Pos { x: 3, y: 4 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 4 }), '.');
        assert_eq!(app.canvas.get(Pos { x: 3, y: 5 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 5 }), '|');
        assert_eq!(app.canvas.get(Pos { x: 3, y: 6 }), '`');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 6 }), '\'');
    }

    #[test]
    fn alt_arrow_keys_pan_viewport() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));

        assert_eq!(app.viewport_origin, Pos { x: 1, y: 1 });
    }

    #[test]
    fn right_drag_pans_viewport() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(app.viewport_origin, Pos { x: 3, y: 1 });
    }

    #[test]
    fn mouse_mapping_respects_viewport_origin() {
        let mut app = App::new();
        app.set_viewport(Rect::new(4, 3, 10, 5));
        app.viewport_origin = Pos { x: 12, y: 7 };

        assert_eq!(app.mouse_to_canvas(6, 4), Some(Pos { x: 14, y: 8 }));
    }

    #[test]
    fn cursor_is_clamped_into_viewport_after_pan() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 10, 5));
        app.cursor = Pos { x: 2, y: 2 };

        app.pan_by(20, 10);

        assert_eq!(app.viewport_origin, Pos { x: 20, y: 10 });
        assert_eq!(app.cursor, Pos { x: 20, y: 10 });
    }

    #[test]
    fn resize_clamps_cursor_to_nearest_visible_position() {
        let mut app = App::new();
        app.viewport_origin = Pos { x: 10, y: 10 };
        app.cursor = Pos { x: 18, y: 14 };

        app.set_viewport(Rect::new(0, 0, 4, 3));

        assert_eq!(app.cursor, Pos { x: 13, y: 12 });
    }

    #[test]
    fn cursor_movement_wraps_within_visible_bounds() {
        let mut app = App::new();
        app.viewport_origin = Pos { x: 10, y: 20 };
        app.set_viewport(Rect::new(0, 0, 4, 3));
        app.cursor = Pos { x: 13, y: 20 };

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 10, y: 21 });

        app.cursor = Pos { x: 10, y: 22 };
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.cursor, Pos { x: 10, y: 20 });
    }

    #[test]
    fn ctrl_q_quits_even_when_help_is_open() {
        let mut app = App::new();
        app.show_help = true;

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.should_quit);
        assert!(app.show_help);
    }

    #[test]
    fn alt_click_extends_existing_selection() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.selection_anchor = Some(Pos { x: 2, y: 3 });
        app.cursor = Pos { x: 5, y: 6 };
        app.mode = Mode::Select;

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::ALT,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::ALT,
        }));

        assert_eq!(app.selection_anchor, Some(Pos { x: 2, y: 3 }));
        assert_eq!(app.cursor, Pos { x: 8, y: 7 });
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn ctrl_t_transposes_active_selection_corner() {
        let mut app = App::new();
        app.selection_anchor = Some(Pos { x: 2, y: 3 });
        app.cursor = Pos { x: 8, y: 7 };
        app.mode = Mode::Select;

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

        assert_eq!(app.selection_anchor, Some(Pos { x: 8, y: 7 }));
        assert_eq!(app.cursor, Pos { x: 2, y: 3 });
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn double_copy_enters_floating_mode() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        // First copy: captures clipboard
        app.copy_selection_or_cell();
        assert!(app.clipboard.is_some());
        assert!(app.floating.is_none());

        // Second copy on same selection: enters floating
        app.copy_selection_or_cell();
        assert!(app.floating.is_some());
        assert_eq!(app.cursor, Pos { x: 1, y: 1 });
        assert!(!app.mode.is_selecting());
        // Original content untouched
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), 'B');
    }

    #[test]
    fn double_cut_enters_floating_mode() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'X');
        app.canvas.set(Pos { x: 2, y: 1 }, 'Y');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        // First cut: captures and clears
        app.cut_selection_or_cell();
        assert!(app.clipboard.is_some());
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), ' ');

        // Second cut on same selection: enters floating (no double-clear)
        app.cut_selection_or_cell();
        assert!(app.floating.is_some());
    }

    #[test]
    fn stamp_floating_writes_to_canvas() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 1, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // enter floating

        // Move float to new position
        app.cursor = Pos { x: 5, y: 3 };

        // Stamp via Ctrl+V
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        // Float persists for repeated stamping
        assert!(app.floating.is_some());
        assert_eq!(app.canvas.get(Pos { x: 5, y: 3 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 3 }), 'B');
    }

    #[test]
    fn esc_dismisses_float_without_stamping() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'Z');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // enter floating

        app.cursor = Pos { x: 5, y: 5 };

        // Dismiss with Esc
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.floating.is_none());
        // Clipboard still intact
        assert!(app.clipboard.is_some());
        // Nothing stamped at (5,5)
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), ' ');
    }

    #[test]
    fn arrow_keys_nudge_floating_position() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 3, y: 3 }, 'Q');
        app.selection_anchor = Some(Pos { x: 3, y: 3 });
        app.cursor = Pos { x: 3, y: 3 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // floating at (3,3)

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert!(app.floating.is_some());
        assert_eq!(app.cursor, Pos { x: 4, y: 4 });
    }

    #[test]
    fn mouse_click_stamps_floating() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.canvas.set(Pos { x: 0, y: 0 }, 'M');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // enter floating

        // Left click stamps at click position
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 7,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 7,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }));

        // Float persists for repeated stamping
        assert!(app.floating.is_some());
        assert_eq!(app.canvas.get(Pos { x: 7, y: 4 }), 'M');
    }

    #[test]
    fn different_bounds_copy_does_not_float() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');

        // Copy single cell
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();

        // Copy different cell
        app.cursor = Pos { x: 1, y: 0 };
        app.copy_selection_or_cell();

        assert!(app.floating.is_none());
    }

    #[test]
    fn triple_copy_toggles_transparency() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell(); // 1st: clipboard
        app.copy_selection_or_cell(); // 2nd: float (opaque)

        assert!(app.floating.is_some());
        assert!(!app.floating.as_ref().unwrap().transparent);

        // 3rd copy while floating: toggle to transparent
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.floating.is_some());
        assert!(app.floating.as_ref().unwrap().transparent);

        // 4th: toggle back to opaque
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn transparent_stamp_preserves_underlying_content() {
        let mut app = App::new();
        // Place "A B" in clipboard (A, space, B)
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 2, y: 0 }, 'B');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 2, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // opaque float

        // Toggle to transparent
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);

        // Place existing content at stamp target
        app.canvas.set(Pos { x: 5, y: 5 }, 'Z');

        // Move float and stamp
        app.cursor = Pos { x: 4, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        // A stamped at (4,5), space at (5,5) skipped so Z preserved, B at (6,5)
        assert_eq!(app.canvas.get(Pos { x: 4, y: 5 }), 'A');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), 'Z');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 5 }), 'B');
    }

    #[test]
    fn drag_paints_like_brush_with_single_undo() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));
        app.canvas.set(Pos { x: 0, y: 0 }, 'X');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // floating

        // Paint stroke: click, drag to two positions, release
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 7,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 7,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));

        // All three positions stamped
        assert_eq!(app.canvas.get(Pos { x: 3, y: 2 }), 'X');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 2 }), 'X');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 2 }), 'X');

        // Float still active
        assert!(app.floating.is_some());

        // Single undo reverts the entire paint stroke
        app.undo();
        assert_eq!(app.canvas.get(Pos { x: 3, y: 2 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 5, y: 2 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 7, y: 2 }), ' ');
    }

    #[test]
    fn repeated_ctrl_v_stamps_create_separate_undos() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'Q');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.copy_selection_or_cell(); // floating

        // Stamp at two positions
        app.cursor = Pos { x: 3, y: 3 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        app.cursor = Pos { x: 6, y: 6 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        assert_eq!(app.canvas.get(Pos { x: 3, y: 3 }), 'Q');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), 'Q');

        // Undo only the second stamp
        app.undo();
        assert_eq!(app.canvas.get(Pos { x: 3, y: 3 }), 'Q');
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), ' ');
    }
}
