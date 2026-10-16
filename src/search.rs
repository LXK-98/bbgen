//! Placing what the file leaves unplaced: footprint parts without `at`, two-lead parts without `pins`.
//!
//! Footprint parts are placed before the circuit is built. A quick estimate scores a placement: how
//! far each net's pins are apart, how far pins on a rail's net (or a two-lead part's trip to a rail)
//! are from that rail and whether their column has rail holes at all, how many signal wires would
//! run side by side past one column, how close parts sit, and how many signal nets have pins either
//! side of a part no wire can lie across (a display whose pins' strips are under its body). Annealing
//! from a few fixed seeds finds good placements by that estimate; the best few are built for real, wires and all.
//!
//! Two-lead parts are placed while the circuit is built, after the footprint parts: each leg goes in
//! a strip or on a rail of its net (or an empty strip, which a wire then joins to it), the pair chosen
//! to lie straight, at a natural span for the part, without covering the free holes its strips
//! still need or standing in a pin's way to its rail. Decoupling caps go next to the pin they serve.
//!
//! The best build is then repaired one move at a time, each move built and scored for real: another
//! leg pair for one two-lead part (the rest staying put), or a footprint part turned round, nudged or
//! moved to where the estimate likes it (its two-lead parts placed afresh). Parts named in warnings
//! are tried first; the search stops after a fixed number of builds, so the same file always gets
//! the same layout.
//!
//! Two-lead parts' legs are in legs.rs. Trials are built a few at a time on threads (BBGEN_JOBS, else one
//! per core) and taken in order, so the search stops where it would one at a time: the same file always
//! gets the same layout, however many cores build it.

use crate::board::{parse_hole, Board, Row, RAILS, TERMINAL};
use crate::circuit::{check_rail_count_for, library_for, net_polarity, pymax, Circuit};
use crate::costs::*;
use crate::dump;
use crate::parts::{body_rect, make_part, rotate, Kind, Part, Rect};
use crate::pyrand::{set2_order, Random};
use crate::val::Val;
use crate::{HashMap, HashSet, IndexMap};
use serde_json::{json, Value};
use std::rc::Rc;

const ROOM_REACH: i64 = 9;
const SEEDS: i64 = 4;
const STEPS: usize = 3000;
const KEEP: usize = 4;
const TURN_ALL: usize = 4;
const REPAIR_BUILDS: usize = 120;
const LOOKAHEAD: f64 = 60.0;
const LOOKAHEAD_BUILDS: usize = 12;
const LOOKAHEAD_TIMES: usize = 6;
const PATIENCE: usize = 40;

/// The top or bottom half of a board's terminal area ("bot" sorts first, as the strings do).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Half {
    Bot,
    Top,
}

fn half_of(y: i64) -> Half {
    if y < 8 { Half::Top } else { Half::Bot }
}

const ROW_YS: [i64; 10] = [3, 4, 5, 6, 7, 10, 11, 12, 13, 14];

/// A pin of the model's parts: (part, pin) by index.
type MPin = (usize, usize);

/// One way to place a footprint part: (at, rot, pins' (x, y), body spans, keep-out spans, walls, rows taken).
#[derive(Debug)]
pub struct Layout {
    pub at: String,
    /// x_of(at): pin 1's column along the model's row of boards
    pub x: i64,
    pub rot: i32,
    pub xy: Vec<(MPin, (i64, i64))>,
    pub spans: Vec<(Half, i64, i64)>,
    pub keep: Vec<(Half, i64, i64)>,
    pub walls: Vec<(Half, i64, i64)>,
    pub taken: HashMap<(Half, i64), i64>,
}

type View = (i64, i64, i64, i64); // (shift, dx, lo, hi)

/// Just enough of a circuit to score placements quickly.
pub struct Model {
    boards: Vec<(Board, i64)>,
    below: HashMap<String, usize>,
    above: HashMap<String, usize>,
    offset: HashMap<String, i64>,
    width: i64,
    gaps: HashSet<i64>,
    strips: HashSet<(Half, i64)>,
    pub parts: IndexMap<String, Part>,
    nets: IndexMap<String, Vec<MPin>>,
    net_of: HashMap<MPin, String>,
    pub free_supplies: Vec<String>,
    plan: IndexMap<String, Vec<String>>,
    rail_y: HashMap<Row, i64>,
    links: Vec<(String, String)>,
    to_rail: IndexMap<String, Vec<String>>,
    offboard: HashSet<String>,
    pub auto: Vec<String>,
    pub choices: Vec<Vec<Rc<Layout>>>,
    fixed: Vec<Rc<Layout>>,
    lo: std::cell::Cell<i64>,
    hi: std::cell::Cell<i64>,
}

fn ceil_i(x: f64) -> i64 {
    x.ceil() as i64
}

fn floor_i(x: f64) -> i64 {
    x.floor() as i64
}

