//! Breadboard geometry, in grid units of 0.1 in (one hole pitch).
//!
//! Each column's a-e holes form one strip and its f-j holes another, with the trench between e and f. The
//! four rails (top+, top-, bot+, bot-) run along the board in groups of five; each rail is one strip unless
//! the board splits it in the middle.

pub type Xy = (f64, f64);

/// A row of holes: a-j, then the four rails, in the order Python's `(*ROW_Y, *RAILS)` lists them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub enum Row {
    A, B, C, D, E, F, G, H, I, J, TopP, TopM, BotP, BotM,
}

pub const ROWS: [Row; 14] = [Row::A, Row::B, Row::C, Row::D, Row::E, Row::F, Row::G, Row::H, Row::I, Row::J,
                             Row::TopP, Row::TopM, Row::BotP, Row::BotM];
pub const TERMINAL: [Row; 10] = [Row::A, Row::B, Row::C, Row::D, Row::E, Row::F, Row::G, Row::H, Row::I, Row::J];
pub const RAILS: [Row; 4] = [Row::TopP, Row::TopM, Row::BotP, Row::BotM];
pub const OUTLINE_Y: (f64, f64) = (-1.2, 18.2);
pub const STACK: f64 = 20.0;

impl Row {
    pub fn name(self) -> &'static str {
        ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "top+", "top-", "bot+", "bot-"][self as usize]
    }

    pub fn parse(s: &str) -> Option<Row> {
        ROWS.iter().copied().find(|r| r.name() == s)
    }

    pub fn is_rail(self) -> bool {
        self as u8 >= Row::TopP as u8
    }

    /// ROW_Y for a terminal row.
    pub fn terminal_y(self) -> i32 {
        [3, 4, 5, 6, 7, 10, 11, 12, 13, 14][self as usize]
    }

    /// a-e: the top half of the terminal area.
    pub fn top_half(self) -> bool {
        (self as u8) <= Row::E as u8
    }

    /// The rail's sign, '+' or '-' (rails only).
    pub fn sign(self) -> char {
        if matches!(self, Row::TopP | Row::BotP) { '+' } else { '-' }
    }

    /// "top" or "bot" (rails only).
    pub fn side(self) -> &'static str {
        if matches!(self, Row::TopP | Row::TopM) { "top" } else { "bot" }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Hole {
    pub board: u8,
    pub row: Row,
    pub col: i32,
}

impl Hole {
    pub fn new(board: u8, row: Row, col: i32) -> Hole {
        Hole { board, row, col }
    }

    pub fn is_rail(&self) -> bool {
        self.row.is_rail()
    }
}

/// What joins holes under the board: a column's a-e or f-j, or a rail (or one half of a split rail).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Strip {
    pub board: u8,
    /// 0: a-e, 1: f-j, else a rail's row
    pub kind: StripKind,
    pub n: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StripKind {
    AE,
    FJ,
    Rail(Row),
}

impl StripKind {
    pub fn name(self) -> &'static str {
        match self {
            StripKind::AE => "a-e",
            StripKind::FJ => "f-j",
            StripKind::Rail(r) => r.name(),
        }
    }

    pub fn rail(self) -> Option<Row> {
        match self {
            StripKind::Rail(r) => Some(r),
            _ => None,
        }
    }
}

