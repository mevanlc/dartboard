use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::canvas::{Canvas, Pos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Visual,
    VisualLine,
    VisualBlock,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Replace => "replace",
            Mode::Visual => "visual",
            Mode::VisualLine => "visual line",
            Mode::VisualBlock => "visual block",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine | Mode::VisualBlock)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
    pub mode: Mode,
}

pub struct App {
    pub canvas: Canvas,
    pub cursor: Pos,
    pub mode: Mode,
    pub should_quit: bool,
    pub show_help: bool,
    pub viewport: Rect,
    pub visual_anchor: Option<Pos>,
    pub pending_replace: bool,
    pub pending_g: bool,
    /// Most recent visual selection (for gv recall from normal mode)
    last_selection: Option<Selection>,
    /// History of operated-on selections (gv/gV to cycle)
    selection_stack: Vec<Selection>,
    /// Current position in the selection stack (-1 = not browsing)
    stack_cursor: isize,
    last_click: Option<(Instant, Pos)>,
    drag_origin: Option<Pos>,
    pub simple_mode: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            canvas: Canvas::new(),
            cursor: Pos { x: 0, y: 0 },
            mode: Mode::Replace,
            should_quit: false,
            show_help: false,
            viewport: Rect::default(),
            visual_anchor: None,
            pending_replace: false,
            pending_g: false,
            last_selection: None,
            selection_stack: Vec::new(),
            stack_cursor: -1,
            last_click: None,
            drag_origin: None,
            simple_mode: true,
        }
    }

    pub fn mode_label(&self) -> &'static str {
        if self.simple_mode {
            if self.mode.is_visual() {
                "select"
            } else {
                "simple"
            }
        } else {
            self.mode.label()
        }
    }

    fn move_left(&mut self) {
        if self.cursor.x > 0 {
            self.cursor.x -= 1;
        } else if self.cursor.y > 0 {
            self.cursor.y -= 1;
            self.cursor.x = self.canvas.width.saturating_sub(1);
        }
    }

    fn move_right(&mut self) {
        let max_x = self.canvas.width.saturating_sub(1);
        if self.cursor.x < max_x {
            self.cursor.x += 1;
        } else if self.cursor.y < self.canvas.height.saturating_sub(1) {
            self.cursor.y += 1;
            self.cursor.x = 0;
        }
    }

    fn move_up(&mut self) {
        if self.cursor.y > 0 {
            self.cursor.y -= 1;
        } else {
            self.cursor.y = self.canvas.height.saturating_sub(1);
        }
    }

    fn move_down(&mut self) {
        let max_y = self.canvas.height.saturating_sub(1);
        if self.cursor.y < max_y {
            self.cursor.y += 1;
        } else {
            self.cursor.y = 0;
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
            let cx = col - vx;
            let cy = row - vy;
            if cx < self.canvas.width && cy < self.canvas.height {
                return Some(Pos { x: cx, y: cy });
            }
        }
        None
    }

    fn clamp_cursor(&mut self) {
        self.cursor.x = self.cursor.x.min(self.canvas.width.saturating_sub(1));
        self.cursor.y = self.cursor.y.min(self.canvas.height.saturating_sub(1));
    }

    fn current_selection(&self) -> Option<Selection> {
        self.visual_anchor.map(|anchor| Selection {
            anchor,
            cursor: self.cursor,
            mode: self.mode,
        })
    }

    fn activate_selection(&mut self, sel: Selection) {
        self.visual_anchor = Some(sel.anchor);
        self.cursor = sel.cursor;
        self.mode = sel.mode;
    }

    /// Leave visual mode: save selection for gv recall, clear anchor
    fn leave_visual(&mut self) {
        self.last_selection = self.current_selection();
        self.visual_anchor = None;
        self.pending_replace = false;
        self.pending_g = false;
    }

    /// Enter visual mode from normal, reviving last selection if available
    fn enter_visual(&mut self, kind: Mode) {
        if let Some(sel) = self.last_selection.take() {
            self.visual_anchor = Some(sel.anchor);
            self.cursor = sel.cursor;
            self.mode = kind;
        } else {
            self.visual_anchor = Some(self.cursor);
            self.mode = kind;
        }
    }

    /// Push current selection to operated-on history.
    /// If browsing mid-stack, truncate forward history first.
    fn push_to_history(&mut self) {
        if let Some(sel) = self.current_selection() {
            if self.stack_cursor >= 0 {
                self.selection_stack
                    .truncate(self.stack_cursor as usize + 1);
            }
            self.selection_stack.push(sel);
            self.stack_cursor = -1;
        }
    }

    /// gv: browse backward through selection history
    fn handle_gv(&mut self) {
        if !self.mode.is_visual() {
            // Normal mode: recall last selection
            if let Some(sel) = self.last_selection {
                self.activate_selection(sel);
            }
            return;
        }
        if self.selection_stack.is_empty() {
            return;
        }
        // Start browsing or step backward
        if self.stack_cursor < 0 {
            self.stack_cursor = self.selection_stack.len() as isize - 1;
        } else {
            self.stack_cursor = (self.stack_cursor - 1).max(0);
        }
        let sel = self.selection_stack[self.stack_cursor as usize];
        self.activate_selection(sel);
    }

    /// gV: browse forward through selection history
    fn handle_g_shift_v(&mut self) {
        if !self.mode.is_visual() || self.selection_stack.is_empty() || self.stack_cursor < 0 {
            return;
        }
        self.stack_cursor =
            (self.stack_cursor + 1).min(self.selection_stack.len() as isize - 1);
        let sel = self.selection_stack[self.stack_cursor as usize];
        self.activate_selection(sel);
    }

    fn toggle_simple_mode(&mut self) {
        self.simple_mode = !self.simple_mode;
        if self.mode.is_visual() {
            self.leave_visual();
        }
        self.visual_anchor = None;
        self.pending_g = false;
        self.pending_replace = false;
        self.mode = if self.simple_mode {
            Mode::Replace
        } else {
            Mode::Normal
        };
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if self.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1) => {
                            self.show_help = false
                        }
                        _ => {}
                    }
                    return;
                }
                // ^Q: quit
                if key.code == KeyCode::Char('q')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.should_quit = true;
                // ^P: help
                } else if key.code == KeyCode::Char('p')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.show_help = !self.show_help;
                // ^G: toggle simple/vi mode
                } else if key.code == KeyCode::Char('g')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.toggle_simple_mode();
                } else if key.code == KeyCode::F(1) {
                    self.show_help = !self.show_help;
                } else if self.simple_mode {
                    self.handle_simple_key(key);
                } else {
                    self.handle_key(key);
                }
            }
            Event::Mouse(mouse) => {
                if self.show_help {
                    return;
                }
                self.pending_g = false;
                self.pending_replace = false;
                let canvas_pos = self.mouse_to_canvas(mouse.column, mouse.row);

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(pos) = canvas_pos {
                            if self.simple_mode {
                                if self.mode.is_visual() {
                                    self.visual_anchor = None;
                                    self.mode = Mode::Replace;
                                }
                                self.cursor = pos;
                                self.drag_origin = Some(pos);
                            } else {
                                let is_double = self.last_click.is_some_and(|(t, p)| {
                                    p == pos && t.elapsed().as_millis() < 400
                                });
                                if self.mode.is_visual() {
                                    self.leave_visual();
                                }
                                self.cursor = pos;
                                self.drag_origin = Some(pos);
                                self.mode = if is_double {
                                    Mode::Insert
                                } else {
                                    Mode::Replace
                                };
                                self.last_click = Some((Instant::now(), pos));
                            }
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if let (Some(origin), Some(pos)) = (self.drag_origin, canvas_pos) {
                            if pos != origin || self.mode.is_visual() {
                                self.visual_anchor = Some(origin);
                                self.mode = Mode::VisualBlock;
                                self.cursor = pos;
                            }
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.drag_origin = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.clamp_cursor();
    }

    fn handle_simple_key(&mut self, key: KeyEvent) {
        // ^D / ^U / ^O: column push/pull, swap selection corner
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.canvas.shift_col_down(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('u') => {
                    self.canvas.shift_col_up(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('o') => {
                    if let Some(anchor) = self.visual_anchor.as_mut() {
                        std::mem::swap(anchor, &mut self.cursor);
                    }
                    return;
                }
                _ => {}
            }
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

        // Shift+move: create/extend block selection
        if is_move && shift {
            if self.visual_anchor.is_none() {
                self.visual_anchor = Some(self.cursor);
                self.mode = Mode::VisualBlock;
            }
            match key.code {
                KeyCode::Up => self.move_up(),
                KeyCode::Down => self.move_down(),
                KeyCode::Left => self.move_left(),
                KeyCode::Right => self.move_right(),
                KeyCode::Home => self.cursor.x = 0,
                KeyCode::End => self.cursor.x = self.canvas.width.saturating_sub(1),
                KeyCode::PageUp => self.cursor.y = 0,
                KeyCode::PageDown => self.cursor.y = self.canvas.height.saturating_sub(1),
                _ => {}
            }
            return;
        }

        // Unshifted move: cancel selection if active
        if is_move && self.mode.is_visual() {
            self.visual_anchor = None;
            self.mode = Mode::Replace;
        }

        match key.code {
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.cursor.x = 0,
            KeyCode::End => self.cursor.x = self.canvas.width.saturating_sub(1),
            KeyCode::PageUp => self.cursor.y = 0,
            KeyCode::PageDown => self.cursor.y = self.canvas.height.saturating_sub(1),
            KeyCode::Enter => self.move_down(),
            KeyCode::Esc => {
                self.visual_anchor = None;
                self.mode = Mode::Replace;
            }
            _ if self.mode.is_visual() && self.visual_anchor.is_some() => {
                // With selection: type fills, bksp/del clears
                match key.code {
                    KeyCode::Char(ch) => {
                        self.push_to_history();
                        self.fill_selection(ch);
                    }
                    KeyCode::Backspace | KeyCode::Delete => {
                        self.push_to_history();
                        self.fill_selection(' ');
                    }
                    _ => {}
                }
            }
            _ => {
                // No selection: replace-style typing
                match key.code {
                    KeyCode::Char(ch) => {
                        self.canvas.set(self.cursor, ch);
                        self.move_right();
                    }
                    KeyCode::Backspace => {
                        self.move_left();
                        self.canvas.clear(self.cursor);
                    }
                    KeyCode::Delete => {
                        self.canvas.clear(self.cursor);
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                self.move_up();
                return;
            }
            KeyCode::Down => {
                self.move_down();
                return;
            }
            KeyCode::Left => {
                self.move_left();
                return;
            }
            KeyCode::Right => {
                self.move_right();
                return;
            }
            KeyCode::Home => {
                self.cursor.x = 0;
                return;
            }
            KeyCode::End => {
                self.cursor.x = self.canvas.width.saturating_sub(1);
                return;
            }
            KeyCode::PageUp => {
                self.cursor.y = 0;
                return;
            }
            KeyCode::PageDown => {
                self.cursor.y = self.canvas.height.saturating_sub(1);
                return;
            }
            _ => {}
        }

        // ^D: push column down at cursor, ^U: pull column up at cursor
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.canvas.shift_col_down(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('u') => {
                    self.canvas.shift_col_up(self.cursor.x, self.cursor.y);
                    return;
                }
                _ => {}
            }
        }

        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::Insert => self.handle_insert(key),
            Mode::Replace => self.handle_replace(key),
            _ if self.mode.is_visual() => self.handle_visual(key),
            _ => {}
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) {
        // r + char: replace one char at cursor, stay in normal
        if self.pending_replace {
            self.pending_replace = false;
            if let KeyCode::Char(ch) = key.code {
                self.canvas.set(self.cursor, ch);
            }
            return;
        }

        // g prefix
        if self.pending_g {
            self.pending_g = false;
            match key.code {
                KeyCode::Char('g') => self.cursor.y = 0,
                KeyCode::Char('v') => self.handle_gv(),
                KeyCode::Char('V') => self.handle_g_shift_v(),
                _ => {}
            }
            return;
        }

        // Ctrl+V: visual block
        if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.enter_visual(Mode::VisualBlock);
            return;
        }

        match key.code {
            KeyCode::Char('h') => self.move_left(),
            KeyCode::Char('j') => self.move_down(),
            KeyCode::Char('k') => self.move_up(),
            KeyCode::Char('l') => self.move_right(),

            KeyCode::Char('0') => self.cursor.x = 0,
            KeyCode::Char('$') => {
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).last() {
                    self.cursor.x = x;
                }
            }
            KeyCode::Char('^') => {
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).first() {
                    self.cursor.x = x;
                }
            }

            KeyCode::Char('w') | KeyCode::Char('W') => self.word_forward(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.word_end(),
            KeyCode::Char('b') | KeyCode::Char('B') => self.word_back(),

            KeyCode::Char('i') => self.mode = Mode::Insert,
            KeyCode::Char('a') => {
                self.cursor.x += 1;
                self.mode = Mode::Insert;
            }
            KeyCode::Char('A') => {
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).last() {
                    self.cursor.x = x + 1;
                }
                self.mode = Mode::Insert;
            }
            KeyCode::Char('R') => self.mode = Mode::Replace,
            KeyCode::Char('G') => {
                self.cursor.y = self.canvas.height.saturating_sub(1);
            }

            KeyCode::Char('v') => self.enter_visual(Mode::Visual),
            KeyCode::Char('V') => self.enter_visual(Mode::VisualLine),

            KeyCode::Char('x') => {
                self.canvas.clear(self.cursor);
                self.canvas.shift_left(self.cursor.y, self.cursor.x);
            }
            KeyCode::Char('X') => {
                if self.cursor.x > 0 {
                    self.cursor.x -= 1;
                    self.canvas.clear(self.cursor);
                    self.canvas.shift_left(self.cursor.y, self.cursor.x);
                }
            }

            KeyCode::Backspace => self.move_left(),
            KeyCode::Delete => self.canvas.clear(self.cursor),

            KeyCode::Char('r') => self.pending_replace = true,
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('q') => self.should_quit = true,

            _ => {}
        }
    }

    fn handle_insert(&mut self, key: KeyEvent) {
        if (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            self.canvas
                .shift_col_down(self.cursor.x, self.cursor.y + 1);
            self.cursor.y += 1;
            return;
        }

        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.canvas.shift_col_down(self.cursor.x, self.cursor.y);
            }
            KeyCode::Char(ch) => {
                self.canvas.shift_right(self.cursor.y, self.cursor.x);
                self.canvas.set(self.cursor, ch);
                self.cursor.x += 1;
            }
            KeyCode::Backspace => {
                if self.cursor.x > 0 {
                    self.cursor.x -= 1;
                    self.canvas.clear(self.cursor);
                    self.canvas.shift_left(self.cursor.y, self.cursor.x);
                }
            }
            KeyCode::Delete => {
                self.canvas.clear(self.cursor);
                self.canvas.shift_left(self.cursor.y, self.cursor.x);
            }
            _ => {}
        }
    }

    fn handle_replace(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.canvas.set(self.cursor, ' ');
                self.cursor.y += 1;
            }
            KeyCode::Char(ch) => {
                self.canvas.set(self.cursor, ch);
                self.cursor.x += 1;
            }
            KeyCode::Backspace => self.cursor.x = self.cursor.x.saturating_sub(1),
            KeyCode::Delete => self.canvas.clear(self.cursor),
            _ => {}
        }
    }

    fn handle_visual(&mut self, key: KeyEvent) {
        // r + char: fill selection, push to history, stay in visual
        if self.pending_replace {
            if let KeyCode::Char(ch) = key.code {
                self.push_to_history();
                self.fill_selection(ch);
            }
            self.pending_replace = false;
            return;
        }

        // g prefix
        if self.pending_g {
            self.pending_g = false;
            match key.code {
                KeyCode::Char('g') => self.cursor.y = 0,
                KeyCode::Char('v') => self.handle_gv(),
                KeyCode::Char('V') => self.handle_g_shift_v(),
                _ => {}
            }
            return;
        }

        // Ctrl+V
        if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.mode == Mode::VisualBlock {
                self.visual_anchor = Some(self.cursor); // reset
            } else {
                self.mode = Mode::VisualBlock; // convert
            }
            return;
        }

        match key.code {
            // Movement
            KeyCode::Char('h') => self.move_left(),
            KeyCode::Char('j') => self.move_down(),
            KeyCode::Char('k') => self.move_up(),
            KeyCode::Char('l') => self.move_right(),
            KeyCode::Char('0') => self.cursor.x = 0,
            KeyCode::Char('$') => {
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).last() {
                    self.cursor.x = x;
                }
            }
            KeyCode::Char('^') => {
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).first() {
                    self.cursor.x = x;
                }
            }
            KeyCode::Char('w') | KeyCode::Char('W') => self.word_forward(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.word_end(),
            KeyCode::Char('b') | KeyCode::Char('B') => self.word_back(),
            KeyCode::Char('G') => {
                self.cursor.y = self.canvas.height.saturating_sub(1);
            }

            // Same key = reset fresh, different key = convert
            KeyCode::Char('v') => {
                if self.mode == Mode::Visual {
                    self.visual_anchor = Some(self.cursor);
                } else {
                    self.mode = Mode::Visual;
                }
            }
            KeyCode::Char('V') => {
                if self.mode == Mode::VisualLine {
                    self.visual_anchor = Some(self.cursor);
                } else {
                    self.mode = Mode::VisualLine;
                }
            }

            // Operations (stay in visual, push to history)
            KeyCode::Char('r') => self.pending_replace = true,
            KeyCode::Backspace | KeyCode::Delete => {
                self.push_to_history();
                self.fill_selection(' ');
            }

            // Prefix
            KeyCode::Char('g') => self.pending_g = true,

            // Exit visual → other mode (saves for gv recall)
            KeyCode::Char('i') => {
                self.leave_visual();
                self.mode = Mode::Insert;
            }
            KeyCode::Char('a') => {
                self.leave_visual();
                self.cursor.x += 1;
                self.mode = Mode::Insert;
            }
            KeyCode::Char('A') => {
                self.leave_visual();
                if let Some(&x) = self.canvas.row_occupied(self.cursor.y).last() {
                    self.cursor.x = x + 1;
                }
                self.mode = Mode::Insert;
            }
            KeyCode::Char('R') => {
                self.leave_visual();
                self.mode = Mode::Replace;
            }
            KeyCode::Esc => {
                self.leave_visual();
                self.mode = Mode::Normal;
            }

            _ => {}
        }
    }

    fn pos_in_selection(pos: Pos, anchor: Pos, cursor: Pos, mode: Mode) -> bool {
        match mode {
            Mode::Visual => {
                let (start, end) = if (anchor.y, anchor.x) <= (cursor.y, cursor.x) {
                    (anchor, cursor)
                } else {
                    (cursor, anchor)
                };
                if start.y == end.y {
                    pos.y == start.y && pos.x >= start.x && pos.x <= end.x
                } else if pos.y == start.y {
                    pos.x >= start.x
                } else if pos.y == end.y {
                    pos.x <= end.x
                } else {
                    pos.y > start.y && pos.y < end.y
                }
            }
            Mode::VisualLine => {
                let (min_y, max_y) = (anchor.y.min(cursor.y), anchor.y.max(cursor.y));
                pos.y >= min_y && pos.y <= max_y
            }
            Mode::VisualBlock => {
                let (min_x, max_x) = (anchor.x.min(cursor.x), anchor.x.max(cursor.x));
                let (min_y, max_y) = (anchor.y.min(cursor.y), anchor.y.max(cursor.y));
                pos.x >= min_x && pos.x <= max_x && pos.y >= min_y && pos.y <= max_y
            }
            _ => false,
        }
    }

    pub fn is_selected(&self, pos: Pos) -> bool {
        match self.visual_anchor {
            Some(anchor) if self.mode.is_visual() => {
                Self::pos_in_selection(pos, anchor, self.cursor, self.mode)
            }
            _ => false,
        }
    }

    fn fill_selection(&mut self, ch: char) {
        let vw = self.canvas.width;
        let anchor = match self.visual_anchor {
            Some(a) => a,
            None => return,
        };

        match self.mode {
            Mode::Visual => {
                let (start, end) = if (anchor.y, anchor.x) <= (self.cursor.y, self.cursor.x) {
                    (anchor, self.cursor)
                } else {
                    (self.cursor, anchor)
                };
                if start.y == end.y {
                    for x in start.x..=end.x {
                        self.canvas.set(Pos { x, y: start.y }, ch);
                    }
                } else {
                    for x in start.x..vw {
                        self.canvas.set(Pos { x, y: start.y }, ch);
                    }
                    for y in (start.y + 1)..end.y {
                        for x in 0..vw {
                            self.canvas.set(Pos { x, y }, ch);
                        }
                    }
                    for x in 0..=end.x {
                        self.canvas.set(Pos { x, y: end.y }, ch);
                    }
                }
            }
            Mode::VisualLine => {
                let (min_y, max_y) = (anchor.y.min(self.cursor.y), anchor.y.max(self.cursor.y));
                for y in min_y..=max_y {
                    for x in 0..vw {
                        self.canvas.set(Pos { x, y }, ch);
                    }
                }
            }
            Mode::VisualBlock => {
                let (min_x, max_x) = (anchor.x.min(self.cursor.x), anchor.x.max(self.cursor.x));
                let (min_y, max_y) = (anchor.y.min(self.cursor.y), anchor.y.max(self.cursor.y));
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        self.canvas.set(Pos { x, y }, ch);
                    }
                }
            }
            _ => {}
        }
    }

    fn word_forward(&mut self) {
        let row = self.canvas.row_content(self.cursor.y);
        let mut x = self.cursor.x;
        let len = row.len();
        if x >= len {
            return;
        }
        if row[x] != ' ' {
            while x < len && row[x] != ' ' {
                x += 1;
            }
        }
        while x < len && row[x] == ' ' {
            x += 1;
        }
        self.cursor.x = x;
    }

    fn word_end(&mut self) {
        let row = self.canvas.row_content(self.cursor.y);
        let len = row.len();
        let mut x = self.cursor.x + 1;
        if x >= len {
            return;
        }
        while x < len && row[x] == ' ' {
            x += 1;
        }
        while x < len && row[x] != ' ' {
            x += 1;
        }
        self.cursor.x = x.saturating_sub(1);
    }

    fn word_back(&mut self) {
        let row = self.canvas.row_content(self.cursor.y);
        if row.is_empty() || self.cursor.x == 0 {
            return;
        }
        let mut x = self.cursor.x.min(row.len()).saturating_sub(1);
        while x > 0 && row[x] == ' ' {
            x -= 1;
        }
        while x > 0 && row[x - 1] != ' ' {
            x -= 1;
        }
        self.cursor.x = x;
    }
}
