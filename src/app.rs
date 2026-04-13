use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::canvas::{Canvas, Pos};

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

#[derive(Debug, Clone, Copy)]
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
struct Clipboard {
    width: usize,
    height: usize,
    cells: Vec<char>,
}

impl Clipboard {
    fn get(&self, x: usize, y: usize) -> char {
        self.cells[y * self.width + x]
    }
}

pub struct App {
    pub canvas: Canvas,
    pub cursor: Pos,
    pub mode: Mode,
    pub should_quit: bool,
    pub show_help: bool,
    pub viewport: Rect,
    pub selection_anchor: Option<Pos>,
    drag_origin: Option<Pos>,
    clipboard: Option<Clipboard>,
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
            selection_anchor: None,
            drag_origin: None,
            clipboard: None,
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

    fn fill_bounds(&mut self, bounds: Bounds, ch: char) {
        for y in bounds.min_y..=bounds.max_y {
            for x in bounds.min_x..=bounds.max_x {
                self.canvas.set(Pos { x, y }, ch);
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
        let bounds = self.selection_or_cursor_bounds();
        self.clipboard = Some(self.capture_bounds(bounds));
    }

    fn cut_selection_or_cell(&mut self) {
        let bounds = self.selection_or_cursor_bounds();
        self.clipboard = Some(self.capture_bounds(bounds));
        self.fill_bounds(bounds, ' ');
    }

    fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.clipboard.as_ref() else {
            return;
        };

        for y in 0..clipboard.height {
            for x in 0..clipboard.width {
                let target_x = self.cursor.x + x;
                let target_y = self.cursor.y + y;
                if target_x >= self.canvas.width || target_y >= self.canvas.height {
                    continue;
                }
                self.canvas.set(
                    Pos {
                        x: target_x,
                        y: target_y,
                    },
                    clipboard.get(x, y),
                );
            }
        }
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
        self.fill_bounds(bounds, ch);
    }

    fn draw_border(&mut self) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };

        if bounds.width() == 1 && bounds.height() == 1 {
            self.canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.min_y,
                },
                '*',
            );
            return;
        }

        if bounds.height() == 1 {
            self.canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.min_y,
                },
                '.',
            );
            for x in (bounds.min_x + 1)..bounds.max_x {
                self.canvas.set(Pos { x, y: bounds.min_y }, '-');
            }
            self.canvas.set(
                Pos {
                    x: bounds.max_x,
                    y: bounds.min_y,
                },
                '.',
            );
            return;
        }

        if bounds.width() == 1 {
            self.canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.min_y,
                },
                '.',
            );
            for y in (bounds.min_y + 1)..bounds.max_y {
                self.canvas.set(Pos { x: bounds.min_x, y }, '|');
            }
            self.canvas.set(
                Pos {
                    x: bounds.min_x,
                    y: bounds.max_y,
                },
                '`',
            );
            return;
        }

        self.canvas.set(
            Pos {
                x: bounds.min_x,
                y: bounds.min_y,
            },
            '.',
        );
        self.canvas.set(
            Pos {
                x: bounds.max_x,
                y: bounds.min_y,
            },
            '.',
        );
        self.canvas.set(
            Pos {
                x: bounds.min_x,
                y: bounds.max_y,
            },
            '`',
        );
        self.canvas.set(
            Pos {
                x: bounds.max_x,
                y: bounds.max_y,
            },
            '\'',
        );

        for x in (bounds.min_x + 1)..bounds.max_x {
            self.canvas.set(Pos { x, y: bounds.min_y }, '-');
            self.canvas.set(Pos { x, y: bounds.max_y }, '-');
        }

        for y in (bounds.min_y + 1)..bounds.max_y {
            self.canvas.set(Pos { x: bounds.min_x, y }, '|');
            self.canvas.set(Pos { x: bounds.max_x, y }, '|');
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if self.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::F(1) => self.show_help = false,
                        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.show_help = false
                        }
                        _ => {}
                    }
                    return;
                }

                if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.should_quit = true;
                } else if key.code == KeyCode::Char('e')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
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
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(pos) = canvas_pos {
                            if self.mode.is_selecting() {
                                self.clear_selection();
                            }
                            self.cursor = pos;
                            self.drag_origin = Some(pos);
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

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Backspace | KeyCode::Char('h') => {
                    self.canvas.push_left(self.cursor.y, self.cursor.x);
                    return;
                }
                KeyCode::Char('j') => {
                    self.canvas.push_down(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('k') => {
                    self.canvas.push_up(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('l') => {
                    self.canvas.push_right(self.cursor.y, self.cursor.x);
                    return;
                }
                KeyCode::Char('y') => {
                    self.canvas.pull_from_left(self.cursor.y, self.cursor.x);
                    return;
                }
                KeyCode::Char('u') => {
                    self.canvas.pull_from_down(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Tab | KeyCode::Char('i') => {
                    self.canvas.pull_from_up(self.cursor.x, self.cursor.y);
                    return;
                }
                KeyCode::Char('o') => {
                    self.canvas.pull_from_right(self.cursor.y, self.cursor.x);
                    return;
                }
                KeyCode::Char('c') => {
                    self.copy_selection_or_cell();
                    return;
                }
                KeyCode::Char('x') => {
                    self.cut_selection_or_cell();
                    return;
                }
                KeyCode::Char('v') => {
                    self.paste_clipboard();
                    return;
                }
                KeyCode::Char('b') => {
                    self.draw_border();
                    return;
                }
                KeyCode::Char(' ') | KeyCode::Null => {
                    self.smart_fill();
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

        if is_move && shift {
            self.begin_selection();
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

        if is_move && self.mode.is_selecting() {
            self.clear_selection();
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
            KeyCode::Esc => self.clear_selection(),
            _ if self.mode.is_selecting() && self.selection_anchor.is_some() => match key.code {
                KeyCode::Char(ch) => self.fill_bounds(self.selection_or_cursor_bounds(), ch),
                KeyCode::Backspace | KeyCode::Delete => {
                    self.fill_bounds(self.selection_or_cursor_bounds(), ' ')
                }
                _ => {}
            },
            _ => match key.code {
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
}