/// `parse_hole`: 'a12', '2:a12', 'top+12' or a bare rail 'top+' -> (board, row, col).
pub fn parse_hole(text: &str) -> Option<(Option<String>, Row, Option<i32>)> {
    let t = text.trim();
    let (board, rest) = match t.find(':') {
        Some(i) => {
            let b = &t[..i];
            if b.is_empty() || b.chars().any(|c| c == ':' || c == '.' || c.is_whitespace()) {
                return None;
            }
            (Some(b.to_string()), &t[i + 1..])
        }
        None => (None, t),
    };
    let (row, digits) = if let Some(r) = ["top+", "top-", "bot+", "bot-"].iter().find(|r| rest.starts_with(**r)) {
        (Row::parse(r).unwrap(), &rest[r.len()..])
    } else {
        let c = rest.chars().next()?;
        if !('a'..='j').contains(&c) {
            return None;
        }
        (Row::parse(&rest[..1]).unwrap(), &rest[1..])
    };
    if digits.is_empty() {
        return Some((board, row, None));
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((board, row, Some(digits.parse().ok()?)))
}

#[derive(Clone, Debug)]
pub struct Board {
    pub id: String,
    pub index: u8,
    pub kind: String,
    pub split_rails: bool,
    pub columns: i32,
    pub rail_cols: Vec<i32>,
    rail_set: Vec<bool>,
    split_after: i32,
    pub rail_order_top: String,
    pub rail_order_bot: String,
    pub x0: f64,
    pub y0: f64,
}

impl Board {
    pub fn new(id: String, index: u8, kind: &str, top: &str, bot: &str, split_rails: bool) -> Result<Board, String> {
        let (columns, first, groups) = match kind {
            "full" => (63, 3, 10),
            "half" => (30, 2, 5),
            _ => return Err(format!("unknown board type '{}' (expected full or half)", kind)),
        };
        let rail_cols: Vec<i32> = (0..groups).flat_map(|g| (0..5).map(move |k| first + 6 * g + k)).collect();
        let mut rail_set = vec![false; (columns + 2) as usize];
        for &c in &rail_cols {
            rail_set[c as usize] = true;
        }
        let split_after = rail_cols[rail_cols.len() / 2 - 1];
        for (side, order) in [("top", top), ("bot", bot)] {
            let mut cs: Vec<char> = order.chars().collect();
            cs.sort();
            if cs != vec!['+', '-'] {
                return Err(format!("rails.{} must be '+-' or '-+' (top to bottom), not '{}'", side, order));
            }
        }
        Ok(Board {
            id, index, kind: kind.to_string(), split_rails, columns, rail_cols, rail_set, split_after,
            rail_order_top: top.to_string(), rail_order_bot: bot.to_string(), x0: 0.0, y0: 0.0,
        })
    }

    pub fn left(&self) -> f64 {
        self.x0 - 2.0
    }

    pub fn right(&self) -> f64 {
        self.x0 + self.columns as f64 + 1.0
    }

    pub fn top(&self) -> f64 {
        self.y0 + OUTLINE_Y.0
    }

    pub fn bottom(&self) -> f64 {
        self.y0 + OUTLINE_Y.1
    }

    pub fn rail_y(&self, rail: Row) -> i32 {
        let (pair, order) = if rail.side() == "top" { ((0, 1), &self.rail_order_top) } else { ((16, 17), &self.rail_order_bot) };
        if order.chars().position(|c| c == rail.sign()).unwrap() == 0 { pair.0 } else { pair.1 }
    }

    /// Where a row of holes is, down the drawing.
    #[inline]
    pub fn y(&self, row: Row) -> f64 {
        if row.is_rail() {
            self.y0 + self.rail_y(row) as f64
        } else {
            self.y0 + row.terminal_y() as f64
        }
    }

    #[inline]
    pub fn has(&self, row: Row, col: i32) -> bool {
        if !row.is_rail() {
            return 1 <= col && col <= self.columns;
        }
        col >= 0 && (col as usize) < self.rail_set.len() && self.rail_set[col as usize]
    }

    #[inline]
    pub fn xy(&self, hole: Hole) -> Xy {
        (self.x0 + hole.col as f64 - 1.0, self.y(hole.row))
    }

    pub fn hole_at(&self, x: f64, y: f64) -> Option<Hole> {
        let col = x - self.x0 + 1.0;
        if (col - col.round_ties_even()).abs() > 1e-6 {
            return None;
        }
        let row = ROWS.iter().copied().find(|&r| (self.y(r) - y).abs() < 1e-6)?;
        let c = col.round_ties_even() as i32;
        if self.has(row, c) {
            Some(Hole::new(self.index, row, c))
        } else {
            None
        }
    }

    pub fn strip(&self, hole: Hole) -> Strip {
        if !hole.row.is_rail() {
            let kind = if hole.row.top_half() { StripKind::AE } else { StripKind::FJ };
            return Strip { board: self.index, kind, n: hole.col };
        }
        Strip { board: self.index, kind: StripKind::Rail(hole.row), n: (self.split_rails && hole.col > self.split_after) as i32 }
    }

    pub fn strip_holes(&self, strip: Strip) -> Vec<Hole> {
        match strip.kind {
            StripKind::AE => [Row::A, Row::B, Row::C, Row::D, Row::E].iter().map(|&r| Hole::new(self.index, r, strip.n)).collect(),
            StripKind::FJ => [Row::F, Row::G, Row::H, Row::I, Row::J].iter().map(|&r| Hole::new(self.index, r, strip.n)).collect(),
            StripKind::Rail(row) => self.rail_cols.iter().map(|&c| Hole::new(self.index, row, c))
                .filter(|h| self.strip(*h) == strip).collect(),
        }
    }

    /// Every hole: the terminal rows column by column within each row, then the rails.
    pub fn holes(&self) -> Vec<Hole> {
        let mut out = Vec::with_capacity(10 * self.columns as usize + 4 * self.rail_cols.len());
        for row in TERMINAL {
            for col in 1..=self.columns {
                out.push(Hole::new(self.index, row, col));
            }
        }
        for rail in RAILS {
            for &col in &self.rail_cols {
                out.push(Hole::new(self.index, rail, col));
            }
        }
        out
    }

    pub fn name(&self, hole: Hole, default_board: u8) -> String {
        if hole.board == default_board {
            format!("{}{}", hole.row.name(), hole.col)
        } else {
            format!("{}:{}{}", self.id, hole.row.name(), hole.col)
        }
    }
}
