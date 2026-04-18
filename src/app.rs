use std::io;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::{clipboard::CopyToClipboard, execute};
use rand::seq::SliceRandom;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::canvas::{Canvas, CellValue, Pos};
use crate::emoji;
use crate::theme;

const UNDO_DEPTH_CAP: usize = 500;
pub const SWATCH_CAPACITY: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwatchZone {
    Body,
    Pin,
}
const LOCAL_USER_NAMES: &[&str] = &[
    "mevanlc",
    "mat",
    "averylongusernamethatgetstruncated",
    "Hades",
    "graybeard",
];

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
enum SelectionShape {
    #[default]
    Rect,
    Ellipse,
}

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
    shape: SelectionShape,
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

    fn normalized_for_canvas(self, canvas: &Canvas) -> Self {
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
    fn bounds(self) -> Bounds {
        Bounds::from_points(self.anchor, self.cursor)
    }

    fn contains(self, pos: Pos) -> bool {
        let bounds = self.bounds();
        if pos.x < bounds.min_x || pos.x > bounds.max_x || pos.y < bounds.min_y || pos.y > bounds.max_y
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
    pub fn get(&self, x: usize, y: usize) -> Option<CellValue> {
        self.cells[y * self.width + x]
    }
}

#[derive(Debug, Clone)]
pub struct Swatch {
    pub clipboard: Clipboard,
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpTab {
    #[default]
    Common,
    Advanced,
}

impl HelpTab {
    pub fn toggle(self) -> Self {
        match self {
            HelpTab::Common => HelpTab::Advanced,
            HelpTab::Advanced => HelpTab::Common,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FloatingSelection {
    pub clipboard: Clipboard,
    pub transparent: bool,
    pub source_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct PanDrag {
    col: u16,
    row: u16,
    origin: Pos,
}

#[derive(Debug, Clone)]
struct UserSession {
    cursor: Pos,
    mode: Mode,
    show_help: bool,
    help_tab: HelpTab,
    emoji_picker_open: bool,
    viewport: Rect,
    viewport_origin: Pos,
    selection_anchor: Option<Pos>,
    selection_shape: SelectionShape,
    drag_origin: Option<Pos>,
    pan_drag: Option<PanDrag>,
    swatches: [Option<Swatch>; SWATCH_CAPACITY],
    floating: Option<FloatingSelection>,
    emoji_picker_state: emoji::EmojiPickerState,
    paint_canvas_before: Option<Canvas>,
    paint_stroke_anchor: Option<Pos>,
    paint_stroke_last: Option<Pos>,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            cursor: Pos { x: 0, y: 0 },
            mode: Mode::Draw,
            show_help: false,
            help_tab: HelpTab::default(),
            emoji_picker_open: false,
            viewport: Rect::default(),
            viewport_origin: Pos { x: 0, y: 0 },
            selection_anchor: None,
            selection_shape: SelectionShape::Rect,
            drag_origin: None,
            pan_drag: None,
            swatches: Default::default(),
            floating: None,
            emoji_picker_state: emoji::EmojiPickerState::default(),
            paint_canvas_before: None,
            paint_stroke_anchor: None,
            paint_stroke_last: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalUser {
    pub name: String,
    pub color: Color,
    session: UserSession,
}

pub struct App {
    pub canvas: Canvas,
    pub cursor: Pos,
    pub mode: Mode,
    pub should_quit: bool,
    pub show_help: bool,
    pub help_tab: HelpTab,
    pub emoji_picker_open: bool,
    pub viewport: Rect,
    pub viewport_origin: Pos,
    pub selection_anchor: Option<Pos>,
    selection_shape: SelectionShape,
    drag_origin: Option<Pos>,
    pan_drag: Option<PanDrag>,
    pub swatches: [Option<Swatch>; SWATCH_CAPACITY],
    pub floating: Option<FloatingSelection>,
    pub emoji_picker_state: emoji::EmojiPickerState,
    pub icon_catalog: Option<emoji::catalog::IconCatalogData>,
    pub swatch_body_hits: [Option<Rect>; SWATCH_CAPACITY],
    pub swatch_pin_hits: [Option<Rect>; SWATCH_CAPACITY],
    pub help_tab_hits: [Option<(HelpTab, Rect)>; 2],
    paint_canvas_before: Option<Canvas>,
    paint_stroke_anchor: Option<Pos>,
    paint_stroke_last: Option<Pos>,
    undo_stack: Vec<Canvas>,
    redo_stack: Vec<Canvas>,
    users: Vec<LocalUser>,
    active_user_idx: usize,
}

impl App {
    pub fn new() -> Self {
        let default_session = UserSession::default();
        let mut used_colors = Vec::with_capacity(LOCAL_USER_NAMES.len());
        let users = LOCAL_USER_NAMES
            .iter()
            .map(|name| {
                let color = random_available_user_color(&used_colors);
                used_colors.push(color);
                LocalUser {
                    name: (*name).to_string(),
                    color,
                    session: default_session.clone(),
                }
            })
            .collect();
        let current_session = default_session;
        Self {
            canvas: Canvas::new(),
            cursor: current_session.cursor,
            mode: current_session.mode,
            should_quit: false,
            show_help: current_session.show_help,
            help_tab: current_session.help_tab,
            emoji_picker_open: current_session.emoji_picker_open,
            viewport: current_session.viewport,
            viewport_origin: current_session.viewport_origin,
            selection_anchor: current_session.selection_anchor,
            selection_shape: current_session.selection_shape,
            drag_origin: current_session.drag_origin,
            pan_drag: current_session.pan_drag,
            swatches: current_session.swatches,
            floating: current_session.floating,
            emoji_picker_state: current_session.emoji_picker_state,
            icon_catalog: None,
            swatch_body_hits: [None; SWATCH_CAPACITY],
            swatch_pin_hits: [None; SWATCH_CAPACITY],
            help_tab_hits: [None; 2],
            paint_canvas_before: current_session.paint_canvas_before,
            paint_stroke_anchor: current_session.paint_stroke_anchor,
            paint_stroke_last: current_session.paint_stroke_last,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            users,
            active_user_idx: 0,
        }
    }

    fn current_session(&self) -> UserSession {
        UserSession {
            cursor: self.cursor,
            mode: self.mode,
            show_help: self.show_help,
            help_tab: self.help_tab,
            emoji_picker_open: self.emoji_picker_open,
            viewport: self.viewport,
            viewport_origin: self.viewport_origin,
            selection_anchor: self.selection_anchor,
            selection_shape: self.selection_shape,
            drag_origin: self.drag_origin,
            pan_drag: self.pan_drag,
            swatches: self.swatches.clone(),
            floating: self.floating.clone(),
            emoji_picker_state: self.emoji_picker_state.clone(),
            paint_canvas_before: self.paint_canvas_before.clone(),
            paint_stroke_anchor: self.paint_stroke_anchor,
            paint_stroke_last: self.paint_stroke_last,
        }
    }

    fn load_session(&mut self, session: UserSession) {
        self.cursor = session.cursor;
        self.mode = session.mode;
        self.show_help = session.show_help;
        self.help_tab = session.help_tab;
        self.emoji_picker_open = session.emoji_picker_open;
        self.viewport = session.viewport;
        self.viewport_origin = session.viewport_origin;
        self.selection_anchor = session.selection_anchor;
        self.selection_shape = session.selection_shape;
        self.drag_origin = session.drag_origin;
        self.pan_drag = session.pan_drag;
        self.swatches = session.swatches;
        self.floating = session.floating;
        self.emoji_picker_state = session.emoji_picker_state;
        self.paint_canvas_before = session.paint_canvas_before;
        self.paint_stroke_anchor = session.paint_stroke_anchor;
        self.paint_stroke_last = session.paint_stroke_last;
        self.swatch_body_hits = [None; SWATCH_CAPACITY];
        self.swatch_pin_hits = [None; SWATCH_CAPACITY];
    }

    pub(crate) fn sync_active_user_slot(&mut self) {
        let session = self.current_session();
        if let Some(user) = self.users.get_mut(self.active_user_idx) {
            user.session = session;
        }
    }

    fn switch_active_user(&mut self, delta: isize) {
        if self.users.is_empty() {
            return;
        }

        self.sync_active_user_slot();
        let len = self.users.len() as isize;
        self.active_user_idx = (self.active_user_idx as isize + delta).rem_euclid(len) as usize;
        let next_session = self.users[self.active_user_idx].session.clone();
        self.load_session(next_session);
        self.clamp_cursor();
    }

    pub fn users(&self) -> &[LocalUser] {
        &self.users
    }

    pub fn active_user_index(&self) -> usize {
        self.active_user_idx
    }

    pub fn active_user_color(&self) -> Color {
        self.users[self.active_user_idx].color
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

    fn swatch_hit(&self, col: u16, row: u16) -> Option<(usize, SwatchZone)> {
        for (idx, maybe_rect) in self.swatch_pin_hits.iter().enumerate() {
            let Some(rect) = maybe_rect else { continue };
            if rect_contains(rect, col, row) {
                return Some((idx, SwatchZone::Pin));
            }
        }
        for (idx, maybe_rect) in self.swatch_body_hits.iter().enumerate() {
            let Some(rect) = maybe_rect else { continue };
            if rect_contains(rect, col, row) {
                return Some((idx, SwatchZone::Body));
            }
        }
        None
    }

    fn help_tab_hit(&self, col: u16, row: u16) -> Option<HelpTab> {
        for maybe in self.help_tab_hits.iter() {
            let Some((tab, rect)) = maybe else { continue };
            if rect_contains(rect, col, row) {
                return Some(*tab);
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

    fn begin_selection_with_shape(&mut self, shape: SelectionShape) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.selection_shape = shape;
        self.mode = Mode::Select;
    }

    fn begin_selection(&mut self) {
        self.begin_selection_with_shape(SelectionShape::Rect);
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_shape = SelectionShape::Rect;
        self.mode = Mode::Draw;
    }

    fn selection(&self) -> Option<Selection> {
        self.selection_anchor.map(|anchor| Selection {
            anchor,
            cursor: self.cursor,
            shape: self.selection_shape,
        })
    }

    fn selection_bounds(&self) -> Option<Bounds> {
        self.selection().map(Selection::bounds)
    }

    fn selection_or_cursor_bounds(&self) -> Bounds {
        self.selection_bounds()
            .unwrap_or_else(|| Bounds::single(self.cursor))
    }

    fn full_canvas_bounds(&self) -> Bounds {
        Bounds {
            min_x: 0,
            max_x: self.canvas.width.saturating_sub(1),
            min_y: 0,
            max_y: self.canvas.height.saturating_sub(1),
        }
    }

    fn system_clipboard_bounds(&self) -> Bounds {
        self.selection_bounds()
            .unwrap_or_else(|| self.full_canvas_bounds())
            .normalized_for_canvas(&self.canvas)
    }

    fn fill_bounds_on(canvas: &mut Canvas, bounds: Bounds, ch: char, fg: Color) {
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

    fn fill_selection_on(
        canvas: &mut Canvas,
        selection: Selection,
        bounds: Bounds,
        ch: char,
        fg: Color,
    ) {
        if selection.shape == SelectionShape::Rect {
            Self::fill_bounds_on(canvas, bounds, ch, fg);
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
            Some(Pos { x: pos.x + 1, y: pos.y }),
            pos.y.checked_sub(1).map(|y| Pos { x: pos.x, y }),
            Some(Pos { x: pos.x, y: pos.y + 1 }),
        ];
        neighbors
            .into_iter()
            .flatten()
            .any(|neighbor| !selection.contains(neighbor))
    }

    fn draw_selection_border_on(
        canvas: &mut Canvas,
        selection: Selection,
        bounds: Bounds,
        color: Color,
    ) {
        if selection.shape == SelectionShape::Rect {
            return;
        }

        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                let pos = Pos { x, y };
                if selection.contains(pos) && Self::selection_has_unselected_neighbor(selection, pos)
                {
                    canvas.set_colored(pos, '*', color);
                }
            }
        }
    }

    fn capture_bounds(&self, bounds: Bounds) -> Clipboard {
        let mut cells = Vec::with_capacity(bounds.width() * bounds.height());
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                cells.push(self.canvas.cell(Pos { x, y }));
            }
        }
        Clipboard {
            width: bounds.width(),
            height: bounds.height(),
            cells,
        }
    }

    fn capture_selection(&self, selection: Selection) -> Clipboard {
        let bounds = selection.bounds().normalized_for_canvas(&self.canvas);
        let mut cells = Vec::with_capacity(bounds.width() * bounds.height());
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                let pos = Pos { x, y };
                let include = selection.contains(pos)
                    || self
                        .canvas
                        .glyph_origin(pos)
                        .is_some_and(|origin| selection.contains(origin));
                cells.push(include.then(|| self.canvas.cell(pos)).flatten());
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
            return;
        }
        let clipboard = match self.selection() {
            Some(selection) => self.capture_selection(selection),
            None => {
                let bounds = self
                    .selection_or_cursor_bounds()
                    .normalized_for_canvas(&self.canvas);
                self.capture_bounds(bounds)
            }
        };
        self.push_swatch(clipboard);
    }

    fn export_bounds_as_text(&self, bounds: Bounds) -> String {
        let mut text = String::with_capacity(bounds.width() * bounds.height() + bounds.height());
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                match self.canvas.cell(Pos { x, y }) {
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

    fn export_selection_as_text(&self, selection: Selection) -> String {
        let bounds = selection.bounds().normalized_for_canvas(&self.canvas);
        let mut text = String::with_capacity(bounds.width() * bounds.height() + bounds.height());
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                let pos = Pos { x, y };
                if selection.contains(pos) {
                    match self.canvas.cell(pos) {
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

    fn export_system_clipboard_text(&self) -> String {
        match self.selection() {
            Some(selection) => self.export_selection_as_text(selection),
            None => self.export_bounds_as_text(self.system_clipboard_bounds()),
        }
    }

    fn copy_to_system_clipboard(&self) {
        let text = self.export_system_clipboard_text();
        let _ = execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text));
    }

    fn cut_selection_or_cell(&mut self) {
        if self.floating.is_some() {
            return;
        }
        let selection = self.selection();
        let bounds = self
            .selection_or_cursor_bounds()
            .normalized_for_canvas(&self.canvas);
        let clipboard = selection
            .map(|selection| self.capture_selection(selection))
            .unwrap_or_else(|| self.capture_bounds(bounds));
        self.push_swatch(clipboard);
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| match selection {
            Some(selection) => Self::fill_selection_on(canvas, selection, bounds, ' ', color),
            None => Self::fill_bounds_on(canvas, bounds, ' ', color),
        });
    }

    fn push_swatch(&mut self, clipboard: Clipboard) {
        let unpinned_slots: Vec<usize> = (0..SWATCH_CAPACITY)
            .filter(|&i| !matches!(&self.swatches[i], Some(s) if s.pinned))
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
    fn populated_swatch_count(&self) -> usize {
        self.swatches.iter().filter(|s| s.is_some()).count()
    }

    pub fn toggle_pin(&mut self, idx: usize) {
        if idx >= SWATCH_CAPACITY {
            return;
        }
        if let Some(swatch) = self.swatches[idx].as_mut() {
            swatch.pinned = !swatch.pinned;
        }
    }

    pub fn activate_swatch(&mut self, idx: usize) {
        if idx >= SWATCH_CAPACITY {
            return;
        }
        let Some(swatch) = self.swatches[idx].as_ref() else {
            return;
        };
        match self.floating.as_mut() {
            Some(floating) if floating.source_index == Some(idx) => {
                floating.transparent = !floating.transparent;
            }
            _ => {
                let clipboard = swatch.clipboard.clone();
                self.end_paint_stroke();
                self.floating = Some(FloatingSelection {
                    clipboard,
                    transparent: false,
                    source_index: Some(idx),
                });
                self.clear_selection();
            }
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
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| {
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
            let color = self.active_user_color();
            for y in 0..cb.height {
                for x in 0..cb.width {
                    let tx = pos.x + x;
                    let ty = pos.y + y;
                    if tx < self.canvas.width && ty < self.canvas.height {
                        let target = Pos { x: tx, y: ty };
                        match cb.get(x, y) {
                            Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => {
                                let _ = self.canvas.put_glyph_colored(target, ch, color);
                            }
                            Some(CellValue::WideCont) => {}
                            None if !floating.transparent => self.canvas.clear(target),
                            None => {}
                        }
                    }
                }
            }
        }
        self.floating = Some(floating);
    }

    fn begin_paint_stroke(&mut self) {
        self.paint_canvas_before = Some(self.canvas.clone());
        self.paint_stroke_anchor = Some(self.cursor);
        self.paint_stroke_last = None;
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
        self.paint_stroke_anchor = None;
        self.paint_stroke_last = None;
    }

    fn dismiss_floating(&mut self) {
        self.end_paint_stroke();
        self.floating = None;
    }

    fn floating_brush_width(&self) -> usize {
        self.floating
            .as_ref()
            .map(|floating| floating.clipboard.width.max(1))
            .unwrap_or(1)
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

    fn paint_floating_at_cursor(&mut self) {
        self.stamp_onto_canvas();
        self.paint_stroke_last = Some(self.cursor);
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

    fn paint_floating_diagonal_segment(&mut self, start: Pos, end: Pos, brush_width: usize) {
        let mut last_stamped = start;
        for point in Self::line_points(start, end).into_iter().skip(1) {
            let should_stamp =
                point.y != last_stamped.y || point.x.abs_diff(last_stamped.x) >= brush_width;
            if !should_stamp {
                continue;
            }

            self.cursor = point;
            self.paint_floating_at_cursor();
            last_stamped = point;
        }

        let should_stamp_end =
            end.y != last_stamped.y || end.x.abs_diff(last_stamped.x) >= brush_width;
        if should_stamp_end {
            self.cursor = end;
            self.paint_floating_at_cursor();
        }
    }

    fn paint_floating_drag(&mut self, raw_pos: Pos) {
        let Some(last) = self.paint_stroke_last else {
            self.cursor = raw_pos;
            self.paint_floating_at_cursor();
            return;
        };

        let anchor = self.paint_stroke_anchor.unwrap_or(last);
        let brush_width = self.floating_brush_width();
        let is_pure_horizontal =
            brush_width > 1 && raw_pos.y == last.y && raw_pos.y == anchor.y && last.y == anchor.y;

        if is_pure_horizontal {
            let anchor_x = anchor.x;
            let snapped_x = Self::snap_horizontal_brush_x(anchor_x, raw_pos.x, brush_width);
            let target = Pos {
                x: snapped_x,
                y: raw_pos.y,
            };

            if target == last {
                return;
            }

            self.cursor = target;
            self.paint_floating_at_cursor();
            return;
        }

        if brush_width > 1 && raw_pos.y == last.y {
            if raw_pos.x.abs_diff(last.x) < brush_width {
                return;
            }

            self.cursor = raw_pos;
            self.paint_floating_at_cursor();
            return;
        }

        if brush_width > 1 && raw_pos.y != last.y {
            self.paint_floating_diagonal_segment(last, raw_pos, brush_width);
            return;
        }

        if raw_pos == last {
            return;
        }

        self.cursor = raw_pos;
        self.paint_floating_at_cursor();
    }

    fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.swatches[0].as_ref().map(|s| s.clipboard.clone()) else {
            return;
        };

        let cursor = self.cursor;
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| {
            for y in 0..clipboard.height {
                for x in 0..clipboard.width {
                    let target_x = cursor.x + x;
                    let target_y = cursor.y + y;
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
                        None => canvas.clear(target),
                    }
                }
            }
        });
    }

    fn smart_fill(&mut self) {
        let selection = self.selection();
        let bounds = self.selection_or_cursor_bounds();
        let ch = if bounds.width() == 1 && bounds.height() > 1 {
            '|'
        } else if bounds.height() == 1 && bounds.width() > 1 {
            '-'
        } else {
            '*'
        };
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| match selection {
            Some(selection) => Self::fill_selection_on(canvas, selection, bounds, ch, color),
            None => Self::fill_bounds_on(canvas, bounds, ch, color),
        });
    }

    fn draw_border(&mut self) {
        let Some(selection) = self.selection() else {
            return;
        };
        let bounds = selection.bounds();

        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| {
            if selection.shape == SelectionShape::Ellipse {
                Self::draw_selection_border_on(canvas, selection, bounds, color);
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
        });
    }

    fn fill_selection_or_cell(&mut self, ch: char) {
        let selection = self.selection();
        let bounds = self
            .selection_or_cursor_bounds()
            .normalized_for_canvas(&self.canvas);
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| match selection {
            Some(selection) => Self::fill_selection_on(canvas, selection, bounds, ch, color),
            None => Self::fill_bounds_on(canvas, bounds, ch, color),
        });
    }

    fn insert_char(&mut self, ch: char) {
        let cursor = self.cursor;
        let width = Canvas::display_width(ch);
        let color = self.active_user_color();
        self.apply_canvas_edit(|canvas| {
            let _ = canvas.put_glyph_colored(cursor, ch, color);
        });
        for _ in 0..width {
            self.move_right();
        }
    }

    fn open_emoji_picker(&mut self) {
        if self.icon_catalog.is_none() {
            self.icon_catalog = Some(emoji::catalog::IconCatalogData::load());
        }
        self.emoji_picker_state = emoji::EmojiPickerState::default();
        self.emoji_picker_open = true;
    }

    fn picker_selectable_count(&self) -> usize {
        let Some(catalog) = self.icon_catalog.as_ref() else {
            return 0;
        };
        let tab = *self.emoji_picker_state.tab.current();
        let sections = catalog.sections(tab, &self.emoji_picker_state.search_query);
        emoji::picker::selectable_count(&sections)
    }

    fn picker_move_selection(&mut self, delta: isize) {
        let max = self.picker_selectable_count();
        if max == 0 {
            return;
        }

        let cur = self.emoji_picker_state.selected_index as isize;
        let next = cur.saturating_add(delta).clamp(0, (max - 1) as isize) as usize;
        self.emoji_picker_state.selected_index = next;

        if let Some(catalog) = self.icon_catalog.as_ref() {
            Self::adjust_picker_scroll(&mut self.emoji_picker_state, catalog);
        }
    }

    fn adjust_picker_scroll(
        state: &mut emoji::EmojiPickerState,
        catalog: &emoji::catalog::IconCatalogData,
    ) {
        let tab = *state.tab.current();
        let sections = catalog.sections(tab, &state.search_query);
        let flat_idx =
            emoji::picker::selectable_to_flat(&sections, state.selected_index).unwrap_or(0);

        let visible = state.visible_height.get().max(1);
        if flat_idx < state.scroll_offset {
            state.scroll_offset = flat_idx;
        } else if flat_idx >= state.scroll_offset + visible {
            state.scroll_offset = flat_idx.saturating_sub(visible - 1);
        }
    }

    fn picker_insert_selected(&mut self, keep_open: bool) {
        let tab = *self.emoji_picker_state.tab.current();
        let selected = self.emoji_picker_state.selected_index;
        let query = self.emoji_picker_state.search_query.clone();

        let icon = {
            let Some(catalog) = self.icon_catalog.as_ref() else {
                self.emoji_picker_open = false;
                return;
            };
            let sections = catalog.sections(tab, &query);
            match emoji::picker::entry_at_selectable(&sections, selected) {
                Some(entry) => entry.icon.clone(),
                None => {
                    if !keep_open {
                        self.emoji_picker_open = false;
                    }
                    return;
                }
            }
        };

        if !keep_open {
            self.emoji_picker_open = false;
        }

        if let Some(ch) = icon.chars().next() {
            self.dismiss_floating();
            self.insert_char(ch);
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::META);

        if alt && key.code == KeyCode::Enter {
            self.picker_insert_selected(true);
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.emoji_picker_open = false;
            }
            KeyCode::Enter => self.picker_insert_selected(false),
            KeyCode::Tab => {
                self.emoji_picker_state.tab.move_next();
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
                self.emoji_picker_state.last_click = None;
            }
            KeyCode::BackTab => {
                self.emoji_picker_state.tab.move_prev();
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
                self.emoji_picker_state.last_click = None;
            }
            KeyCode::Backspace => {
                if self.emoji_picker_state.search_cursor > 0 {
                    let byte_pos = self
                        .emoji_picker_state
                        .search_query
                        .char_indices()
                        .nth(self.emoji_picker_state.search_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.emoji_picker_state.search_query.remove(byte_pos);
                    self.emoji_picker_state.search_cursor -= 1;
                    self.emoji_picker_state.selected_index = 0;
                    self.emoji_picker_state.scroll_offset = 0;
                }
            }
            KeyCode::Left => {
                self.emoji_picker_state.search_cursor =
                    self.emoji_picker_state.search_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                let len = self.emoji_picker_state.search_query.chars().count();
                if self.emoji_picker_state.search_cursor < len {
                    self.emoji_picker_state.search_cursor += 1;
                }
            }
            KeyCode::Up => self.picker_move_selection(-1),
            KeyCode::Down => self.picker_move_selection(1),
            KeyCode::PageUp => {
                let page = self.emoji_picker_state.visible_height.get().max(1) as isize;
                self.picker_move_selection(-page);
            }
            KeyCode::PageDown => {
                let page = self.emoji_picker_state.visible_height.get().max(1) as isize;
                self.picker_move_selection(page);
            }
            KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => {
                let byte_pos = self
                    .emoji_picker_state
                    .search_query
                    .char_indices()
                    .nth(self.emoji_picker_state.search_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.emoji_picker_state.search_query.len());
                self.emoji_picker_state.search_query.insert(byte_pos, ch);
                self.emoji_picker_state.search_cursor += 1;
                self.emoji_picker_state.selected_index = 0;
                self.emoji_picker_state.scroll_offset = 0;
            }
            _ => {}
        }
    }

    fn handle_picker_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let row_0based = mouse.row;
                let col_0based = mouse.column;

                let tabs = self.emoji_picker_state.tabs_inner.get();
                if tabs.height > 0 && row_0based >= tabs.y && row_0based < tabs.y + tabs.height {
                    if let Some(idx) = emoji::picker::tab_at_x(tabs, col_0based) {
                        self.emoji_picker_state.tab.set_index(idx);
                        self.emoji_picker_state.selected_index = 0;
                        self.emoji_picker_state.scroll_offset = 0;
                        self.emoji_picker_state.last_click = None;
                        return;
                    }
                }

                let list = self.emoji_picker_state.list_inner.get();
                if list.height == 0 || row_0based < list.y || row_0based >= list.y + list.height {
                    return;
                }
                let offset_in_list = (row_0based - list.y) as usize;
                let flat_idx = self.emoji_picker_state.scroll_offset + offset_in_list;

                let Some(catalog) = self.icon_catalog.as_ref() else {
                    return;
                };
                let tab = *self.emoji_picker_state.tab.current();
                let sections = catalog.sections(tab, &self.emoji_picker_state.search_query);
                let Some(selectable_idx) = emoji::picker::flat_to_selectable(&sections, flat_idx)
                else {
                    return;
                };

                let now = std::time::Instant::now();
                let is_double = match self.emoji_picker_state.last_click {
                    Some((prev, prev_idx)) => {
                        prev_idx == selectable_idx
                            && now.duration_since(prev).as_millis() <= emoji::DOUBLE_CLICK_WINDOW_MS
                    }
                    None => false,
                };

                self.emoji_picker_state.selected_index = selectable_idx;
                Self::adjust_picker_scroll(&mut self.emoji_picker_state, catalog);

                if is_double {
                    self.emoji_picker_state.last_click = None;
                    self.picker_insert_selected(true);
                } else {
                    self.emoji_picker_state.last_click = Some((now, selectable_idx));
                }
            }
            MouseEventKind::ScrollDown => self.picker_move_selection(3),
            MouseEventKind::ScrollUp => self.picker_move_selection(-3),
            _ => {}
        }
    }

    fn paste_text_block(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let origin = self.cursor;
        let color = self.active_user_color();
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
                            let _ = canvas.put_glyph_colored(Pos { x, y }, ch, color);
                        }
                        x += Canvas::display_width(ch);
                    }
                }
            }
        });
    }

    fn backspace(&mut self) {
        self.move_left();
        let origin = self.canvas.glyph_origin(self.cursor);
        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| canvas.clear(cursor));
        if let Some(origin) = origin {
            self.cursor = origin;
        }
    }

    fn delete_at_cursor(&mut self) {
        if let Some(origin) = self.canvas.glyph_origin(self.cursor) {
            self.cursor = origin;
        }
        let cursor = self.cursor;
        self.apply_canvas_edit(|canvas| canvas.clear(cursor));
    }

    fn push_left(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_left(y, x));
    }

    fn push_down(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_down(x, y));
    }

    fn push_up(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_up(x, y));
    }

    fn push_right(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.push_right(y, x));
    }

    fn pull_from_left(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_left(y, x));
    }

    fn pull_from_down(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_down(x, y));
    }

    fn pull_from_up(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
        let y = self.cursor.y;
        self.apply_canvas_edit(|canvas| canvas.pull_from_up(x, y));
    }

    fn pull_from_right(&mut self) {
        let x = self
            .canvas
            .glyph_origin(self.cursor)
            .map(|pos| pos.x)
            .unwrap_or(self.cursor.x);
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
            KeyCode::Char(ch) if swatch_home_row_index(ch).is_some() => {
                self.activate_swatch(swatch_home_row_index(ch).unwrap());
            }
            _ => return false,
        }

        true
    }

    fn handle_alt_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') => self.copy_to_system_clipboard(),
            KeyCode::Left => self.pan_by(-1, 0),
            KeyCode::Right => self.pan_by(1, 0),
            KeyCode::Up => self.pan_by(0, -1),
            KeyCode::Down => self.pan_by(0, 1),
            _ => return false,
        }

        true
    }

    fn is_open_picker_key(key: KeyEvent) -> bool {
        matches!(
            key.code,
            KeyCode::Char(']') if key.modifiers.contains(KeyModifiers::CONTROL)
        ) || matches!(
            key.code,
            KeyCode::Char('5') if key.modifiers.contains(KeyModifiers::CONTROL)
        ) || matches!(key.code, KeyCode::Char('\u{1d}'))
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press && Self::is_open_picker_key(key) => {
                self.open_emoji_picker();
            }
            Event::Key(key) if key.kind == KeyEventKind::Press && self.emoji_picker_open => {
                self.handle_picker_key(key);
                return;
            }
            Event::Mouse(mouse) if self.emoji_picker_open => {
                self.handle_picker_mouse(mouse);
                return;
            }
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
                        KeyCode::Tab | KeyCode::BackTab => {
                            self.help_tab = self.help_tab.toggle();
                        }
                        _ => {}
                    }
                    return;
                }

                if key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE {
                    self.switch_active_user(1);
                    return;
                }

                if key.code == KeyCode::BackTab {
                    self.switch_active_user(-1);
                    return;
                }

                if (key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL))
                    || key.code == KeyCode::F(1)
                {
                    self.show_help = !self.show_help;
                } else {
                    self.handle_key(key);
                }
            }
            Event::Mouse(mouse) => {
                if self.show_help {
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        if let Some(tab) = self.help_tab_hit(mouse.column, mouse.row) {
                            self.help_tab = tab;
                        }
                    }
                    return;
                }

                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    if let Some((idx, zone)) = self.swatch_hit(mouse.column, mouse.row) {
                        match zone {
                            SwatchZone::Pin => self.toggle_pin(idx),
                            SwatchZone::Body => self.activate_swatch(idx),
                        }
                        self.clamp_cursor();
                        return;
                    }
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
                                self.paint_floating_at_cursor();
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(pos) = canvas_pos {
                                self.paint_floating_drag(pos);
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
                                let extend_selection = mouse.modifiers.contains(KeyModifiers::ALT)
                                    && self.selection_anchor.is_some();
                                let ellipse_drag =
                                    mouse.modifiers.contains(KeyModifiers::CONTROL) && !extend_selection;

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
                                    self.selection_shape = if ellipse_drag {
                                        SelectionShape::Ellipse
                                    } else {
                                        SelectionShape::Rect
                                    };
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
            let alt = key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::META);

            match key.code {
                KeyCode::Char('t') if ctrl => {
                    self.toggle_float_transparency();
                    return;
                }
                KeyCode::Char(ch) if ctrl && swatch_home_row_index(ch).is_some() => {
                    self.activate_swatch(swatch_home_row_index(ch).unwrap());
                    return;
                }
                KeyCode::Char('c') | KeyCode::Char('x') if ctrl => {
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
        let Some(selection) = self.selection() else {
            return false;
        };
        selection.contains(pos)
    }
}