impl Model {
    pub fn new(data: &Val) -> Result<Model, String> {
        let default_boards = Val::List(vec![Val::Map(vec![(Val::Str("type".into()), Val::Str("full".into()))])]);
        let specs = if data.at("boards").truthy() { data.at("boards") } else { &default_boards };
        let mut boards: Vec<(Board, i64)> = Vec::new();
        let (mut below, mut above) = (HashMap::default(), HashMap::default());
        let mut x0 = 0i64;
        for (i, spec) in specs.as_list().cloned().unwrap_or_default().iter().enumerate() {
            let spec = match spec {
                Val::Str(_) => Val::Map(vec![(Val::Str("type".into()), spec.clone())]),
                _ => spec.clone(),
            };
            let id = spec.get("id").map(Val::py_str).unwrap_or((i + 1).to_string());
            let kind = spec.get("type").map(Val::py_str).unwrap_or("full".into());
            let rails = spec.at("rails");
            let top = rails.get("top").map(Val::py_str).unwrap_or("+-".into());
            let bot = rails.get("bottom").or(rails.get("bot")).map(Val::py_str).unwrap_or("+-".into());
            let board = Board::new(id, i as u8, &kind, &top, &bot, spec.at("split_rails").truthy())?;
            if let Some(b) = spec.get("below").filter(|v| !v.is_null()) {
                if let Some(up) = boards.iter().position(|(x, _)| x.id == b.py_str()) {
                    below.insert(boards[up].0.id.clone(), boards.len());
                    above.insert(board.id.clone(), up);
                }
            }
            let cols = board.columns as i64;
            boards.push((board, x0));
            x0 += cols + 6;
        }
        let offset: HashMap<String, i64> = boards.iter().map(|(b, x)| (b.id.clone(), *x)).collect();
        let last = boards.last().unwrap();
        let width = last.1 + last.0.columns as i64;
        let mut gaps = HashSet::default();
        let mut strips = HashSet::default();
        for (b, x0) in &boards {
            for c in 1..=b.columns {
                if !b.rail_cols.contains(&c) {
                    gaps.insert(x0 + c as i64);
                }
                strips.insert((Half::Top, x0 + c as i64));
                strips.insert((Half::Bot, x0 + c as i64));
            }
        }
        let library = library_for(data);
        let mut parts: IndexMap<String, Part> = IndexMap::default();
        for (r, spec) in data.at("parts").as_map().cloned().unwrap_or_default() {
            let r = r.py_str();
            parts.insert(r.clone(), make_part(&r, &spec, library.as_deref())?);
        }
        let mut nets: IndexMap<String, Vec<MPin>> = IndexMap::default();
        let mut explicit: IndexMap<String, Vec<String>> = IndexMap::default();
        let mut polarity: IndexMap<String, Option<char>> = IndexMap::default();
        let mut guessed: HashSet<String> = HashSet::default();
        for (name, spec) in data.at("nets").as_map().cloned().unwrap_or_default() {
            let name = name.py_str();
            let spec = match spec {
                Val::List(_) => Val::Map(vec![(Val::Str("pins".into()), spec.clone())]),
                _ => spec,
            };
            let mut pins: Vec<MPin> = Vec::new();
            for text in spec.at("pins").as_list().cloned().unwrap_or_default() {
                let text = text.py_str();
                let (r, token) = text.split_once('.').unwrap_or((text.as_str(), ""));
                if let Some(pi) = parts.get_index_of(r) {
                    for k in parts[pi].find(token) {
                        if !pins.contains(&(pi, k)) {
                            pins.push((pi, k));
                        }
                    }
                }
            }
            nets.insert(name.clone(), pins);
            let (sign, physical, g) = net_polarity(&name, &spec)?;
            polarity.insert(name.clone(), sign);
            if g {
                guessed.insert(name.clone());
            }
            explicit.insert(name, physical.iter().map(|r| r.rsplit(':').next().unwrap().to_string()).collect());
        }
        let board_list: Vec<Board> = boards.iter().map(|(b, _)| b.clone()).collect();
        check_rail_count_for(&polarity, &guessed, &board_list)?;
        let mut net_of: HashMap<MPin, String> = HashMap::default();
        for (name, pins) in &nets {
            for &p in pins {
                net_of.insert(p, name.clone());
            }
        }
        let mut legs: IndexMap<String, Vec<String>> = IndexMap::default();
        for (pi, part) in parts.values().enumerate() {
            for (k, leg) in part.legs.iter().enumerate().take(part.pins.len()) {
                if let Some((_, row, _)) = parse_hole(leg) {
                    if let Some(net) = net_of.get(&(pi, k)) {
                        if row.is_rail() {
                            legs.entry(net.clone()).or_default().push(row.name().to_string());
                        }
                    }
                }
            }
        }
        let free_supplies: Vec<String> = polarity.iter().filter(|(n, p)| **p == Some('+')
            && explicit.get(*n).map(|e| e.is_empty()).unwrap_or(true) && legs.get(*n).map(|e| e.is_empty()).unwrap_or(true))
            .map(|(n, _)| n.clone()).collect();
        let order: Vec<String> = data.at("_rail_order").as_list().map(|l| l.iter().map(Val::py_str).collect()).unwrap_or_default();
        let plan = crate::autoplace::rail_plan(&polarity, &explicit, &legs, &order);
        let first = &boards[0].0;
        let rail_y: HashMap<Row, i64> = RAILS.iter().map(|&r| (r, first.rail_y(r) as i64)).collect();
        let mut links: Vec<(String, String)> = Vec::new();
        for (pi, part) in parts.values().enumerate() {
            if part.kind.is_lead() {
                let a = net_of.get(&(pi, 0)).cloned();
                let b = net_of.get(&(pi, 1)).cloned();
                if let (Some(a), Some(b)) = (a, b) {
                    links.push((a, b));
                }
            }
        }
        let mut to_rail: IndexMap<String, Vec<String>> = plan.iter().map(|(n, r)| (n.clone(), r.clone())).collect();
        for (a, b) in &links {
            for (x, y) in [(a, b), (b, a)] {
                if plan.contains_key(y) && !plan.contains_key(x) {
                    to_rail.entry(x.clone()).or_default().extend(plan[y].iter().cloned());
                }
            }
        }
        let offboard: HashSet<String> = nets.iter().filter(|(_, pins)| pins.iter().any(|&(pi, _)| parts[pi].kind == Kind::Offboard))
            .map(|(n, _)| n.clone()).collect();
        let auto: Vec<String> = parts.iter().filter(|(_, p)| p.kind.is_footprint() && p.at.as_deref().map(|a| a.is_empty()).unwrap_or(true))
            .map(|(r, _)| r.clone()).collect();
        let mut m = Model {
            boards, below, above, offset, width, gaps, strips, parts, nets, net_of, free_supplies, plan, rail_y, links,
            to_rail, offboard, auto, choices: vec![], fixed: vec![], lo: std::cell::Cell::new(1), hi: std::cell::Cell::new(0),
        };
        m.hi.set(m.boards[0].0.columns as i64);
        let mut choices = Vec::new();
        for r in m.auto.clone() {
            let pi = m.parts.get_index_of(&r).unwrap();
            let ch = m.choices_of(pi)?;
            if ch.is_empty() {
                return Err(format!("parts.{}: there's no place on the board where its pins all land in holes without two of \
                                    them sharing a strip; give it at:, or use another footprint", r));
            }
            choices.push(ch.into_iter().map(Rc::new).collect());
        }
        m.choices = choices;
        let mut fixed = Vec::new();
        for pi in 0..m.parts.len() {
            let part = &m.parts[pi];
            if part.kind.is_footprint() {
                if let Some(at) = part.at.clone().filter(|a| !a.is_empty()) {
                    if let Some((bid, row, Some(col))) = parse_hole(&at) {
                        if !row.is_rail() {
                            let bi = m.boards.iter().position(|(b, _)| Some(&b.id) == bid.as_ref()).unwrap_or(0);
                            if let Some(l) = m.layout(pi, row, col as i64, part.rot, bi) {
                                fixed.push(Rc::new(l));
                            }
                        }
                    }
                }
            }
        }
        m.fixed = fixed;
        Ok(m)
    }

    fn views(&self, bi: usize) -> Vec<View> {
        let board = &self.boards[bi].0;
        let off = self.offset[&board.id];
        let mut out = vec![(0, 0, off + 1, off + board.columns as i64)];
        for (other, shift) in [(self.below.get(&board.id), 20i64), (self.above.get(&board.id), -20i64)] {
            if let Some(&o) = other {
                let ob = &self.boards[o].0;
                let oo = self.offset[&ob.id];
                out.push((shift, oo - off, oo + 1, oo + ob.columns as i64));
            }
        }
        out
    }

    fn bx(rect: Rect, rot: i32, ax: i64, ay: i64) -> Rect {
        let corners: Vec<(f64, f64)> = [rect.0, rect.2].iter().flat_map(|&x| [rect.1, rect.3].map(|y| rotate((x, y), rot))).collect();
        let xs: Vec<f64> = corners.iter().map(|c| c.0).collect();
        let ys: Vec<f64> = corners.iter().map(|c| c.1).collect();
        (ax as f64 + crate::parts::min(&xs), ay as f64 + crate::parts::min(&ys), ax as f64 + crate::parts::max(&xs), ay as f64 + crate::parts::max(&ys))
    }

    fn covered(&self, rect: Option<Rect>, rot: i32, ax: i64, ay: i64, view: View) -> Option<(i64, i64, Vec<Half>)> {
        let rect = rect?;
        let (shift, dx, vlo, vhi) = view;
        let (x0, y0, x1, y1) = Model::bx(rect, rot, ax, ay);
        let lo = vlo.max(ceil_i(x0 + dx as f64 + 1e-6));
        let hi = vhi.min(floor_i(x1 + dx as f64 - 1e-6));
        let rows: Vec<i64> = ROW_YS.iter().copied().filter(|&y| y0 < (y + shift) as f64 && ((y + shift) as f64) < y1).collect();
        if lo > hi || rows.is_empty() {
            return None;
        }
        let mut halves: Vec<Half> = Vec::new();
        for y in rows {
            let h = half_of(y);
            if !halves.contains(&h) {
                halves.push(h);
            }
        }
        halves.sort();
        Some((lo, hi, halves))
    }

