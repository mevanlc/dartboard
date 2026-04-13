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

    pub fn push_left(&mut self, y: usize, to_x: usize) {
        let mut xs: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.y == y && p.x <= to_x)
            .map(|p| p.x)
            .collect();
        xs.sort();
        for x in xs {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                if x > 0 {
                    self.cells.insert(Pos { x: x - 1, y }, ch);
                }
            }
        }
    }

    pub fn push_right(&mut self, y: usize, from_x: usize) {
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

    pub fn push_up(&mut self, x: usize, to_y: usize) {
        let mut ys: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.x == x && p.y <= to_y)
            .map(|p| p.y)
            .collect();
        ys.sort();
        for y in ys {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                if y > 0 {
                    self.cells.insert(Pos { x, y: y - 1 }, ch);
                }
            }
        }
    }

    pub fn push_down(&mut self, x: usize, from_y: usize) {
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

    pub fn pull_from_left(&mut self, y: usize, to_x: usize) {
        self.cells.remove(&Pos { x: to_x, y });
        let mut xs: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.y == y && p.x < to_x)
            .map(|p| p.x)
            .collect();
        xs.sort_by(|a, b| b.cmp(a));
        for x in xs {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x: x + 1, y }, ch);
            }
        }
    }

    pub fn pull_from_right(&mut self, y: usize, from_x: usize) {
        self.cells.remove(&Pos { x: from_x, y });
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

    pub fn pull_from_up(&mut self, x: usize, to_y: usize) {
        self.cells.remove(&Pos { x, y: to_y });
        let mut ys: Vec<usize> = self
            .cells
            .keys()
            .filter(|p| p.x == x && p.y < to_y)
            .map(|p| p.y)
            .collect();
        ys.sort_by(|a, b| b.cmp(a));
        for y in ys {
            if let Some(ch) = self.cells.remove(&Pos { x, y }) {
                self.cells.insert(Pos { x, y: y + 1 }, ch);
            }
        }
    }

    pub fn pull_from_down(&mut self, x: usize, from_y: usize) {
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
}

#[cfg(test)]
mod tests {
    use super::{Canvas, Pos};

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
}
