use std::collections::HashMap;

use ratatui::style::Color;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: usize,
    pub y: usize,
}

pub const DEFAULT_WIDTH: usize = 352;
pub const DEFAULT_HEIGHT: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellValue {
    Narrow(char),
    Wide(char),
    WideCont,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub pos: Pos,
    pub ch: char,
    pub width: usize,
    pub fg: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    cells: HashMap<Pos, CellValue>,
    colors: HashMap<Pos, Color>,
    pub width: usize,
    pub height: usize,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            colors: HashMap::new(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }

    pub fn display_width(ch: char) -> usize {
        UnicodeWidthChar::width(ch).unwrap_or(1).clamp(1, 2)
    }

    pub fn cell(&self, pos: Pos) -> Option<CellValue> {
        self.cells.get(&pos).copied()
    }

    pub fn fg(&self, pos: Pos) -> Option<Color> {
        let origin = self.glyph_origin(pos)?;
        self.colors.get(&origin).copied()
    }

    pub fn is_continuation(&self, pos: Pos) -> bool {
        matches!(self.cell(pos), Some(CellValue::WideCont))
    }

    pub fn glyph_origin(&self, pos: Pos) -> Option<Pos> {
        match self.cell(pos) {
            Some(CellValue::Narrow(_) | CellValue::Wide(_)) => Some(pos),
            Some(CellValue::WideCont) if pos.x > 0 => Some(Pos {
                x: pos.x - 1,
                y: pos.y,
            }),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn glyph_at(&self, pos: Pos) -> Option<Glyph> {
        let origin = self.glyph_origin(pos)?;
        match self.cell(origin)? {
            CellValue::Narrow(ch) => Some(Glyph {
                pos: origin,
                ch,
                width: 1,
                fg: self.colors.get(&origin).copied(),
            }),
            CellValue::Wide(ch) => Some(Glyph {
                pos: origin,
                ch,
                width: 2,
                fg: self.colors.get(&origin).copied(),
            }),
            CellValue::WideCont => None,
        }
    }

    fn clear_glyph_at_origin(&mut self, origin: Pos) {
        match self.cell(origin) {
            Some(CellValue::Narrow(_)) => {
                self.cells.remove(&origin);
                self.colors.remove(&origin);
            }
            Some(CellValue::Wide(_)) => {
                self.cells.remove(&origin);
                self.cells.remove(&Pos {
                    x: origin.x + 1,
                    y: origin.y,
                });
                self.colors.remove(&origin);
            }
            _ => {}
        }
    }

    pub fn clear_cell(&mut self, pos: Pos) {
        if let Some(origin) = self.glyph_origin(pos) {
            self.clear_glyph_at_origin(origin);
        }
    }

    #[allow(dead_code)]
    pub fn put_glyph(&mut self, pos: Pos, ch: char) -> bool {
        self.put_glyph_with_optional_color(pos, ch, None)
    }

    pub fn put_glyph_colored(&mut self, pos: Pos, ch: char, fg: Color) -> bool {
        self.put_glyph_with_optional_color(pos, ch, Some(fg))
    }

    fn put_glyph_with_optional_color(&mut self, pos: Pos, ch: char, fg: Option<Color>) -> bool {
        if pos.x >= self.width || pos.y >= self.height {
            return false;
        }
        if ch == ' ' {
            self.clear_cell(pos);
            return true;
        }

        let width = Self::display_width(ch);
        if width == 2 && pos.x + 1 >= self.width {
            return false;
        }

        self.clear_cell(pos);
        if width == 2 {
            self.clear_cell(Pos {
                x: pos.x + 1,
                y: pos.y,
            });
        }

        self.cells.insert(
            pos,
            if width == 2 {
                CellValue::Wide(ch)
            } else {
                CellValue::Narrow(ch)
            },
        );
        if width == 2 {
            self.cells.insert(
                Pos {
                    x: pos.x + 1,
                    y: pos.y,
                },
                CellValue::WideCont,
            );
        }
        if let Some(color) = fg {
            self.colors.insert(pos, color);
        } else {
            self.colors.remove(&pos);
        }
        true
    }

    #[allow(dead_code)]
    pub fn set(&mut self, pos: Pos, ch: char) {
        let _ = self.put_glyph(pos, ch);
    }

    pub fn set_colored(&mut self, pos: Pos, ch: char, fg: Color) {
        let _ = self.put_glyph_colored(pos, ch, fg);
    }

    pub fn clear(&mut self, pos: Pos) {
        self.clear_cell(pos);
    }

    #[allow(dead_code)]
    pub fn get(&self, pos: Pos) -> char {
        match self.cell(pos) {
            Some(CellValue::Narrow(ch) | CellValue::Wide(ch)) => ch,
            _ => ' ',
        }
    }

    #[allow(dead_code)] // will be used for network sync
    pub fn iter(&self) -> impl Iterator<Item = (&Pos, &CellValue)> {
        self.cells.iter()
    }

    fn glyphs(&self) -> Vec<Glyph> {
        let mut glyphs: Vec<_> = self
            .cells
            .iter()
            .filter_map(|(pos, cell)| match cell {
                CellValue::Narrow(ch) => Some(Glyph {
                    pos: *pos,
                    ch: *ch,
                    width: 1,
                    fg: self.colors.get(pos).copied(),
                }),
                CellValue::Wide(ch) => Some(Glyph {
                    pos: *pos,
                    ch: *ch,
                    width: 2,
                    fg: self.colors.get(pos).copied(),
                }),
                CellValue::WideCont => None,
            })
            .collect();
        glyphs.sort_by_key(|glyph| (glyph.pos.y, glyph.pos.x));
        glyphs
    }

    fn can_place_glyph(&self, glyph: &Glyph) -> bool {
        glyph.pos.x < self.width
            && glyph.pos.y < self.height
            && glyph.pos.x + glyph.width <= self.width
            && glyph.width <= 2
    }

    fn rebuild_from_glyphs(&mut self, glyphs: Vec<Glyph>) {
        self.cells.clear();
        self.colors.clear();
        for glyph in glyphs {
            if self.can_place_glyph(&glyph) {
                let _ = self.put_glyph_with_optional_color(glyph.pos, glyph.ch, glyph.fg);
            }
        }
    }

    pub fn push_left(&mut self, y: usize, to_x: usize) {
        let mut glyphs = self.glyphs();
        for glyph in &mut glyphs {
            if glyph.pos.y == y && glyph.pos.x <= to_x {
                if glyph.pos.x == 0 {
                    glyph.width = 0;
                } else {
                    glyph.pos.x -= 1;
                }
            }
        }
        self.rebuild_from_glyphs(glyphs.into_iter().filter(|g| g.width > 0).collect());
    }

    pub fn push_right(&mut self, y: usize, from_x: usize) {
        let mut glyphs = self.glyphs();
        for glyph in &mut glyphs {
            if glyph.pos.y == y && glyph.pos.x >= from_x {
                glyph.pos.x += 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }

    pub fn push_up(&mut self, x: usize, to_y: usize) {
        let mut glyphs = self.glyphs();
        for glyph in &mut glyphs {
            let covers_x = x >= glyph.pos.x && x < glyph.pos.x + glyph.width;
            if covers_x && glyph.pos.y <= to_y {
                if glyph.pos.y == 0 {
                    glyph.width = 0;
                } else {
                    glyph.pos.y -= 1;
                }
            }
        }
        self.rebuild_from_glyphs(glyphs.into_iter().filter(|g| g.width > 0).collect());
    }

    pub fn push_down(&mut self, x: usize, from_y: usize) {
        let mut glyphs = self.glyphs();
        for glyph in &mut glyphs {
            let covers_x = x >= glyph.pos.x && x < glyph.pos.x + glyph.width;
            if covers_x && glyph.pos.y >= from_y {
                glyph.pos.y += 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }

    pub fn pull_from_left(&mut self, y: usize, to_x: usize) {
        let remove_origin = self.glyph_origin(Pos { x: to_x, y });
        let mut glyphs = self.glyphs();
        glyphs.retain(|glyph| Some(glyph.pos) != remove_origin);
        for glyph in &mut glyphs {
            if glyph.pos.y == y && glyph.pos.x < to_x {
                glyph.pos.x += 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }

    pub fn pull_from_right(&mut self, y: usize, from_x: usize) {
        let remove_origin = self.glyph_origin(Pos { x: from_x, y });
        let mut glyphs = self.glyphs();
        glyphs.retain(|glyph| Some(glyph.pos) != remove_origin);
        for glyph in &mut glyphs {
            if glyph.pos.y == y && glyph.pos.x > from_x {
                glyph.pos.x -= 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }

    pub fn pull_from_up(&mut self, x: usize, to_y: usize) {
        let remove_origin = self.glyph_origin(Pos { x, y: to_y });
        let mut glyphs = self.glyphs();
        glyphs.retain(|glyph| Some(glyph.pos) != remove_origin);
        for glyph in &mut glyphs {
            let covers_x = x >= glyph.pos.x && x < glyph.pos.x + glyph.width;
            if covers_x && glyph.pos.y < to_y {
                glyph.pos.y += 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }

    pub fn pull_from_down(&mut self, x: usize, from_y: usize) {
        let remove_origin = self.glyph_origin(Pos { x, y: from_y });
        let mut glyphs = self.glyphs();
        glyphs.retain(|glyph| Some(glyph.pos) != remove_origin);
        for glyph in &mut glyphs {
            let covers_x = x >= glyph.pos.x && x < glyph.pos.x + glyph.width;
            if covers_x && glyph.pos.y > from_y {
                glyph.pos.y -= 1;
            }
        }
        self.rebuild_from_glyphs(glyphs);
    }
}

#[cfg(test)]
mod tests {
    use super::{Canvas, CellValue, Pos};
    use ratatui::style::Color;

    #[test]
    fn row_push_and_pull_are_directional() {
        let mut canvas = Canvas::new();
        canvas.set(Pos { x: 0, y: 0 }, 'A');
        canvas.set(Pos { x: 1, y: 0 }, 'B');
        canvas.set(Pos { x: 2, y: 0 }, 'C');
        canvas.set(Pos { x: 3, y: 0 }, 'D');

        canvas.push_left(0, 2);
        assert_eq!(canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(canvas.get(Pos { x: 1, y: 0 }), 'C');
        assert_eq!(canvas.get(Pos { x: 2, y: 0 }), ' ');
        assert_eq!(canvas.get(Pos { x: 3, y: 0 }), 'D');

        canvas.pull_from_right(0, 1);
        assert_eq!(canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(canvas.get(Pos { x: 1, y: 0 }), ' ');
        assert_eq!(canvas.get(Pos { x: 2, y: 0 }), 'D');
    }

    #[test]
    fn column_push_and_pull_are_directional() {
        let mut canvas = Canvas::new();
        canvas.set(Pos { x: 0, y: 0 }, 'A');
        canvas.set(Pos { x: 0, y: 1 }, 'B');
        canvas.set(Pos { x: 0, y: 2 }, 'C');
        canvas.set(Pos { x: 0, y: 3 }, 'D');

        canvas.push_up(0, 2);
        assert_eq!(canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(canvas.get(Pos { x: 0, y: 1 }), 'C');
        assert_eq!(canvas.get(Pos { x: 0, y: 2 }), ' ');
        assert_eq!(canvas.get(Pos { x: 0, y: 3 }), 'D');

        canvas.pull_from_down(0, 1);
        assert_eq!(canvas.get(Pos { x: 0, y: 0 }), 'B');
        assert_eq!(canvas.get(Pos { x: 0, y: 1 }), ' ');
        assert_eq!(canvas.get(Pos { x: 0, y: 2 }), 'D');
    }

    #[test]
    fn wide_glyph_occupies_owner_and_continuation_cells() {
        let mut canvas = Canvas::new();
        canvas.set(Pos { x: 3, y: 2 }, '🌱');

        assert_eq!(canvas.cell(Pos { x: 3, y: 2 }), Some(CellValue::Wide('🌱')));
        assert_eq!(canvas.cell(Pos { x: 4, y: 2 }), Some(CellValue::WideCont));
        assert_eq!(canvas.get(Pos { x: 3, y: 2 }), '🌱');
        assert_eq!(canvas.get(Pos { x: 4, y: 2 }), ' ');
    }

    #[test]
    fn clearing_continuation_clears_the_whole_wide_glyph() {
        let mut canvas = Canvas::new();
        canvas.set(Pos { x: 1, y: 1 }, '🌱');

        canvas.clear(Pos { x: 2, y: 1 });

        assert_eq!(canvas.get(Pos { x: 1, y: 1 }), ' ');
        assert_eq!(canvas.get(Pos { x: 2, y: 1 }), ' ');
    }

    #[test]
    fn colored_glyph_exposes_foreground_on_origin_and_continuation() {
        let mut canvas = Canvas::new();
        let color = Color::Rgb(84, 196, 255);

        canvas.set_colored(Pos { x: 3, y: 2 }, '🌱', color);

        assert_eq!(canvas.fg(Pos { x: 3, y: 2 }), Some(color));
        assert_eq!(canvas.fg(Pos { x: 4, y: 2 }), Some(color));
    }

    #[test]
    fn directional_push_preserves_glyph_color() {
        let mut canvas = Canvas::new();
        let color = Color::Rgb(192, 132, 255);

        canvas.set_colored(Pos { x: 1, y: 0 }, 'A', color);
        canvas.push_left(0, 1);

        assert_eq!(canvas.get(Pos { x: 0, y: 0 }), 'A');
        assert_eq!(canvas.fg(Pos { x: 0, y: 0 }), Some(color));
    }
}