    fn taken(&self, part: &Part, rot: i32, ax: i64, ay: i64, xy: &[(MPin, (i64, i64))], views: &[View]) -> HashMap<(Half, i64), i64> {
        let mut taken: HashMap<(Half, i64), i64> = HashMap::default();
        for (_, (x, y)) in xy {
            *taken.entry((half_of(*y), *x)).or_insert(0) += 1;
        }
        let rect = if part.raised { None } else { body_rect(part) };
        if let Some(rect) = rect {
            let (x0, y0, x1, y1) = Model::bx(rect, rot, ax, ay);
            let pins: HashSet<(i64, i64)> = xy.iter().map(|(_, p)| *p).collect();
            for &(shift, dx, lo, hi) in views {
                for x in lo.max(ceil_i(x0 + dx as f64 + 1e-6))..=hi.min(floor_i(x1 + dx as f64 - 1e-6)) {
                    for &y in &ROW_YS {
                        if y0 < (y + shift) as f64 && ((y + shift) as f64) < y1 && !pins.contains(&(x, y)) {
                            *taken.entry((half_of(y), x)).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        taken
    }

    fn walls(&self, part: &Part, rot: i32, ax: i64, ay: i64, xy: &[(MPin, (i64, i64))], view: View) -> Vec<(Half, i64, i64)> {
        let rect = match body_rect(part) {
            Some(r) if !part.raised => r,
            _ => return vec![],
        };
        let (shift, dx, vlo, vhi) = view;
        let (x0, y0, x1, y1) = Model::bx(rect, rot, ax, ay);
        let lo = vlo.max(ceil_i(x0 + dx as f64 + 1e-6));
        let hi = vhi.min(floor_i(x1 + dx as f64 - 1e-6));
        if lo > hi {
            return vec![];
        }
        let under: HashSet<i64> = ROW_YS.iter().copied().filter(|&y| y0 < (y + shift) as f64 && ((y + shift) as f64) < y1).collect();
        let pins: HashSet<i64> = xy.iter().filter(|(_, (x, _))| vlo <= *x && *x <= vhi).map(|(_, (_, y))| *y).collect();
        let mut out = Vec::new();
        for (half, ys) in [(Half::Top, &ROW_YS[..5]), (Half::Bot, &ROW_YS[5..])] {
            let free: Vec<i64> = ys.iter().copied().filter(|y| !under.contains(y) && !pins.contains(y)).collect();
            if ys.iter().any(|y| under.contains(y)) && (free.is_empty() || (free.len() <= 1 && ys.iter().any(|y| pins.contains(y)))) {
                out.push((half, lo, hi));
            }
        }
        out
    }

    /// One placement of a part: pin 1 in row/col of board bi, or None if a pin misses the holes.
    fn layout(&self, pi: usize, row: Row, col: i64, rot: i32, bi: usize) -> Option<Layout> {
        let part = &self.parts[pi];
        let board = &self.boards[bi].0;
        let off = self.offset[&board.id];
        self.lo.set(off + 1);
        self.hi.set(off + board.columns as i64);
        let views = self.views(bi);
        let (ax, ay) = (off + col, row.terminal_y() as i64);
        let mut xy: Vec<(MPin, (i64, i64))> = Vec::with_capacity(part.pins.len());
        let mut view_of: Vec<View> = Vec::with_capacity(part.pins.len());
        for (k, pin) in part.pins.iter().enumerate() {
            let (dx, dy) = rotate(pin.offset, rot);
            let (x, y) = (ax + dx as i64, ay + dy as i64);
            let mut found = false;
            for &v in &views {
                let (shift, vdx, lo, hi) = v;
                if ROW_YS.contains(&(y - shift)) && lo <= x + vdx && x + vdx <= hi {
                    xy.push(((pi, k), (x + vdx, y - shift)));
                    view_of.push(v);
                    found = true;
                    break;
                }
            }
            if !found {
                return None;
            }
        }
        if matches!(part.kind, Kind::Sip | Kind::Kicad) {
            let mut strips: Vec<((i64, bool), usize)> = Vec::new();
            for &((_, k), (x, y)) in &xy {
                let key = (x, y < 8);
                match strips.iter().find(|(s, _)| *s == key) {
                    None => strips.push((key, k)),
                    Some(&(_, other)) => {
                        // two pins in one strip: only if they're joined inside the part anyway (the same bus)
                        let bus = |p: usize| part.buses.iter().position(|b| b.contains(&p));
                        if other != k && (bus(k).is_none() || bus(k) != bus(other)) {
                            return None;
                        }
                    }
                }
            }
        }
        let body = body_rect(part);
        if let Some(r) = body {
            let corners = [rotate((r.0, r.1), rot), rotate((r.2, r.3), rot)];
            let mn = crate::circuit::pymin(ax as f64 + corners[0].0, ax as f64 + corners[1].0);
            let mx = pymax(ax as f64 + corners[0].0, ax as f64 + corners[1].0);
            if mn < self.lo.get() as f64 - 0.6 || mx > self.hi.get() as f64 + 0.6 {
                return None;
            }
        }
        let across = {
            let mut distinct: Vec<View> = Vec::new();
            for v in &view_of {
                if !distinct.contains(v) {
                    distinct.push(*v);
                }
            }
            distinct.len() > 1
        };
        let mut spans: Vec<(Half, i64, i64)> = Vec::new();
        for &v in &views {
            let mine: Vec<(i64, i64)> = xy.iter().zip(&view_of).filter(|(_, w)| **w == v).map(|((_, p), _)| *p).collect();
            let cov = self.covered(body, rot, ax, ay, v);
            if mine.is_empty() && cov.is_none() {
                continue;
            }
            let mut xs: Vec<i64> = mine.iter().map(|p| p.0).collect();
            let mut halves: Vec<Half> = Vec::new();
            for p in &mine {
                let h = half_of(p.1);
                if !halves.contains(&h) {
                    halves.push(h);
                }
            }
            if let Some((lo, hi, hs)) = &cov {
                xs.push(*lo);
                xs.push(*hi);
                for h in hs {
                    if !halves.contains(h) {
                        halves.push(*h);
                    }
                }
            }
            if matches!(part.kind, Kind::Pico | Kind::Dip) && !across {
                halves = vec![Half::Bot, Half::Top];
            }
            halves.sort();
            let (mn, mx) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
            for h in halves {
                spans.push((h, mn, mx));
            }
        }
        spans.sort_by_key(|s| (s.1, s.0));
        let lo = spans.iter().map(|s| s.1).min().unwrap();
        let hi = spans.iter().map(|s| s.2).max().unwrap();
        let mut keep = Vec::new();
        for (what, rect) in &part.keepouts {
            if let Some((mut klo, mut khi, hs)) = self.covered(Some(*rect), rot, ax, ay, views[0]) {
                if what.to_lowercase().contains("cable") {
                    if khi < lo {
                        klo = self.lo.get();
                    } else if klo > hi {
                        khi = self.hi.get();
                    }
                }
                for h in hs {
                    keep.push((h, klo, khi));
                }
            }
        }
        let prefix = if bi == 0 { String::new() } else { format!("{}:", board.id) };
        let walls: Vec<(Half, i64, i64)> = views.iter().flat_map(|&v| self.walls(part, rot, ax, ay, &xy, v)).collect();
        let taken = self.taken(part, rot, ax, ay, &xy, &views);
        Some(Layout { at: format!("{}{}{}", prefix, row.name(), col), x: ax, rot, xy, spans, keep, walls, taken })
    }

    fn choices_of(&self, pi: usize) -> Result<Vec<Layout>, String> {
        let mut out = Vec::new();
        for bi in 0..self.boards.len() {
            out.extend(self.choices_on(pi, bi)?);
        }
        Ok(out)
    }

    fn choices_on(&self, pi: usize, bi: usize) -> Result<Vec<Layout>, String> {
        let part = &self.parts[pi];
        let cols: Vec<i64> = (1..=self.boards[bi].0.columns as i64).collect();
        let rows = |s: &str| -> Vec<Row> { s.chars().map(|c| Row::parse(&c.to_string()).unwrap()).collect() };
        let mut spots: Vec<(Row, i64, i32)> = Vec::new();
        match part.kind {
            Kind::Pico => {
                for r in rows("ghi") { for &c in &cols { spots.push((r, c, 0)); } }
                for r in rows("bcd") { for &c in &cols { spots.push((r, c, 180)); } }
            }
            Kind::Dip if part.width == 3 => {
                for &c in &cols { spots.push((Row::F, c, 0)); }
                for &c in &cols { spots.push((Row::E, c, 180)); }
            }
            Kind::Kicad => {
                for r in TERMINAL { for &c in &cols { for rot in [0, 90, 180, 270] { spots.push((r, c, rot)); } } }
            }
            _ => {
                for r in TERMINAL { for &c in &cols { for rot in [0, 180] { spots.push((r, c, rot)); } } }
            }
        }
        let mut out: Vec<Layout> = spots.into_iter().filter_map(|(r, c, rot)| self.layout(pi, r, c, rot, bi)).collect();
        let straddles = |l: &Layout| {
            let top = l.xy.iter().any(|(_, (_, y))| *y < 8);
            let bot = l.xy.iter().any(|(_, (_, y))| *y >= 8);
            top && bot
        };
        if part.kind == Kind::Dip && part.width != 3 {
            out.retain(|o| straddles(o) && o.xy.iter().all(|(_, (x, y))| o.taken.get(&(half_of(*y), *x)).copied().unwrap_or(0) < 5));
            if out.is_empty() {
                return Err(format!("parts.{}: a {} in DIP can't straddle the trench with a free hole by each pin; give it at:",
                                   part.r#ref, crate::pyf::repr(part.width as f64 / 10.0).trim_end_matches(".0")));
            }
        }
        if part.kind == Kind::Kicad {
            let room = |l: &Layout| l.xy.iter().map(|(_, (_, y))| if *y < 8 { y - 3 } else { 14 - y }).min().unwrap();
            let mut best: Vec<((i32, bool), i64)> = Vec::new();
            for o in &out {
                let s = straddles(o);
                let v = if s { room(o) } else { 0 };
                match best.iter_mut().find(|(k, _)| *k == (o.rot, s)) {
                    Some(slot) => slot.1 = slot.1.max(v),
                    None => best.push(((o.rot, s), (-1i64).max(v))),
                }
            }
            let get = |k: (i32, bool)| best.iter().find(|(x, _)| *x == k).map(|(_, v)| *v).unwrap();
            out.retain(|o| !straddles(o) || room(o) >= get((o.rot, true)) - 1);
        }
        Ok(out)
    }

    /// Where along the boards a layout's pin 1 is ("2:f10" is column 10 of board 2).
    pub fn x_of(&self, at: &str) -> i64 {
        let (b, _, col) = parse_hole(at).unwrap();
        let id = b.unwrap_or(self.boards[0].0.id.clone());
        self.offset.get(&id).copied().unwrap_or(0) + col.unwrap() as i64
    }

    /// The estimate for a placement of the parts bbgen places (in this order), the file's own placements first.
    pub fn cost(&self, state: &[(usize, usize)]) -> f64 {
        let layouts: Vec<&Layout> = self.fixed.iter().map(|l| l.as_ref())
            .chain(state.iter().map(|&(a, ch)| self.choices[a][ch].as_ref())).collect();
        let spans: Vec<(Half, i64, i64)> = layouts.iter().flat_map(|l| l.spans.iter().copied()).collect();
        let mut cost = 0.0f64;
        for i in 0..spans.len() {
            let (h1, a1, b1) = spans[i];
            for &(h2, a2, b2) in &spans[i + 1..] {
                if h1 == h2 {
                    let gap = a1.max(a2) - b1.min(b2) - 1;
                    if gap < 1 {
                        return f64::INFINITY;
                    }
                    cost += SPACE_COST * 0.max(3 - gap) as f64;
                }
            }
        }
        for (li, lay) in layouts.iter().enumerate() {
            for &(h1, a1, b1) in &lay.keep {
                cost += KEEP_COST * (b1 - a1 + 1) as f64;
                for (lj, lay2) in layouts.iter().enumerate() {
                    if lj != li && lay2.spans.iter().any(|&(h2, a2, b2)| h1 == h2 && a1 <= b2 && a2 <= b1) {
                        return f64::INFINITY;
                    }
                }
            }
        }
        let mut where_: IndexMap<MPin, (i64, i64)> = IndexMap::default();
        for lay in &layouts {
            for &(p, xy) in &lay.xy {
                where_.insert(p, xy);
            }
        }
        for (p, &(_, y)) in &where_ {
            if self.net_of.contains_key(p) && ROW_YS.contains(&y) {
                let room = if y < 8 { y - 3 } else { 14 - y };
                cost += ROOM_COST * 0.max(4 - room) as f64;
            }
        }
        let mut crowd: HashMap<(Half, i64), i64> = HashMap::default();
        for (name, pins) in &self.nets {
            let pts: Vec<(i64, i64)> = pins.iter().filter_map(|p| where_.get(p).copied()).collect();
            if pts.is_empty() {
                continue;
            }
            if let Some(rails) = self.to_rail.get(name).filter(|r| !r.is_empty()) {
                for &(x, y) in &pts {
                    let m = rails.iter().map(|r| (y - self.rail_y[&Row::parse(r).unwrap()]).abs()).min().unwrap();
                    cost += RAIL_COST * m as f64;
                    cost += GAP_COST * (self.gaps.contains(&x) as i64) as f64;
                }
            }
            if !self.plan.contains_key(name) {
                let (xmin, xmax) = (pts.iter().map(|p| p.0).min().unwrap(), pts.iter().map(|p| p.0).max().unwrap());
                let (ymin, ymax) = (pts.iter().map(|p| p.1).min().unwrap(), pts.iter().map(|p| p.1).max().unwrap());
                cost += ((xmax - xmin) + (ymax - ymin)) as f64;
                for half in [Half::Top, Half::Bot] {
                    let hx: Vec<i64> = pts.iter().filter(|(_, y)| (*y < 8) == (half == Half::Top)).map(|p| p.0).collect();
                    if hx.len() > 1 {
                        for x in *hx.iter().min().unwrap()..=*hx.iter().max().unwrap() {
                            *crowd.entry((half, x)).or_insert(0) += 1;
                        }
                    }
                }
            }
            if self.offboard.contains(name) {
                cost += OFFBOARD_COST * (self.width - pts.iter().map(|p| p.0).max().unwrap()) as f64;
            }
            if !self.plan.contains_key(name) && pts.len() > 1 {
                let (lo, hi) = (pts.iter().map(|p| p.0).min().unwrap(), pts.iter().map(|p| p.0).max().unwrap());
                for lay in &layouts {
                    let walls = lay.walls.iter().filter(|w| lo < w.1 && w.2 < hi).count();
                    if walls > 0 && !lay.xy.iter().any(|(p, _)| self.net_of.get(p) == Some(name)) {
                        cost += if walls > 1 { WALL_COST } else { HALF_WALL_COST };
                    }
                }
            }
        }
        let mut taken: HashMap<(Half, i64), i64> = HashMap::default();
        for lay in &layouts {
            for (k, n) in &lay.taken {
                *taken.entry(*k).or_insert(0) += n;
            }
        }
        let crowded: i64 = crowd.iter().map(|(k, n)| 0.max(n - 1.max(4 - taken.get(k).copied().unwrap_or(0)))).sum();
        cost += CROWD_COST * crowded as f64;
        for (a, b) in &self.links {
            let pa: Vec<(i64, i64)> = self.nets[a].iter().filter_map(|p| where_.get(p).copied()).collect();
            let pb: Vec<(i64, i64)> = self.nets[b].iter().filter_map(|p| where_.get(p).copied()).collect();
            if !pa.is_empty() && !pb.is_empty() && !self.plan.contains_key(a) && !self.plan.contains_key(b) {
                let m = pa.iter().flat_map(|&(x1, y1)| pb.iter().map(move |&(x2, y2)| (x1 - x2).abs() + (y1 - y2).abs())).min().unwrap();
                cost += LEG_LINK_COST * m as f64;
            }
            for (x, y) in [(a, b), (b, a)] {
                let pins: Vec<(i64, i64)> = self.nets[x].iter().filter_map(|p| where_.get(p).copied()).collect();
                if self.plan.contains_key(y) && !self.plan.contains_key(x) && !pins.is_empty() {
                    let m = pins.iter().flat_map(|&(_, py)| self.plan[y].iter().map(move |r| (py, r)))
                        .map(|(py, r)| (py - self.rail_y[&Row::parse(r).unwrap()]).abs()).min().unwrap();
                    cost += LEG_LINK_COST * m as f64;
                }
            }
        }
        let mut claimed: HashSet<(Half, i64)> = HashSet::default();
        for (a, b) in &self.links {
            if self.plan.contains_key(a) || self.plan.contains_key(b) {
                continue;
            }
            let mut xs: Vec<i64> = self.nets[a].iter().chain(self.nets[b].iter()).filter_map(|p| where_.get(p)).map(|p| p.0).collect();
            if xs.is_empty() {
                continue;
            }
            xs.sort();
            xs.dedup();
            let mut spot = None;
            'search: for d in 0..=ROOM_REACH {
                for &x in &xs {
                    let ks = set2_order(x - d, x + d);
                    let ks: &[i64] = if d == 0 { &ks[..1] } else { &ks };
                    for &k in ks {
                        for key in [(Half::Top, k), (Half::Bot, k)] {
                            if self.strips.contains(&key) && !claimed.contains(&key) && taken.get(&key).copied().unwrap_or(0) == 0 {
                                spot = Some(key);
                                break 'search;
                            }
                        }
                    }
                }
            }
            match spot {
                None => cost += ROOMLESS_COST,
                Some(s) => {
                    claimed.insert(s);
                }
            }
        }
        cost
    }

    /// Annealing from one seed: the best few placements it came across, cheapest first.
    pub fn anneal(&self, seed: i64) -> Vec<(f64, Vec<(usize, usize)>)> {
        let mut rng = Random::new(seed);
        let n = self.auto.len();
        let mut state: Vec<(usize, usize)> = Vec::new();
        for a in 0..n {
            let mut order: Vec<usize> = (0..self.choices[a].len()).collect();
            rng.shuffle(&mut order);
            let mut pick = order[0];
            for &ch in &order {
                let mut trial = state.clone();
                trial.push((a, ch));
                if self.cost(&trial) < f64::INFINITY {
                    pick = ch;
                    break;
                }
            }
            state.push((a, pick));
        }
        if self.cost(&state) == f64::INFINITY {
            state.clear();
            let width = |a: usize| self.choices[a][0].spans.iter().map(|s| s.2 - s.1).max().unwrap();
            let mut refs: Vec<usize> = (0..n).collect();
            // sorted(..., reverse=True): stable, widest first
            refs.sort_by(|&x, &y| width(y).cmp(&width(x)));
            for a in refs {
                let sign = if seed % 2 == 0 { 1 } else { -1 };
                let mut order: Vec<usize> = (0..self.choices[a].len()).collect();
                order.sort_by_key(|&ch| self.choices[a][ch].spans.iter().map(|s| s.1).min().unwrap() * sign);
                let mut pick = order[0];
                for &ch in &order {
                    let mut trial = state.clone();
                    trial.push((a, ch));
                    if self.cost(&trial) < f64::INFINITY {
                        pick = ch;
                        break;
                    }
                }
                state.push((a, pick));
            }
            state.sort_by_key(|s| s.0);
        }
        let mut cost = self.cost(&state);
        let mut seen: IndexMap<Vec<(String, String, i32)>, (f64, Vec<(usize, usize)>)> = IndexMap::default();
        if cost < f64::INFINITY {
            seen.insert(self.frozen(&state), (cost, state.clone()));
        }
        let all: Vec<usize> = (0..n).collect();
        for step in 0..STEPS {
            let temp = 30.0 * (1.0 - step as f64 / STEPS as f64) + 0.05;
            let a = rng.choice(&all);
            let mut new = state.clone();
            if rng.random() < 0.7 {
                let at_x = self.choices[a][state[a].1].x;
                let near: Vec<usize> = (0..self.choices[a].len()).filter(|&ch| (self.choices[a][ch].x - at_x).abs() <= 6).collect();
                let pool: Vec<usize> = if near.is_empty() { (0..self.choices[a].len()).collect() } else { near };
                new[a].1 = rng.choice(&pool);
            } else if n > 1 && rng.random() < 0.5 {
                let others: Vec<usize> = (0..n).filter(|&o| o != a).collect();
                let o = rng.choice(&others);
                let xo = self.choices[o][state[o].1].x;
                let xa = self.choices[a][state[a].1].x;
                new[a].1 = (0..self.choices[a].len()).min_by_key(|&ch| (self.choices[a][ch].x - xo).abs()).unwrap();
                new[o].1 = (0..self.choices[o].len()).min_by_key(|&ch| (self.choices[o][ch].x - xa).abs()).unwrap();
            } else {
                let all_ch: Vec<usize> = (0..self.choices[a].len()).collect();
                new[a].1 = rng.choice(&all_ch);
            }
            let c2 = self.cost(&new);
            if c2 < cost || (c2 < f64::INFINITY && rng.random() < ((cost - c2) / temp).exp()) {
                state = new;
                cost = c2;
                let f = self.frozen(&state);
                match seen.get_mut(&f) {
                    Some(v) => *v = (cost, state.clone()),
                    None => {
                        seen.insert(f, (cost, state.clone()));
                    }
                }
            }
        }
        let mut found: Vec<(f64, Vec<(usize, usize)>)> = seen.into_values().collect();
        found.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        found.truncate(KEEP);
        found
    }

    pub fn frozen(&self, state: &[(usize, usize)]) -> Vec<(String, String, i32)> {
        let mut f: Vec<(String, String, i32)> = state.iter().map(|&(a, ch)| {
            let l = &self.choices[a][ch];
            (self.auto[a].clone(), l.at.clone(), l.rot)
        }).collect();
        f.sort();
        f
    }
}

fn frozen_json(f: &[(String, String, i32)]) -> Value {
    Value::Array(f.iter().map(|(r, at, rot)| json!([r, at, rot])).collect())
}

// ---- the search on built layouts ----

/// The file with some parts' fields changed (a field set to None is removed): autoplace.py's _with.
fn with(data: &Val, changes: &[(String, Vec<(&str, Val)>)]) -> Val {
    let mut out = data.clone();
    let parts = out.map_get_mut("parts").unwrap();
    for (r, fields) in changes {
        let spec = parts.map_get_mut(r).unwrap();
        for (k, v) in fields {
            spec.map_set(k, v.clone());
        }
        spec.map_retain(|_, v| !v.is_null());
    }
    out
}

fn set_key(data: &Val, key: &str, v: Val) -> Val {
    let mut out = data.clone();
    out.map_set(key, v);
    out
}

fn leg_rank_val(ranks: &[(String, i64)]) -> Val {
    Val::Map(ranks.iter().map(|(r, k)| (Val::Str(r.clone()), Val::Int(*k))).collect())
}

/// The trial's layout as dump.py records it.
fn layout_json(trial: &Val) -> Value {
    let mut parts = serde_json::Map::new();
    for r in trial.at("_placed").as_list().cloned().unwrap_or_default() {
        let r = r.py_str();
        let spec = trial.at("parts").at(&r);
        let v = if spec.get("at").is_some() {
            json!([spec.at("at").to_json(), spec.get("rot").map(Val::to_json).unwrap_or(Value::Null)])
        } else {
            spec.get("pins").map(Val::to_json).unwrap_or(Value::Null)
        };
        parts.insert(r, v);
    }
    let rank = if trial.at("_leg_rank").truthy() { trial.at("_leg_rank").to_json() } else { json!({}) };
    let order = if trial.at("_rail_order").truthy() { trial.at("_rail_order").to_json() } else { json!([]) };
    json!({"parts": parts, "leg_rank": rank, "rail_order": order})
}

pub struct Built {
    pub score: f64,
    pub circuit: Circuit,
}

/// Build a trial and score it: autoplace.py's _evaluate.
fn evaluate(trial: &Val, source: &str) -> Built {
    let c = Circuit::new(trial, source).expect("a trial layout that doesn't build");
    let r = crate::check::check(&c);
    let score = crate::autoplace::score(&c, &r);
    Built { score, circuit: c }
}

/// Build trials in order, each scored (one at a time; the same results whichever way they're built).
struct Scorer<'a> {
    source: &'a str,
}

/// How many trials to build side by side: BBGEN_JOBS, or one per core.
pub fn jobs() -> usize {
    std::env::var("BBGEN_JOBS").ok().and_then(|v| v.parse().ok()).filter(|&n: &usize| n >= 1)
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
}

impl<'a> Scorer<'a> {
    /// Build the trials in order, handing each (its index, the file, its build) to visit until visit says stop.
    /// The next few are built side by side; visit still sees them one at a time and in order, so the search
    /// stops at the same trial and ends the same as built one at a time. (A record is made one at a time, as
    /// the events come from the builds.)
    fn scan(&self, trials: Vec<Val>, mut visit: impl FnMut(usize, Val, Built) -> bool) {
        use rayon::prelude::*;
        let jobs = jobs();
        if jobs <= 1 || dump::active() || (crate::trace::active() && !crate::trace::printing()) {
            for (i, t) in trials.into_iter().enumerate() {
                let b = self.score(&t);
                if visit(i, t, b) {
                    return;
                }
            }
            return;
        }
        let mut it = trials.into_iter().enumerate();
        loop {
            let batch: Vec<(usize, Val)> = it.by_ref().take(jobs).collect();
            if batch.is_empty() {
                return;
            }
            let built: Vec<Built> = batch.par_iter().map(|(_, t)| evaluate(t, self.source)).collect();
            for ((i, t), b) in batch.into_iter().zip(built) {
                if visit(i, t, b) {
                    return;
                }
            }
        }
    }

    fn score(&self, trial: &Val) -> Built {
        let b = evaluate(trial, self.source);
        if dump::active() {
            dump::event("trial", vec![("layout", layout_json(trial)), ("score", dump::num(b.score))]);
        }
        b
    }
}

fn pinned(c: &Circuit, leads: &[String]) -> Vec<(String, Vec<(&'static str, Val)>)> {
    let mut out = Vec::new();
    for r in leads {
        let pi = c.part_index[r] as usize;
        let part = &c.parts[pi];
        if part.pins.iter().all(|p| p.hole.is_some()) {
            let holes = part.pins.iter().map(|p| Val::Str(c.hole_name(p.hole.unwrap()))).collect();
            out.push((r.clone(), vec![("pins", Val::List(holes))]));
        }
    }
    out
}

/// Parts named in the warnings, or owning a net a warned wire or a flexible jumper carries, most mentioned first.
fn trouble(c: &Circuit, result: &crate::check::Result) -> Vec<String> {
    let mut count: IndexMap<String, i64> = IndexMap::default();
    let mut add = |r: &str, n: i64| *count.entry(r.to_string()).or_insert(0) += n;
    let net_of = crate::autoplace::net_of(c);
    let parts_on = |group: &str| -> Vec<String> {
        c.parts.iter().enumerate().filter(|(pi, part)| (0..part.pins.len())
            .any(|k| net_of.get(&crate::circuit::PinId { part: *pi as u16, pin: k as u16 }).map(|n| n == group).unwrap_or(false)))
            .map(|(_, p)| p.r#ref.clone()).collect()
    };
    for issue in result.issues.iter().filter(|i| i.level == "warning") {
        for word in words(&issue.message) {
            if c.part_index.contains_key(&word) {
                add(&word, 2);
            } else if word.len() > 1 && word.starts_with('W') && word[1..].chars().all(|ch| ch.is_ascii_digit()) {
                let k: usize = word[1..].parse().unwrap_or(usize::MAX);
                if k >= 1 && k <= c.wires.len() {
                    let group = c.wires[k - 1].group.clone();
                    for r in parts_on(&group) {
                        add(&r, 1);
                    }
                }
            }
        }
    }
    for w in &c.wires {
        if w.flex {
            for r in parts_on(&w.group) {
                add(&r, 1);
            }
        }
    }
    let mut items: Vec<(String, i64)> = count.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1)); // most_common: stable, highest first
    items.into_iter().map(|(r, _)| r).collect()
}

/// re.findall(r"[A-Za-z_][\w-]*", text)
fn words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '-') {
                j += 1;
            }
            out.push(chars[i..j].iter().collect());
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn choice_of(model: &Model, a: usize, data: &Val) -> usize {
    let spec = data.at("parts").at(&model.auto[a]);
    let (at, rot) = (spec.at("at").py_str(), spec.at("rot").py_int().unwrap_or(0) as i32);
    (0..model.choices[a].len()).find(|&ch| model.choices[a][ch].at == at && model.choices[a][ch].rot == rot).unwrap()
}

/// The layout with a footprint part turned round where it is (the same holes), or None.
fn flip(model: &Model, data: &Val, a: usize) -> Option<usize> {
    let now = choice_of(model, a, data);
    let n = &model.choices[a][now];
    (0..model.choices[a].len()).find(|&ch| ch != now && model.choices[a][ch].rot != n.rot && model.choices[a][ch].spans == n.spans)
}

fn at_rot(model: &Model, a: usize, ch: usize) -> Vec<(&'static str, Val)> {
    let l = &model.choices[a][ch];
    vec![("at", Val::Str(l.at.clone())), ("rot", Val::Int(l.rot as i64))]
}

type Changes = Vec<(String, Vec<(&'static str, Val)>)>;

/// Changes to try, the parts in trouble first.
fn moves(model: &Model, leads: &[String], c: &Circuit, result: &crate::check::Result, data: &Val) -> Vec<(Changes, Vec<(String, i64)>)> {
    let order = trouble(c, result);
    let rank = |r: &str| order.iter().position(|x| x == r).unwrap_or(order.len());
    let pinned = pinned(c, leads);
    let unpinned: Changes = leads.iter().map(|r| (r.clone(), vec![("pins", Val::Null)])).collect();
    let state: Vec<(usize, usize)> = (0..model.auto.len()).map(|a| (a, choice_of(model, a, data))).collect();
    let mut chips: Vec<usize> = (0..model.auto.len()).collect();
    chips.sort_by_key(|&a| rank(&model.auto[a]));
    let mut out = Vec::new();
    let merge = |base: &Changes, more: Changes| -> Changes {
        let mut m = base.clone();
        for (r, f) in more {
            match m.iter_mut().find(|(x, _)| *x == r) {
                Some(slot) => slot.1 = f,
                None => m.push((r, f)),
            }
        }
        m
    };
    let mut flipped: HashMap<usize, usize> = HashMap::default();
    for &a in &chips {
        let now = &model.choices[a][state[a].1];
        let flips: Vec<usize> = (0..model.choices[a].len()).filter(|&ch| ch != state[a].1 && model.choices[a][ch].rot != now.rot
            && model.choices[a][ch].spans == now.spans).collect();
        if let Some(&f) = flips.first() {
            flipped.insert(a, f);
            out.push((merge(&unpinned, vec![(model.auto[a].clone(), at_rot(model, a, f))]), vec![]));
        }
    }
    for change in shifted(model, &state) {
        out.push((merge(&unpinned, change), vec![]));
    }
    let mut by_rank: Vec<&String> = leads.iter().collect();
    by_rank.sort_by_key(|r| rank(r));
    for r in by_rank {
        for k in 0..4 {
            let mut ch: Changes = pinned.iter().filter(|(x, _)| x != r).cloned().collect();
            ch.push((r.clone(), vec![("pins", Val::Null)]));
            out.push((ch, vec![(r.clone(), k)]));
        }
    }
    for &a in &chips {
        let spec = data.at("parts").at(&model.auto[a]);
        let at = spec.at("at").py_str();
        let (_, row, _) = parse_hole(&at).unwrap();
        let col = model.x_of(&at);
        let rot = spec.at("rot").py_int().unwrap_or(0) as i32;
        let layouts: Vec<usize> = (0..model.choices[a].len()).filter(|&ch| ch != state[a].1 && Some(&ch) != flipped.get(&a)).collect();
        let near = |ch: usize| {
            let l = &model.choices[a][ch];
            (l.x - col).abs() + (parse_hole(&l.at).unwrap().1 != row) as i64 * 2 + (l.rot != rot) as i64 * 3
        };
        let cost_with = |ch: usize| {
            let mut s = state.clone();
            s[a].1 = ch;
            model.cost(&s)
        };
        let costs: HashMap<usize, f64> = layouts.iter().map(|&ch| (ch, cost_with(ch))).collect();
        let mut scored = layouts.clone();
        scored.sort_by(|x, y| costs[x].partial_cmp(&costs[y]).unwrap());
        let mut tries: Vec<usize> = layouts.clone();
        tries.sort_by_key(|&ch| near(ch));
        tries.truncate(4);
        tries.extend(scored.iter().take(6).copied().filter(|ch| costs[ch] < f64::INFINITY));
        let mut seen = Vec::new();
        for ch in tries {
            if !seen.contains(&ch) {
                seen.push(ch);
                out.push((merge(&unpinned, vec![(model.auto[a].clone(), at_rot(model, a, ch))]), vec![]));
            }
        }
    }
    out
}

/// What a repair move does, in words, and the parts it moves.
fn describe(model: &Model, data: &Val, changes: &Changes, ranks: &[(String, i64)]) -> (String, Vec<String>) {
    if let Some((r, k)) = ranks.first() {
        let nth = ["best", "2nd best", "3rd best", "4th best"][*k as usize];
        return (format!("{} into its {} legs", r, nth), vec![r.clone()]);
    }
    let has_at = |fields: &Vec<(&str, Val)>| fields.iter().any(|(k, _)| *k == "at");
    let moved: Vec<String> = changes.iter().filter(|(_, f)| has_at(f)).map(|(r, _)| r.clone()).collect();
    if moved.len() > 1 {
        return (format!("move {} a row", moved.join(" and ")), moved);
    }
    for (r, fields) in changes {
        if has_at(fields) {
            let get = |k: &str| fields.iter().find(|(x, _)| *x == k).map(|(_, v)| v.clone()).unwrap();
            let (at, rot) = (get("at").py_str(), get("rot").py_int().unwrap_or(0) as i32);
            let a = model.auto.iter().position(|x| x == r).unwrap();
            if let Some(f) = flip(model, data, a) {
                if model.choices[a][f].at == at && model.choices[a][f].rot == rot {
                    return (format!("turn {}", r), vec![r.clone()]);
                }
            }
            return (format!("move {} from {} to {}", r, data.at("parts").at(r).at("at").py_str(), at), vec![r.clone()]);
        }
    }
    ("re-place the two-lead parts".into(), vec![])
}

/// The layout with every footprint part bbgen places moved a row up, or a row down, where they all can go.
fn shifted(model: &Model, state: &[(usize, usize)]) -> Vec<Changes> {
    let mut out = Vec::new();
    for dr in [1i32, -1] {
        let mut change: Vec<(usize, usize)> = Vec::new();
        let mut ok = true;
        for &(a, ch) in state {
            let now = &model.choices[a][ch];
            let (b, row, col) = parse_hole(&now.at).unwrap();
            let r = (row.name().as_bytes()[0] as i32 + dr) as u8 as char;
            let at = format!("{}{}{}", b.map(|b| format!("{}:", b)).unwrap_or_default(), r, col.unwrap());
            match (0..model.choices[a].len()).find(|&c| model.choices[a][c].at == at && model.choices[a][c].rot == now.rot) {
                Some(c) => change.push((a, c)),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && !change.is_empty() {
            let mut s = state.to_vec();
            for &(a, c) in &change {
                s[a].1 = c;
            }
            if model.cost(&s) < f64::INFINITY {
                out.push(change.iter().map(|&(a, c)| (model.auto[a].clone(), at_rot(model, a, c))).collect());
            }
        }
    }
    out
}

fn with_ranks(data: &Val, changes: &Changes, ranks: &[(String, i64)]) -> Val {
    set_key(&with(data, changes), "_leg_rank", leg_rank_val(ranks))
}

fn without_rank(mut v: Val) -> Val {
    v.map_remove("_leg_rank");
    v
}

/// A few leg-pair moves on a fresh layout, greedily: (score, data, circuit, builds spent).
fn polish(sc: &Scorer, leads: &[String], data: Val, mut score: f64, mut c: Circuit, budget: usize) -> (f64, Val, Circuit, usize) {
    let mut spent = 0;
    let mut data = with(&data, &pinned(&c, leads));
    let mut improved = true;
    while improved && spent < budget {
        improved = false;
        let order = trouble(&c, &crate::check::check(&c));
        let rank = |r: &str| order.iter().position(|x| x == r).unwrap_or(order.len());
        let mut refs: Vec<&String> = leads.iter().collect();
        refs.sort_by_key(|r| rank(r));
        let here = data.clone();
        let mut trials = Vec::new();
        for r in refs.into_iter().take(4) {
            for k in [1, 2] {
                trials.push(set_key(&with(&here, &vec![(r.clone(), vec![("pins", Val::Null)])]), "_leg_rank", leg_rank_val(&[(r.clone(), k)])));
            }
        }
        if spent >= budget {
            break;
        }
        trials.truncate(budget - spent);
        let mut kept: Option<(f64, Val, Circuit)> = None;
        sc.scan(trials, |_, trial, b| {
            spent += 1;
            if b.score < score - 1e-9 {
                if dump::active() {
                    dump::event("keep", vec![("stage", json!("polish")), ("score", dump::num(b.score))]);
                }
                kept = Some((b.score, trial, b.circuit));
                return true;
            }
            false
        });
        if let Some((s2, trial, c2)) = kept {
            score = s2;
            c = c2;
            data = without_rank(with(&trial, &pinned(&c, leads)));
            improved = true;
        }
    }
    (score, data, c, spent)
}

/// The file with every part bbgen has to place placed.
pub fn autoplace(data: &Val, source: &str) -> Result<Val, String> {
    let model = Model::new(data)?;
    let leads: Vec<String> = model.parts.iter().filter(|(_, p)| p.kind.is_lead() && p.legs.is_empty()).map(|(r, _)| r.clone()).collect();
    let mut base = data.clone();
    if base.get("autowire").is_none() {
        base.map_set("autowire", Val::Bool(true));
    }
    base.map_set("_placed", Val::List(model.auto.iter().chain(leads.iter()).map(|r| Val::Str(r.clone())).collect()));
    let mut orders = vec![model.free_supplies.clone()];
    if model.free_supplies.len() > 1 {
        orders.push(model.free_supplies.iter().rev().cloned().collect());
    }
    let mut bases: Vec<Val> = Vec::new();
    let mut models: Vec<Model> = Vec::new();
    for (k, order) in orders.iter().enumerate() {
        let b = if orders.len() > 1 { set_key(&base, "_rail_order", Val::List(order.iter().map(|r| Val::Str(r.clone())).collect())) } else { base.clone() };
        if k > 0 {
            models.push(Model::new(&b)?);
        }
        bases.push(b);
    }
    let mut all_models: Vec<&Model> = vec![&model];
    all_models.extend(models.iter());
    let mut candidates: IndexMap<(usize, Vec<(String, String, i32)>), (f64, Vec<(usize, usize)>, usize)> = IndexMap::default();
    for (k, m) in all_models.iter().enumerate() {
        if dump::active() {
            let choices: serde_json::Map<String, Value> = m.auto.iter().enumerate()
                .map(|(a, r)| (r.clone(), Value::Array(m.choices[a].iter().map(|l| json!([l.at, l.rot])).collect()))).collect();
            dump::event("model", vec![("model", json!(k)), ("supplies", json!(orders[k])), ("choices", Value::Object(choices))]);
        }
        if !m.auto.is_empty() {
            for seed in 0..SEEDS {
                let found = m.anneal(seed);
                if dump::active() {
                    let f: Vec<Value> = found.iter().map(|(c, s)| json!([dump::num(*c), frozen_json(&m.frozen(s))])).collect();
                    dump::event("anneal", vec![("model", json!(k)), ("seed", json!(seed)), ("found", Value::Array(f))]);
                }
                for (cost, state) in found {
                    candidates.entry((k, m.frozen(&state))).or_insert((cost, state, k));
                }
            }
        } else {
            candidates.insert((k, vec![]), (0.0, vec![], k));
        }
    }
    let mut starts: Vec<(f64, Vec<(usize, usize)>, usize)> = candidates.into_values().collect();
    starts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    starts.truncate(KEEP * 2);
    let sc = Scorer { source };
    let mut best: Option<(f64, Val, Circuit, usize)> = None;
    let trials: Vec<Val> = starts.iter().map(|(_, state, k)| {
        let m = all_models[*k];
        let changes: Changes = state.iter().map(|&(a, ch)| (m.auto[a].clone(), at_rot(m, a, ch))).collect();
        with(&bases[*k], &changes)
    }).collect();
    let n_starts = starts.len();
    sc.scan(trials, |i, trial, b| {
        let kept = best.as_ref().map(|x| b.score < x.0).unwrap_or(true);
        if crate::trace::active() {
            crate::trace::scored(&b.circuit, &format!("starting layout {} of {}: score {:.0}{}", i + 1, n_starts, b.score,
                                                      if kept { " ✓ best so far" } else { "" }),
                                 crate::trace::Scored { phase: "start", what: format!("layout {} of {}", i + 1, n_starts),
                                                        from: best.as_ref().map(|x| x.0), score: b.score }, kept, &[]);
        }
        if kept {
            if dump::active() {
                dump::event("keep", vec![("stage", json!("start")), ("score", dump::num(b.score))]);
            }
            best = Some((b.score, trial, b.circuit, starts[i].2));
        }
        false
    });
    let (score, data, c, k) = best.ok_or("bbgen found no way to fit these parts on the board without overlapping; place the \
                                          biggest yourself (at:), or use more boards")?;
    let model = all_models[k];
    let (score, data, c) = turned(&sc, model, score, data, c);
    Ok(repair(&sc, model, &leads, score, data, c))
}

/// The best layout with its footprint parts turned round where they are, in every combination.
fn turned(sc: &Scorer, model: &Model, mut score: f64, mut data: Val, mut c: Circuit) -> (f64, Val, Circuit) {
    let mut flips: Vec<(String, usize, usize)> = Vec::new();
    for a in 0..model.auto.len() {
        if let Some(f) = flip(model, &data, a) {
            flips.push((model.auto[a].clone(), a, f));
        }
    }
    flips.sort_by(|x, y| x.0.cmp(&y.0));
    let n = flips.len();
    let mut combos: Vec<Vec<usize>> = Vec::new();
    let sizes: Vec<usize> = if n <= TURN_ALL { (1..=n).collect() } else { vec![1, 2] };
    for size in sizes {
        combinations(n, size, &mut combos);
    }
    let mut start = 0;
    while start < combos.len() {
        let here = data.clone();
        let from = start;
        start = combos.len();
        let trials: Vec<Val> = (from..combos.len()).map(|j| {
            let changes: Changes = combos[j].iter().map(|&i| (flips[i].0.clone(), at_rot(model, flips[i].1, flips[i].2))).collect();
            with(&here, &changes)
        }).collect();
        let mut kept: Option<(usize, f64, Val, Circuit)> = None;
        sc.scan(trials, |i, trial, b| {
            let better = b.score < score - 1e-9;
            if crate::trace::active() {
                let refs: Vec<String> = combos[from + i].iter().map(|&k| flips[k].0.clone()).collect();
                crate::trace::scored(&b.circuit, &format!("turn {}: score {:.0} → {:.0} {}", refs.join(" and "), score, b.score,
                                                          if better { "✓" } else { "✗" }),
                                     crate::trace::Scored { phase: "turn", what: format!("turn {}", refs.join(" and ")),
                                                            from: Some(score), score: b.score }, better, &refs);
            }
            if better {
                if dump::active() {
                    dump::event("keep", vec![("stage", json!("turn")), ("score", dump::num(b.score))]);
                }
                kept = Some((from + i, b.score, trial, b.circuit));
                return true;
            }
            false
        });
        if let Some((j, s2, trial, c2)) = kept {
            score = s2;
            data = trial;
            c = c2;
            start = j + 1;
        }
    }
    (score, data, c)
}

/// itertools.combinations(range(n), k), in its order.
fn combinations(n: usize, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == 0 || k > n {
        return;
    }
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.clone());
        // the rightmost index that can still move on
        match (0..k).rev().find(|&i| idx[i] != i + n - k) {
            None => return,
            Some(i) => {
                idx[i] += 1;
                for j in i + 1..k {
                    idx[j] = idx[j - 1] + 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn combinations_in_itertools_order() {
        let mut out = Vec::new();
        super::combinations(4, 2, &mut out);
        assert_eq!(out, vec![vec![0, 1], vec![0, 2], vec![0, 3], vec![1, 2], vec![1, 3], vec![2, 3]]);
        let mut out = Vec::new();
        super::combinations(3, 3, &mut out);
        assert_eq!(out, vec![vec![0, 1, 2]]);
    }
}

fn repair(sc: &Scorer, model: &Model, leads: &[String], mut score: f64, mut data: Val, mut c: Circuit) -> Val {
    let (mut builds, mut since, mut polishes) = (0usize, 0usize, 0usize);
    let mut improved = true;
    while improved && builds < REPAIR_BUILDS && since < PATIENCE {
        improved = false;
        let here = data.clone();
        let result = crate::check::check(&c);
        let ms = moves(model, leads, &c, &result, &data);
        let ranked: Vec<bool> = ms.iter().map(|(_, ranks)| !ranks.is_empty()).collect();
        let trials: Vec<Val> = ms.iter().map(|(changes, ranks)| with_ranks(&here, changes, ranks)).collect();
        let mut kept: Option<(f64, Val, Circuit)> = None;
        sc.scan(trials, |i, mut trial, b| {
            let (mut s2, mut c2) = (b.score, b.circuit);
            builds += 1;
            since += 1;
            if !ranked[i] && score <= s2 && s2 < score + LOOKAHEAD && polishes < LOOKAHEAD_TIMES {
                let (s3, d3, c3, _) = polish(sc, leads, trial.clone(), s2, c2, LOOKAHEAD_BUILDS);
                polishes += 1;
                if s3 < s2 {
                    s2 = s3;
                    trial = d3;
                }
                c2 = c3;
            }
            if crate::trace::active() {
                let (what, refs) = describe(model, &data, &ms[i].0, &ms[i].1);
                let kept = s2 < score - 1e-9;
                crate::trace::scored(&c2, &format!("repair {}: {}: score {:.0} → {:.0} {}", builds, what, score, s2,
                                                   if kept { "✓" } else { "✗" }),
                                     crate::trace::Scored { phase: "repair", what, from: Some(score), score: s2 }, kept, &refs);
            }
            if s2 < score - 1e-9 {
                if dump::active() {
                    dump::event("keep", vec![("stage", json!("repair")), ("score", dump::num(s2))]);
                }
                since = 0;
                kept = Some((s2, trial, c2));
                return true;
            }
            builds >= REPAIR_BUILDS || since >= PATIENCE
        });
        if let Some((s2, trial, c2)) = kept {
            score = s2;
            c = c2;
            data = without_rank(with(&trial, &pinned(&c, leads)));
            improved = true;
        }
    }
    if !leads.is_empty() && data.get("_leg_rank").is_none() {
        data = with(&data, &pinned(&c, leads));
    }
    data
}

#[allow(dead_code)]
fn _unused(_: Rect) -> f64 {
    pymax(0.0, 0.0)
}