fn rect_contains(rect: &Rect, col: u16, row: u16) -> bool {
    col >= rect.x && row >= rect.y && col < rect.x + rect.width && row < rect.y + rect.height
}

fn swatch_home_row_index(ch: char) -> Option<usize> {
    match ch {
        'a' | 'A' => Some(0),
        's' | 'S' => Some(1),
        'd' | 'D' => Some(2),
        'f' | 'F' => Some(3),
        'g' | 'G' => Some(4),
        _ => None,
    }
}

fn random_available_user_color(used_colors: &[Color]) -> Color {
    let mut rng = rand::thread_rng();
    theme::PLAYER_PALETTE
        .iter()
        .copied()
        .filter(|color| !used_colors.contains(color))
        .collect::<Vec<_>>()
        .choose(&mut rng)
        .copied()
        .or_else(|| {
            theme::PLAYER_PALETTE
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .choose(&mut rng)
                .copied()
        })
        .unwrap_or(theme::TEXT)
}

#[cfg(test)]
mod tests {
    use super::{App, HelpTab, Mode, SelectionShape, SWATCH_CAPACITY};
    use crate::canvas::{CellValue, Pos};
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;

    fn setup_floating_wide_brush() -> App {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 64, 24));
        app.canvas.set(Pos { x: 0, y: 0 }, '🌱');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 0, y: 0 };
        app.mode = Mode::Select;
        app.copy_selection_or_cell();
        app.activate_swatch(0);
        app
    }

    fn wide_origins_in_row(app: &App, y: usize, x_max: usize) -> Vec<usize> {
        (0..=x_max)
            .filter_map(|x| match app.canvas.cell(Pos { x, y }) {
                Some(CellValue::Wide(_)) => Some(x),
                _ => None,
            })
            .collect()
    }

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
    fn ctrl_right_bracket_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn group_separator_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('\u{1d}'),
            KeyModifiers::NONE,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn ctrl_five_opens_picker() {
        let mut app = App::new();

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('5'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.emoji_picker_open);
    }

    #[test]
    fn tab_switches_active_local_user() {
        let mut app = App::new();
        app.cursor = Pos { x: 7, y: 4 };
        app.selection_anchor = Some(Pos { x: 3, y: 2 });
        app.mode = Mode::Select;

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.cursor, Pos { x: 0, y: 0 });
        assert_eq!(app.selection_anchor, None);
        assert!(!app.mode.is_selecting());

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.cursor, Pos { x: 7, y: 4 });
        assert_eq!(app.selection_anchor, Some(Pos { x: 3, y: 2 }));
        assert!(app.mode.is_selecting());
    }

    #[test]
    fn tab_cycles_help_tabs_when_help_open() {
        let mut app = App::new();
        app.show_help = true;
        assert_eq!(app.help_tab, HelpTab::Common);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.help_tab, HelpTab::Advanced);
        assert_eq!(app.active_user_idx, 0);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.help_tab, HelpTab::Common);
        assert_eq!(app.active_user_idx, 0);
    }

    #[test]
    fn local_users_share_canvas_but_keep_separate_swatch_state() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        app.cursor = Pos { x: 5, y: 5 };
        app.copy_selection_or_cell();
        assert!(app.swatches[0].is_some());

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));

        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert!(app.swatches[0].is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));

        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert!(app.swatches[0].is_some());
        assert_eq!(app.cursor, Pos { x: 5, y: 5 });
    }

    #[test]
    fn local_users_start_with_distinct_colors() {
        let app = App::new();
        let colors: Vec<_> = app.users().iter().map(|user| user.color).collect();
        for (idx, color) in colors.iter().enumerate() {
            assert!(
                colors[(idx + 1)..].iter().all(|other| other != color),
                "duplicate player color at index {idx}: {color:?}"
            );
        }
    }

    #[test]
    fn authored_cells_take_the_active_user_color() {
        let mut app = App::new();
        let first_color = app.active_user_color();

        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(app.canvas.fg(Pos { x: 0, y: 0 }), Some(first_color));

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        let second_color = app.active_user_color();
        assert_ne!(second_color, first_color);

        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(app.canvas.fg(Pos { x: 0, y: 0 }), Some(second_color));
    }

    #[test]
    fn keep_open_picker_insert_writes_adjacent_cells() {
        let mut app = App::new();
        app.open_emoji_picker();

        let expected = {
            let catalog = app.icon_catalog.as_ref().unwrap();
            let tab = *app.emoji_picker_state.tab.current();
            let sections = catalog.sections(tab, &app.emoji_picker_state.search_query);
            crate::emoji::picker::entry_at_selectable(
                &sections,
                app.emoji_picker_state.selected_index,
            )
            .unwrap()
            .icon
            .chars()
            .next()
            .unwrap()
        };

        app.picker_insert_selected(true);
        app.picker_insert_selected(true);

        assert!(app.emoji_picker_open);
        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), expected);
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 0 }), expected);
        assert_eq!(app.cursor, Pos { x: 4, y: 0 });
    }

    #[test]
    fn wide_glyph_insert_advances_two_cells() {
        let mut app = App::new();

        app.insert_char('🌱');

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), '🌱');
        assert!(app.canvas.is_continuation(Pos { x: 1, y: 0 }));
        assert_eq!(app.cursor, Pos { x: 2, y: 0 });
    }

    #[test]
    fn backspace_on_wide_glyph_clears_both_cells() {
        let mut app = App::new();
        app.insert_char('🌱');

        app.backspace();

        assert_eq!(app.canvas.get(Pos { x: 0, y: 0 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(app.cursor, Pos { x: 0, y: 0 });
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
    fn ctrl_drag_creates_ellipse_selection_and_masks_fill() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 8,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));

        assert_eq!(app.selection_anchor, Some(Pos { x: 2, y: 2 }));
        assert_eq!(app.cursor, Pos { x: 8, y: 6 });
        assert_eq!(app.selection_shape, SelectionShape::Ellipse);
        assert!(app.mode.is_selecting());
        assert!(app.is_selected(Pos { x: 5, y: 4 }));
        assert!(!app.is_selected(Pos { x: 2, y: 2 }));

        app.fill_selection_or_cell('x');

        assert_eq!(app.canvas.get(Pos { x: 5, y: 4 }), 'x');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 2 }), ' ');
    }

    #[test]
    fn ellipse_selection_state_is_per_user() {
        let mut app = App::new();
        app.set_viewport(Rect::new(0, 0, 20, 10));

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 9,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 9,
            row: 6,
            modifiers: KeyModifiers::CONTROL,
        }));

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(app.active_user_idx, 1);
        assert_eq!(app.selection_anchor, None);
        assert!(!app.mode.is_selecting());

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(app.active_user_idx, 0);
        assert_eq!(app.selection_anchor, Some(Pos { x: 3, y: 2 }));
        assert_eq!(app.cursor, Pos { x: 9, y: 6 });
        assert_eq!(app.selection_shape, SelectionShape::Ellipse);
        assert!(app.mode.is_selecting());
        assert!(app.is_selected(Pos { x: 6, y: 4 }));
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
    fn copy_pushes_swatch_without_entering_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 1);
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), 'A');

        // Another copy on same selection: still no auto-lift, just another swatch push.
        app.copy_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 2);
        assert!(app.floating.is_none());
    }

    #[test]
    fn cut_pushes_swatch_and_clears_canvas() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'X');
        app.canvas.set(Pos { x: 2, y: 1 }, 'Y');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.cut_selection_or_cell();
        assert_eq!(app.populated_swatch_count(), 1);
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 2, y: 1 }), ' ');
    }

    #[test]
    fn swatch_history_newest_first_and_capped() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C', 'D', 'E', 'F'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }

        assert_eq!(app.swatches.iter().filter(|s| s.is_some()).count(), 5);
        // Most recent is at index 0.
        assert_eq!(
            app.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('F'))
        );
        // Oldest ('A') evicted once a sixth swatch pushed in.
        assert_eq!(
            app.swatches[4].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
    }

    #[test]
    fn pinned_swatch_holds_slot_when_history_rotates() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }
        // Slot order after three copies: [C (idx 0), B (idx 1), A (idx 2), _, _].
        assert_eq!(
            app.swatches[1].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
        app.toggle_pin(1);
        assert!(app.swatches[1].as_ref().unwrap().pinned);

        // Push three more; B at slot 1 must not move or get evicted.
        for (i, ch) in ['D', 'E', 'F'].iter().enumerate() {
            app.canvas.set(Pos { x: 10 + i, y: 0 }, *ch);
            app.cursor = Pos { x: 10 + i, y: 0 };
            app.copy_selection_or_cell();
        }

        // Slot 1 still B (pinned).
        assert_eq!(
            app.swatches[1].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('B'))
        );
        assert!(app.swatches[1].as_ref().unwrap().pinned);
        // Newest (F) sits at slot 0.
        assert_eq!(
            app.swatches[0].as_ref().unwrap().clipboard.get(0, 0),
            Some(CellValue::Narrow('F'))
        );
    }

    #[test]
    fn all_pinned_swatches_reject_new_push() {
        let mut app = App::new();
        for (i, ch) in ['A', 'B', 'C', 'D', 'E'].iter().enumerate() {
            app.canvas.set(Pos { x: i, y: 0 }, *ch);
            app.cursor = Pos { x: i, y: 0 };
            app.copy_selection_or_cell();
        }
        for i in 0..SWATCH_CAPACITY {
            app.toggle_pin(i);
        }
        let before: Vec<_> = app
            .swatches
            .iter()
            .map(|s| s.as_ref().unwrap().clipboard.get(0, 0))
            .collect();

        app.canvas.set(Pos { x: 20, y: 0 }, 'Z');
        app.cursor = Pos { x: 20, y: 0 };
        app.copy_selection_or_cell();

        let after: Vec<_> = app
            .swatches
            .iter()
            .map(|s| s.as_ref().unwrap().clipboard.get(0, 0))
            .collect();
        assert_eq!(before, after, "all-pinned strip should reject new copies");
    }

    #[test]
    fn ctrl_home_row_activates_swatch() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();

        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(app.floating.is_some());
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
    }

    #[test]
    fn ctrl_home_row_while_floating_switches_or_cycles_swatch() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');
        app.cursor = Pos { x: 1, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(1); // lift from the older swatch (A at slot 1)
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(1));

        // ^a while floating switches to slot 0 (B).
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
        assert!(!app.floating.as_ref().unwrap().transparent);

        // Pressing ^a again cycles transparency for the active swatch.
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn bare_digit_draws_even_while_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.activate_swatch(0);
        assert!(app.floating.is_some());

        // Pressing '1' now dismisses the lift and draws the digit like any other char.
        app.cursor = Pos { x: 5, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert!(app.floating.is_none());
        assert_eq!(app.canvas.get(Pos { x: 5, y: 5 }), '1');
    }

    #[test]
    fn activate_swatch_enters_floating_from_history() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 1 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        assert!(app.floating.is_some());
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(0));
        assert!(!app.mode.is_selecting());
        assert_eq!(app.canvas.get(Pos { x: 1, y: 1 }), 'A');
    }

    #[test]
    fn activate_same_swatch_again_toggles_transparency() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(0);
        assert!(!app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(0);
        assert!(app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(0);
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn activate_different_swatch_switches_to_opaque() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 1, y: 0 }, 'B');

        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.cursor = Pos { x: 1, y: 0 };
        app.copy_selection_or_cell();

        app.activate_swatch(0);
        app.activate_swatch(0); // flip to transparent
        assert!(app.floating.as_ref().unwrap().transparent);

        app.activate_swatch(1); // switch: should be opaque again
        assert_eq!(app.floating.as_ref().unwrap().source_index, Some(1));
        assert!(!app.floating.as_ref().unwrap().transparent);
    }

    #[test]
    fn ctrl_t_toggles_transparency_while_floating() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.cursor = Pos { x: 0, y: 0 };
        app.copy_selection_or_cell();
        app.activate_swatch(0);

        assert!(!app.floating.as_ref().unwrap().transparent);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(app.floating.as_ref().unwrap().transparent);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(!app.floating.as_ref().unwrap().transparent);
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
        app.activate_swatch(0);

        app.cursor = Pos { x: 5, y: 3 };
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

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
        app.activate_swatch(0);

        app.cursor = Pos { x: 5, y: 5 };
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.floating.is_none());
        // Swatch history still intact so the user can re-enter.
        assert_eq!(app.populated_swatch_count(), 1);
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
        app.activate_swatch(0);

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
        app.activate_swatch(0);

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

        assert!(app.floating.is_some());
        assert_eq!(app.canvas.get(Pos { x: 7, y: 4 }), 'M');
    }

    #[test]
    fn transparent_stamp_preserves_underlying_content() {
        let mut app = App::new();
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 2, y: 0 }, 'B');
        app.selection_anchor = Some(Pos { x: 0, y: 0 });
        app.cursor = Pos { x: 2, y: 0 };
        app.mode = Mode::Select;

        app.copy_selection_or_cell();
        app.activate_swatch(0);

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
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
        app.activate_swatch(0);

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
        app.activate_swatch(0);

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

    #[test]
    fn horizontal_drag_with_wide_brush_skips_overlapping_cells() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 4,
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
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 3, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 4, y: 2 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 6, y: 2 }),
            Some(CellValue::WideCont)
        );
    }

    #[test]
    fn diagonal_drag_with_wide_brush_does_not_emit_horizontal_rays() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 16,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 7,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 12, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 13, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 16, y: 7 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 17, y: 7 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 8, y: 7 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 9, y: 7 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(app.canvas.get(Pos { x: 10, y: 7 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 12, y: 7 }), ' ');
    }

    #[test]
    fn wide_brush_same_row_jump_does_not_fill_intermediate_cells() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 12, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 13, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 4, y: 6 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 6 }),
            Some(CellValue::WideCont)
        );
        assert_eq!(app.canvas.get(Pos { x: 6, y: 6 }), ' ');
        assert_eq!(app.canvas.get(Pos { x: 10, y: 6 }), ' ');
    }

    #[test]
    fn shallow_diagonal_drag_with_wide_brush_fills_more_evenly() {
        let mut app = setup_floating_wide_brush();

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 9,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }));
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 9,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            app.canvas.cell(Pos { x: 3, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 5, y: 2 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 6, y: 3 }),
            Some(CellValue::Wide('🌱'))
        );
        assert_eq!(
            app.canvas.cell(Pos { x: 8, y: 3 }),
            Some(CellValue::Wide('🌱'))
        );
    }

    #[test]
    fn shallow_wide_brush_diagonal_sweep_keeps_row_gaps_within_brush_width() {
        for start_x in [2_u16, 3_u16] {
            for end_x in (start_x + 3)..=24 {
                let mut app = setup_floating_wide_brush();

                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: start_x,
                    row: 2,
                    modifiers: KeyModifiers::NONE,
                }));
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: end_x,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: end_x,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));

                let row_two = wide_origins_in_row(&app, 2, end_x as usize + 2);
                let row_three = wide_origins_in_row(&app, 3, end_x as usize + 2);

                assert!(
                    !row_two.is_empty(),
                    "row 2 empty for start_x={start_x}, end_x={end_x}"
                );
                assert!(
                    !row_three.is_empty(),
                    "row 3 empty for start_x={start_x}, end_x={end_x}"
                );
                assert!(
                    row_two.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                    "row 2 gap too large for start_x={start_x}, end_x={end_x}: {row_two:?}"
                );
                assert!(
                    row_three.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                    "row 3 gap too large for start_x={start_x}, end_x={end_x}: {row_three:?}"
                );
            }
        }
    }

    #[test]
    fn shallow_diagonal_with_same_row_micro_steps_keeps_visible_progress() {
        for start_x in [3_u16, 4_u16] {
            let mut app = setup_floating_wide_brush();

            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: start_x,
                row: 2,
                modifiers: KeyModifiers::NONE,
            }));
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: start_x + 4,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }));
            for column in (start_x + 5)..=(start_x + 11) {
                app.handle_event(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            app.handle_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: start_x + 11,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }));

            let row_three = wide_origins_in_row(&app, 3, (start_x + 13) as usize);
            assert!(
                row_three.len() >= 4,
                "expected multiple visible stamps on shallow row for start_x={start_x}: {row_three:?}"
            );
            assert!(
                row_three.windows(2).all(|pair| pair[1] - pair[0] <= 2),
                "row 3 gap too large for start_x={start_x}: {row_three:?}"
            );
        }
    }

    #[test]
    fn system_clipboard_export_uses_selection_when_present() {
        let mut app = App::new();
        app.canvas.width = 4;
        app.canvas.height = 3;
        app.canvas.set(Pos { x: 1, y: 1 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'B');
        app.canvas.set(Pos { x: 1, y: 2 }, 'C');
        app.canvas.set(Pos { x: 2, y: 2 }, 'D');
        app.selection_anchor = Some(Pos { x: 1, y: 1 });
        app.cursor = Pos { x: 2, y: 2 };
        app.mode = Mode::Select;

        assert_eq!(app.export_system_clipboard_text(), "AB\nCD");
    }

    #[test]
    fn system_clipboard_export_uses_full_canvas_without_selection() {
        let mut app = App::new();
        app.canvas.width = 3;
        app.canvas.height = 2;
        app.canvas.set(Pos { x: 0, y: 0 }, 'A');
        app.canvas.set(Pos { x: 2, y: 1 }, 'Z');

        assert_eq!(app.export_system_clipboard_text(), "A  \n  Z");
    }
}
