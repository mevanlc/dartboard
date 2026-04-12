use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: usize,
    pub y: usize,
}

pub const DEFAULT_WIDTH: usize = 176;
pub const DEFAULT_HEIGHT: usize = 48;

pub struct Canvas {
    cells: HashMap<Pos, char>,
    pub width: usize,
    pub height: usize,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }

    pub fn set(&mut self, pos: Pos, ch: char) {
        if ch == ' ' {
            self.cells.remove(&pos);
        } else {
            self.cells.insert(pos, ch);
        }
    }

    pub fn clear(&mut self, pos: Pos) {
        self.cells.remove(&pos);
    }

    pub fn get(&self, pos: Pos) -> char {
        self.cells.get(&pos).copied().unwrap_or(' ')
    }

    #[allow(dead_code)] // will be used for network sync
    pub fn iter(&self) -> impl Iterator<Item = (&Pos, &char)> {
        self.cells.iter()
    }

    pub fn row_occupied(&self, y: usize) -> Vec<usize> {
        let mut xs: Vec<usize> = self.cells.keys().filter(|p| p.y == y).map(|p| p.x).collect();
        xs.sort();
        xs
    }

    pub fn row_content(&self, y: usize) -> Vec<char> {
        let positions = self.row_occupied(y);
        if positions.is_empty() {
            return vec![];
        }
        let max_x = *positions.last().unwrap();
        let mut row = vec![' '; max_x + 1];
        for x in positions {
            row[x] = self.get(Pos { x, y });
        }
        row
    }

    pub fn shift_right(&mut self, y: usize, from_x: usize) {
        let mut xs: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.y == y && p.x >= from_x)
            .map(|p| p.x)
            .collect();
        xs.sort_by(|a, b| b.cmp(a));
        for x in xs {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x: x + 1, y }, ch);
            }
        }
    }

    /// Shift all cells in column `x` at row >= `from_y` down by one row.
    pub fn shift_col_down(&mut self, x: usize, from_y: usize) {
        let mut ys: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.x == x && p.y >= from_y)
            .map(|p| p.y)
            .collect();
        ys.sort_by(|a, b| b.cmp(a));
        for y in ys {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x, y: y + 1 }, ch);
            }
        }
    }

    /// Pull all cells in column `x` at row > `from_y` up by one row.
    /// The cell at `from_y` is overwritten (consumed).
    pub fn shift_col_up(&mut self, x: usize, from_y: usize) {
        self.cells.remove(&Pos { x, y: from_y });
        let mut ys: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.x == x && p.y > from_y)
            .map(|p| p.y)
            .collect();
        ys.sort();
        for y in ys {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x, y: y - 1 }, ch);
            }
        }
    }

    pub fn shift_left(&mut self, y: usize, from_x: usize) {
        let mut xs: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.y == y && p.x > from_x)
            .map(|p| p.x)
            .collect();
        xs.sort();
        for x in xs {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x: x - 1, y }, ch);
            }
        }
    }
}
