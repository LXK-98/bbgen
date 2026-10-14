//! Load a circuit file: boards, parts, wires and nets, with every pin placed (bbgen/circuit.py).
//!
//! Problems with the layout itself (a pin in the trench, two things in one hole, a wire to a pin that doesn't
//! exist) are collected as issues so the drawing still renders. Only a malformed file is an error.

use crate::board::{parse_hole, Board, Hole, Row, Strip, Xy, RAILS, ROWS};
use crate::parts::{self, body_rect, make_part, placed_box, rotate, slant_reason, Kind, Part, Rect};
use crate::pyf;
use crate::val::Val;
use crate::IndexMap;
use crate::{HashMap, HashSet};
use std::path::PathBuf;

pub const EPS: f64 = 1e-6;
const CLEARANCE: f64 = 0.25;
const DETOUR_REACH: i32 = 12;
const CHAR_W: f64 = 0.2;

pub type K = (i64, i64);

const COLORS: [(&str, &str); 14] = [
    ("red", "#d62728"), ("black", "#262626"), ("blue", "#1f5fbf"), ("green", "#2ca02c"), ("yellow", "#f2c80f"),
    ("orange", "#ff7f0e"), ("white", "#f7f7f7"), ("purple", "#8c4fbf"), ("gray", "#8a8a8a"), ("grey", "#8a8a8a"),
    ("brown", "#8c564b"), ("pink", "#e377c2"), ("cyan", "#17becf"), ("", ""),
];
const SUPPLY_COLORS: [&str; 4] = ["red", "orange", "pink", "brown"];
const NET_PALETTE: [&str; 10] = ["blue", "green", "yellow", "orange", "purple", "white", "brown", "cyan", "pink", "gray"];

pub fn color_hex(color: &str) -> String {
    let low = color.to_lowercase();
    COLORS.iter().find(|(n, _)| !n.is_empty() && *n == low).map(|(_, h)| h.to_string()).unwrap_or(color.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PinId {
    pub part: u16,
    pub pin: u16,
}

/// A connection point: a strip of a board, or an off-board pin (`("strip", board, kind, n)`, `("off", ref, num)`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Node {
    Strip(Strip),
    Off(u16, i64),
}

impl Node {
    pub fn strip(&self) -> Option<Strip> {
        match self {
            Node::Strip(s) => Some(*s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub level: &'static str,
    pub code: &'static str,
    pub message: String,
}

impl Issue {
    pub fn new(level: &'static str, code: &'static str, message: String) -> Issue {
        Issue { level, code, message }
    }
}

#[derive(Clone, Debug)]
pub struct End {
    pub spec: String,
    pub hole: Option<Hole>,
    pub pin: Option<PinId>,
}

#[derive(Clone, Debug)]
pub struct Wire {
    /// stays the same when the wire is renumbered, or taken out and put back
    pub id: u64,
    pub num: i64,
    pub group: String,
    pub spec: (String, String),
    pub color: Option<String>,
    pub via: Vec<String>,
    pub slant: Option<String>,
    pub auto: bool,
    pub flex: bool,
    pub a: Option<End>,
    pub b: Option<End>,
    pub via_xy: Vec<Xy>,
}

impl Wire {
    /// Both ends in holes on the board.
    pub fn on_board(&self) -> bool {
        matches!((&self.a, &self.b), (Some(a), Some(b)) if a.hole.is_some() && b.hole.is_some())
    }
}

#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub pins: Vec<PinId>,
    pub color: String,
    pub rails: Vec<String>,
    pub polarity: Option<char>,
    pub guessed: bool,
}

/// A place one end of a wire or leg could go.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    pub hole: Option<Hole>,
    pub xy: Xy,
    pub near: f64,
    pub pin: Option<PinId>,
    pub fixed: bool,
    pub covered: bool,
    pub under: bool,
    pub kept: bool,
}

impl Opt {
    pub fn at(hole: Option<Hole>, xy: Xy) -> Opt {
        Opt { hole, xy, near: 0.0, pin: None, fixed: false, covered: false, under: false, kept: false }
    }
}

pub type Unresolved = String;

/// (polarity, physical rails, guessed) from a net's `rail:`.
pub fn net_polarity(name: &str, spec: &Val) -> Result<(Option<char>, Vec<String>, bool), String> {
    let raw = if spec.as_map().is_some() { spec.at("rail").clone() } else { Val::Null };
    let items: Vec<String> = match &raw {
        Val::Null => vec![],
        Val::Str(s) => vec![s.trim().to_string()],
        Val::List(l) => l.iter().map(|v| v.py_str().trim().to_string()).collect(),
        _ => return Err(format!("nets.{}: rail is +, -, none, a rail's name (top+, bot-) or a list of names", name)),
    };
    if items == ["none"] {
        return Ok((None, vec![], false));
    }
    let mut signs: Vec<char> = items.iter().filter_map(|r| r.chars().last()).filter(|c| *c == '+' || *c == '-').collect();
    signs.sort();
    signs.dedup();
    if signs.len() > 1 {
        return Err(format!("nets.{}: rail gives both + and - rails; a net is a supply or a ground, not both", name));
    }
    let physical: Vec<String> = items.iter().filter(|r| *r != "+" && *r != "-").cloned().collect();
    if let Some(&s) = signs.first() {
        return Ok((Some(s), physical, false));
    }
    if !physical.is_empty() {
        return Ok((None, physical, false));
    }
    let guess = if is_ground(name) { Some('-') } else if is_supply(name) { Some('+') } else { None };
    Ok((guess, vec![], guess.is_some()))
}

fn is_ground(name: &str) -> bool {
    // ^(A|D|P)?(GND|VSS|0V), ignoring case
    let u = name.to_uppercase();
    let g = |s: &str| s.starts_with("GND") || s.starts_with("VSS") || s.starts_with("0V");
    g(&u) || (u.len() > 1 && matches!(&u[..1], "A" | "D" | "P") && g(&u[1..]))
}

fn is_supply(name: &str) -> bool {
    // ^(VSYS|VBUS|VIN|VCC|VDD|VBAT|\+?\d+V\d*|\+?\d+\.\d+V)$, ignoring case
    let u = name.to_uppercase();
    if ["VSYS", "VBUS", "VIN", "VCC", "VDD", "VBAT"].contains(&u.as_str()) {
        return true;
    }
    let s = u.strip_prefix('+').unwrap_or(&u);
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let rest = &s[digits..];
    if let Some(after) = rest.strip_prefix('V') {
        if after.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if let Some(after) = rest.strip_prefix('.') {
        let d2 = after.chars().take_while(|c| c.is_ascii_digit()).count();
        return d2 > 0 && &after[d2..] == "V";
    }
    false
}

/// The holes strictly between two points of a straight run, as keys.
pub fn aligned(a: Xy, b: Xy) -> bool {
    (a.0 - b.0).abs() < EPS || (a.1 - b.1).abs() < EPS
}

/// (line, lo, hi) for a straight run: the row or column it lies on and the span along it; None if slanted.
pub fn line(a: Xy, b: Xy) -> Option<((u8, i64), f64, f64)> {
    if (a.1 - b.1).abs() < EPS {
        return Some(((b'h', pyf::round_scaled(a.1, 3)), pymin(a.0, b.0), pymax(a.0, b.0)));
    }
    if (a.0 - b.0).abs() < EPS {
        return Some(((b'v', pyf::round_scaled(a.0, 3)), pymin(a.1, b.1), pymax(a.1, b.1)));
    }
    None
}

/// Python's min(a, b): a unless b is smaller.
#[inline]
pub fn pymin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

#[inline]
pub fn pymax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Whether segment a-b passes through the inside of box.
pub fn crosses(a: Xy, b: Xy, bx: Rect) -> bool {
    let (x0, y0, x1, y1) = (bx.0 + 0.01, bx.1 + 0.01, bx.2 - 0.01, bx.3 - 0.01);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    for (p, q) in [(-dx, a.0 - x0), (dx, x1 - a.0), (-dy, a.1 - y0), (dy, y1 - a.1)] {
        if p.abs() < EPS {
            if q < 0.0 {
                return false;
            }
        } else if p < 0.0 {
            t0 = pymax(t0, q / p);
        } else {
            t1 = pymin(t1, q / p);
        }
    }
    t1 - t0 > EPS
}

/// Distance from p to a hint whose x or y may be None (meaning: any).
pub fn hint_dist(p: Xy, hint: Option<(Option<f64>, Option<f64>)>) -> f64 {
    match hint {
        None => 0.0,
        Some((hx, hy)) => {
            let dx = hx.map(|h| p.0 - h).unwrap_or(0.0);
            let dy = hy.map(|h| p.1 - h).unwrap_or(0.0);
            pyf::hypot(dx, dy)
        }
    }
}

pub type Hint = Option<(Option<f64>, Option<f64>)>;

pub struct Circuit {
    pub source: String,
    pub title: String,
    pub flex: bool,
    pub issues: Vec<Issue>,
    pub occupied: HashMap<Hole, String>,
    pub used: HashMap<K, String>,
    pub segments: Vec<(Xy, Xy, String)>,
    pub lines: HashMap<(u8, i64), Vec<(f64, f64, String)>>,
    pub bodies: Vec<(Rect, String)>,
    pub pin_holes: HashSet<Hole>,
    pub covered: HashMap<K, String>,
    pub keepouts: HashSet<String>,
    pub raised: Vec<(Rect, String)>,
    pub under: HashMap<K, String>,
    pub tall: HashSet<String>,
    pub cables: Vec<(Rect, u8)>,
    pub rail_order: Vec<String>,
    pub boards: Vec<Board>,
    pub default_board: u8,
    pub levels: Vec<f64>,
    pub library: Option<Vec<PathBuf>>,
    pub parts: Vec<Part>,
    pub part_index: HashMap<String, u16>,
    pub wires: Vec<Wire>,
    pub groups: Vec<String>,
    pub nets: IndexMap<String, Net>,
    pub lead_room: HashSet<Hole>,
    pub autowire: bool,
    // wiring's state (autowire.rs)
    pub ends: HashSet<K>,
    pub lain: HashSet<K>,
    pub pin_strips: HashSet<Strip>,
    pub strip_at: HashMap<K, Strip>,
    pub wired: u64,
    pub empty: Option<(u64, HashSet<Strip>)>,
    pub step_blocked: HashMap<(u64, u64, u64, u64), bool>,
    pub straight_strips: HashMap<(Strip, Strip), bool>,
    pub next_id: u64,
}

fn bits(p: Xy) -> (u64, u64) {
    (p.0.to_bits(), p.1.to_bits())
}

impl Circuit {
    pub fn load(path: &str) -> Result<Circuit, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path, e))?;
        let data = Val::load(&text)?;
        Circuit::new(&data, path)
    }

    pub fn new(data: &Val, source: &str) -> Result<Circuit, String> {
        if data.as_map().is_none() {
            return Err("the file must be a mapping with title, boards, parts, wires and nets".into());
        }
        if needs_placing(data) {
            return Err("this port can't place parts yet: give it a file as placement left it".into());
        }
        let stem = std::path::Path::new(source).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut c = Circuit {
            source: source.into(),
            title: if data.at("title").truthy() { data.at("title").py_str() } else { stem },
            flex: !matches!(data.get("flex"), Some(Val::Bool(false))),
            issues: vec![], occupied: HashMap::default(), used: HashMap::default(), segments: vec![], lines: HashMap::default(),
            bodies: vec![], pin_holes: HashSet::default(), covered: HashMap::default(), keepouts: HashSet::default(), raised: vec![],
            under: HashMap::default(), tall: HashSet::default(), cables: vec![],
            rail_order: data.at("_rail_order").as_list().map(|l| l.iter().map(Val::py_str).collect()).unwrap_or_default(),
            boards: vec![], default_board: 0, levels: vec![], library: None, parts: vec![], part_index: HashMap::default(),
            wires: vec![], groups: vec![], nets: IndexMap::default(), lead_room: HashSet::default(), autowire: false,
            ends: HashSet::default(), lain: HashSet::default(), pin_strips: HashSet::default(), strip_at: HashMap::default(), wired: 0,
            empty: None, step_blocked: HashMap::default(), straight_strips: HashMap::default(), next_id: 0,
        };
        let boards = if data.at("boards").truthy() { data.at("boards").clone() } else {
            Val::List(vec![Val::Map(vec![(Val::Str("type".into()), Val::Str("full".into()))])])
        };
        c.load_boards(&boards)?;
        c.library = library_for(data);
        let parts = data.at("parts").clone();
        c.load_parts(&parts)?;
        if let Some(placed) = data.at("_placed").as_list() {
            for r in placed {
                if let Some(&i) = c.part_index.get(&r.py_str()) {
                    c.parts[i as usize].auto = true;
                }
            }
        }
        c.load_wires(data.at("wires"))?;
        c.load_nets(data.at("nets"))?;
        c.place();
        if crate::dump::active() {
            let mut pins = serde_json::Map::new();
            for part in &c.parts {
                pins.insert(part.r#ref.clone(), serde_json::Value::Array(part.pins.iter()
                    .map(|p| p.hole.map(|h| serde_json::Value::String(c.hole_name(h))).unwrap_or(serde_json::Value::Null)).collect()));
            }
            crate::dump::event("parts", vec![("pins", serde_json::Value::Object(pins))]);
        }
        if data.at("autowire").truthy() {
            c.autowire = true;
            crate::autowire::autowire(&mut c);
        }
        c.find_slants();
        c.find_over_parts();
        c.find_under();
        c.find_stacking();
        Ok(c)
    }

    // ---- loading ----

    fn load_boards(&mut self, specs: &Val) -> Result<(), String> {
        let mut left = 0.0f64;
        let empty = vec![];
        for (i, spec) in specs.as_list().unwrap_or(&empty).iter().enumerate() {
            let spec = match spec {
                Val::Str(_) => Val::Map(vec![(Val::Str("type".into()), spec.clone())]),
                _ => spec.clone(),
            };
            let id = spec.get("id").map(Val::py_str).unwrap_or((i + 1).to_string());
            let kind = spec.get("type").map(Val::py_str).unwrap_or("full".into());
            let rails = spec.at("rails");
            let top = rails.get("top").map(Val::py_str).unwrap_or("+-".into());
            let bot = rails.get("bottom").or(rails.get("bot")).map(Val::py_str).unwrap_or("+-".into());
            let mut board = Board::new(id, i as u8, &kind, &top, &bot, spec.at("split_rails").truthy())
                .map_err(|e| format!("boards[{}]: {}", i, e))?;
            if self.boards.iter().any(|b| b.id == board.id) {
                return Err(format!("boards[{}]: duplicate id '{}'", i, board.id));
            }
            if let Some(below) = spec.get("below").filter(|v| !v.is_null()) {
                let above = self.boards.iter().find(|b| b.id == below.py_str())
                    .ok_or(format!("boards[{}]: below {} names no board before it", i, below.py_repr()))?;
                if self.boards.iter().any(|b| b.y0 == above.y0 + crate::board::STACK && b.x0 == above.x0) {
                    return Err(format!("boards[{}]: board {} already has a board below it", i, above.id));
                }
                board.x0 = above.x0;
                board.y0 = above.y0 + crate::board::STACK;
            } else {
                board.x0 = left + 2.0;
                left = board.right() + 3.0;
            }
            self.boards.push(board);
        }
        self.default_board = 0;
        let mut levels: Vec<f64> = Vec::new();
        for b in &self.boards {
            for r in ROWS {
                let y = b.y(r);
                if !levels.contains(&y) {
                    levels.push(y);
                }
            }
        }
        levels.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.levels = levels;
        Ok(())
    }

    pub fn board_by_id(&self, id: &str) -> Option<u8> {
        self.boards.iter().position(|b| b.id == id).map(|i| i as u8)
    }

    fn load_parts(&mut self, specs: &Val) -> Result<(), String> {
        let empty = vec![];
        for (r, spec) in specs.as_map().unwrap_or(&empty) {
            let r = r.py_str();
            let ok = r.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
                && r.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
            if !ok {
                return Err(format!("parts.{}: refs are letters, digits, _ and - (and start with a letter)", r));
            }
            let part = make_part(&r, spec, self.library.as_deref())?;
            if let Some(&i) = self.part_index.get(&r) {
                self.parts[i as usize] = part;
            } else {
                self.part_index.insert(r, self.parts.len() as u16);
                self.parts.push(part);
            }
        }
        Ok(())
    }

    fn load_wires(&mut self, wires: &Val) -> Result<(), String> {
        let groups: Vec<(String, Val)> = match wires {
            Val::Map(m) => m.iter().map(|(k, v)| (k.py_str(), v.clone())).collect(),
            Val::Null => vec![("wires".into(), Val::Null)],
            other => vec![("wires".into(), other.clone())],
        };
        for (group, items) in groups {
            self.groups.push(group.clone());
            let empty = vec![];
            for (i, item) in items.as_list().unwrap_or(&empty).iter().enumerate() {
                let where_ = format!("wires.{}[{}]", group, i);
                let (mut slant, mut flex) = (None, false);
                let (a, b, color, via) = match item {
                    Val::List(l) if l.len() == 2 || l.len() == 3 => (l[0].clone(), l[1].clone(),
                        if l.len() == 3 { l[2].clone() } else { Val::Null }, vec![]),
                    Val::Map(_) if item.get("from").is_some() && item.get("to").is_some() => {
                        slant = slant_reason(item, &where_)?;
                        flex = match item.get("flex") {
                            None => false,
                            Some(Val::Bool(f)) => *f,
                            Some(_) => return Err(format!("{}: flex is true or false", where_)),
                        };
                        let via: Vec<Val> = if item.at("via").truthy() { item.at("via").as_list().cloned().unwrap_or_default() } else { vec![] };
                        if flex && !via.is_empty() {
                            return Err(format!("{}: a flexible jumper arches straight over; give it no via", where_));
                        }
                        (item.at("from").clone(), item.at("to").clone(), item.at("color").clone(), via)
                    }
                    _ => return Err(format!("{}: expected [from, to], [from, to, color] or {{from, to, color, via, slant, flex}}", where_)),
                };
                if !color.is_null() {
                    let s = color.py_str();
                    if !COLORS.iter().any(|(n, _)| !n.is_empty() && *n == s.to_lowercase()) && !s.starts_with('#') {
                        return Err(format!("{}: unknown color {}", where_, color.py_repr()));
                    }
                }
                self.next_id += 1;
                self.wires.push(Wire {
                    id: self.next_id, num: self.wires.len() as i64 + 1, group: group.clone(), spec: (a.py_str(), b.py_str()),
                    color: if color.truthy() { Some(color.py_str()) } else { None },
                    via: via.iter().map(Val::py_str).collect(), slant, auto: false, flex, a: None, b: None, via_xy: vec![],
                });
            }
        }
        Ok(())
    }

    fn load_nets(&mut self, specs: &Val) -> Result<(), String> {
        let mut colors: Vec<Option<String>> = Vec::new();
        let empty = vec![];
        for (name, spec) in specs.as_map().unwrap_or(&empty) {
            let name = name.py_str();
            let spec = match spec {
                Val::List(_) => Val::Map(vec![(Val::Str("pins".into()), spec.clone())]),
                _ => spec.clone(),
            };
            let pins_v = match spec.get("pins") {
                Some(Val::List(l)) => l.clone(),
                _ => return Err(format!("nets.{}: expected a list of pins or {{pins: [...], color: ...}}", name)),
            };
            let mut pins: Vec<PinId> = Vec::new();
            for text in &pins_v {
                match self.net_pins(&text.py_str()) {
                    Ok(found) => {
                        for p in found {
                            if !pins.contains(&p) {
                                pins.push(p);
                            }
                        }
                    }
                    Err(e) => self.issues.push(Issue::new("error", "BAD_NET", format!("nets.{}: {}", name, e))),
                }
            }
            let (polarity, rails, guessed) = net_polarity(&name, &spec)?;
            let supplies = self.nets.values().filter(|n| n.polarity == Some('+')).count();
            let color = if spec.at("color").truthy() {
                Some(spec.at("color").py_str())
            } else if polarity == Some('-') {
                Some("black".into())
            } else if polarity == Some('+') {
                Some(SUPPLY_COLORS[supplies % SUPPLY_COLORS.len()].into())
            } else {
                None
            };
            if let Some(i) = self.nets.get_index_of(&name) {
                colors[i] = color;
            } else {
                colors.push(color);
            }
            self.nets.insert(name.clone(), Net { name, pins, color: String::new(), rails, polarity, guessed });
        }
        self.check_rail_count()?;
        let used: HashSet<String> = colors.iter().flatten().map(|c| color_hex(c).to_lowercase()).collect();
        let mut palette: Vec<&str> = NET_PALETTE.iter().copied().filter(|c| !used.contains(&color_hex(c).to_lowercase())).collect();
        if palette.is_empty() {
            palette = NET_PALETTE.to_vec();
        }
        let mut k = 0;
        for (i, net) in self.nets.values_mut().enumerate() {
            net.color = match &colors[i] {
                Some(c) => c.clone(),
                None => {
                    let c = palette[k % palette.len()].to_string();
                    k += 1;
                    c
                }
            };
        }
        Ok(())
    }

    fn check_rail_count(&self) -> Result<(), String> {
        for (sign, what) in [('+', "positive supplies"), ('-', "grounds")] {
            let nets: Vec<&str> = self.nets.values().filter(|n| n.polarity == Some(sign)).map(|n| n.name.as_str()).collect();
            let mut strips: HashSet<(Strip, u8)> = HashSet::default();
            for b in &self.boards {
                for h in b.holes() {
                    if h.row.is_rail() && h.row.sign() == sign {
                        strips.insert((b.strip(h), b.index));
                    }
                }
            }
            if nets.len() > strips.len() {
                let guesses: Vec<&str> = nets.iter().copied().filter(|n| self.nets[*n].guessed).collect();
                let note = if guesses.is_empty() { String::new() } else {
                    format!(" ({} {} taken as {} from the name: say rail: none if not)", guesses.join(", "),
                            if guesses.len() == 1 { "is" } else { "are" }, if sign == '+' { "a supply" } else { "a ground" })
                };
                let many = self.boards.len() > 1;
                return Err(format!("nets: {} {} ({}) but the board{} {} {} {} rail{}: each needs its own. Split the rails \
                                    (split_rails: true) or add a board{}", nets.len(), what, nets.join(", "),
                                   if many { "s" } else { "" }, if many { "have" } else { "has" }, strips.len(), sign,
                                   if strips.len() != 1 { "s" } else { "" }, note));
            }
        }
        Ok(())
    }

    // ---- placement ----

    fn place(&mut self) {
        for i in 0..self.parts.len() {
            if self.parts[i].kind.is_footprint() {
                self.place_footprint(i);
            }
        }
        self.layout_offboard();
        self.lead_room = crate::autoplace::lead_room(self);
        for i in 0..self.parts.len() {
            for (k, leg) in self.parts[i].legs.clone().iter().enumerate() {
                let who = format!("{} leg {}", self.parts[i].r#ref, k + 1);
                self.reserve(leg, &who);
            }
        }
        for w in 0..self.wires.len() {
            let (a, b) = self.wires[w].spec.clone();
            let who = format!("W{}", self.wires[w].num);
            self.reserve(&a, &who);
            self.reserve(&b, &who);
        }
        let auto: Vec<usize> = (0..self.parts.len()).filter(|&i| self.parts[i].kind.is_lead() && self.parts[i].legs.is_empty()).collect();
        let mut pending: Vec<usize> = (0..self.parts.len()).filter(|&i| self.parts[i].kind.is_lead() && !auto.contains(&i)).collect();
        if !auto.is_empty() {
            // the port can't place legs yet; needs_placing() turned such files away
            unreachable!("two-lead parts without legs");
        }
        while !pending.is_empty() {
            let ready: Vec<usize> = pending.iter().copied()
                .filter(|&i| !self.parts[i].legs.iter().any(|l| self.waits_on_leads(l))).collect();
            if ready.is_empty() {
                for &i in &pending {
                    let r = self.parts[i].r#ref.clone();
                    self.issues.push(Issue::new("error", "BAD_LEG", format!("{}: its legs refer to two-lead parts that in \
                                                                              turn refer back to it", r)));
                }
                break;
            }
            for i in ready {
                pending.retain(|&p| p != i);
                self.place_leads(i);
            }
        }
        for w in 0..self.wires.len() {
            self.route(w);
        }
    }

    fn place_footprint(&mut self, pi: usize) {
        let at = match self.parts[pi].at.clone() {
            Some(a) if !a.is_empty() => a,
            _ => {
                let r = self.parts[pi].r#ref.clone();
                self.issues.push(Issue::new("error", "BAD_PLACE", format!("{}: bbgen couldn't place it; give it at:", r)));
                return;
            }
        };
        let anchor = match self.hole_of(&at) {
            Ok(h) => h,
            Err(e) => {
                let r = self.parts[pi].r#ref.clone();
                self.issues.push(Issue::new("error", "BAD_PLACE", format!("{} at {}: {}", r, at, e)));
                return;
            }
        };
        let bi = anchor.board as usize;
        let (ax, ay) = self.boards[bi].xy(anchor);
        let rot = self.parts[pi].rot;
        for k in 0..self.parts[pi].pins.len() {
            let (dx, dy) = rotate(self.parts[pi].pins[k].offset, rot);
            let xy = (ax + dx, ay + dy);
            self.parts[pi].pins[k].xy = Some(xy);
            let hole = self.boards[bi].hole_at(xy.0, xy.1)
                .or_else(|| self.boards.iter().filter(|b| b.index as usize != bi).find_map(|b| b.hole_at(xy.0, xy.1)));
            self.parts[pi].pins[k].hole = hole;
            let name = self.parts[pi].pin_str(k);
            match hole {
                None => self.issues.push(Issue::new("error", "OFF_HOLES", format!("{} lands in the trench or off the board", name))),
                Some(h) => {
                    self.occupy(h, &name);
                    self.pin_holes.insert(h);
                }
            }
        }
        let part = &self.parts[pi];
        let (x0, y0, x1, y1) = placed_box(part, body_rect(part).unwrap());
        let flat = matches!(part.kind, Kind::Pico | Kind::Dip) || part.source.starts_with("Package_DIP:")
            || part.source.starts_with("Module:") || part.source.contains("Package_DIP:") || part.source.contains("Module:");
        let m = if flat { 0.0 } else { CLEARANCE };
        let own: Vec<Option<Hole>> = part.pins.iter().map(|p| p.hole).collect();
        let r = part.r#ref.clone();
        let raised = part.raised;
        if raised {
            self.raised.push(((x0, y0, x1, y1), r.clone()));
            let along_x = rotate((1.0, 0.0), part.rot).1.abs() < 1e-9;
            let mut rows: Vec<(i64, Vec<Xy>)> = Vec::new();
            for pin in &part.pins {
                if let Some(xy) = pin.xy {
                    let l = if along_x { pyf::round_scaled(xy.1, 3) } else { pyf::round_scaled(xy.0, 3) };
                    match rows.iter_mut().find(|(k, _)| *k == l) {
                        Some(slot) => slot.1.push(xy),
                        None => rows.push((l, vec![xy])),
                    }
                }
            }
            for (_, pts) in rows {
                let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
                let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
                let (mnx, mxx, mny, mxy) = (parts::min(&xs), parts::max(&xs), parts::min(&ys), parts::max(&ys));
                let bx = if mxy - mny < 1e-9 { (mnx - 0.5, mny - 0.3, mxx + 0.5, mxy + 0.3) } else { (mnx - 0.3, mny - 0.5, mxx + 0.3, mxy + 0.5) };
                self.bodies.push((bx, r.clone()));
            }
        } else {
            self.bodies.push(((x0 - m, y0 - m, x1 + m, y1 + m), r.clone()));
            if !flat {
                self.tall.insert(r.clone());
            }
        }
        for ob in 0..self.boards.len() {
            for hole in self.boards[ob].holes() {
                let (x, y) = self.boards[ob].xy(hole);
                if x0 < x && x < x1 && y0 < y && y < y1 && !own.contains(&Some(hole)) {
                    if raised {
                        self.under.insert(pyf::key((x, y)), r.clone());
                    } else {
                        self.occupy(hole, &format!("the body of {}", r));
                    }
                }
            }
        }
        let keepouts = self.parts[pi].keepouts.clone();
        for (what, rect) in keepouts {
            let bx = placed_box(&self.parts[pi], rect);
            let name = format!("{}'s {}", r, what);
            self.bodies.push((bx, name.clone()));
            self.keepouts.insert(name.clone());
            if what.to_lowercase().contains("cable") {
                let board = &self.boards[bi];
                if bx.2 <= x0 {
                    self.cables.push(((board.left(), bx.1, bx.2, bx.3), board.index));
                } else if bx.0 >= x1 {
                    self.cables.push(((bx.0, bx.1, board.right(), bx.3), board.index));
                } else {
                    self.cables.push((bx, board.index));
                }
            }
            for hole in self.boards[bi].holes() {
                let (x, y) = self.boards[bi].xy(hole);
                if bx.0 < x && x < bx.2 && bx.1 < y && y < bx.3 && !own.contains(&Some(hole)) {
                    self.occupy(hole, &name);
                }
            }
        }
    }

    fn layout_offboard(&mut self) {
        let mut right = self.boards.iter().map(|b| b.right()).fold(f64::NEG_INFINITY, pymax) + 3.0;
        let mut below_x = self.boards.iter().map(|b| b.left()).fold(f64::INFINITY, pymin);
        let bottom = self.boards.iter().map(|b| b.bottom()).fold(f64::NEG_INFINITY, pymax);
        for part in self.parts.iter_mut() {
            if part.kind != Kind::Offboard {
                continue;
            }
            let n = part.pins.len() as i64;
            let cols = 1.max(part.cols.min(n));
            let rows = -((-n).div_euclid(cols));
            let grid: Vec<(i64, i64)> = if part.numbering == "row" {
                (0..n).map(|k| (k % cols, k / cols)).collect()
            } else {
                (0..n).map(|k| (k / rows, k % rows)).collect()
            };
            let ncols = grid.iter().map(|g| g.0).max().unwrap() + 1;
            let nrows = grid.iter().map(|g| g.1).max().unwrap() + 1;
            for (pin, &(c, r)) in part.pins.iter_mut().zip(&grid) {
                pin.label_dir = if ncols == 1 { (1, 0) } else if ncols == 2 { if c == 0 { (-1, 0) } else { (1, 0) } }
                                else if nrows == 2 && r == 1 { (0, 1) } else { (0, -1) };
            }
            let longest = part.pins.iter().map(|p| format!("{} {}", p.num, p.name).chars().count()).max().unwrap();
            let lw = longest as f64 * CHAR_W + 0.7;
            let pad_l = if ncols == 2 { lw } else { 0.8 };
            let pad_r = if ncols <= 2 { lw } else { 0.8 };
            let pad_t = if ncols > 2 { lw } else { 0.8 };
            let pad_b = if ncols > 2 && nrows == 2 { lw } else { 0.8 };
            let (ox, oy);
            if part.place == "right" {
                ox = right + pad_l;
                oy = 1.0;
                right = ox + ncols as f64 - 1.0 + pad_r + 2.0;
            } else {
                ox = below_x + pad_l;
                oy = bottom + 1.5 + pad_t;
                below_x = ox + ncols as f64 - 1.0 + pad_r + 2.0;
            }
            for (pin, &(c, r)) in part.pins.iter_mut().zip(&grid) {
                pin.xy = Some((ox + c as f64, oy + r as f64));
            }
            part.r#box = Some((ox - 0.6, oy - 0.6, ox + ncols as f64 - 0.4, oy + nrows as f64 - 0.4));
            part.extent = Some((ox - pad_l, oy - pymax(pad_t, 1.6), ox + ncols as f64 - 1.0 + pad_r, oy + nrows as f64 - 1.0 + pad_b));
        }
    }

    fn place_leads(&mut self, pi: usize) {
        let legs = self.parts[pi].legs.clone();
        let r = self.parts[pi].r#ref.clone();
        let mut options: Vec<Option<Vec<Opt>>> = Vec::new();
        for (i, spec) in legs.iter().enumerate() {
            let hint = self.hint(&legs[1 - i]);
            let found = self.options(spec, hint, false).and_then(|found| {
                if found.iter().any(|o| o.pin.is_some()) {
                    Err("a leg goes in a breadboard hole, not an off-board pin".to_string())
                } else {
                    Ok(found)
                }
            });
            match found {
                Ok(f) => options.push(Some(f)),
                Err(e) => {
                    self.issues.push(Issue::new("error", "BAD_LEG", format!("{} leg {} ({}): {}", r, i + 1, spec, e)));
                    options.push(None);
                }
            }
        }
        let chosen: Vec<Option<Opt>> = if options.iter().all(|o| o.is_some()) {
            match self.best(options[0].as_ref().unwrap(), options[1].as_ref().unwrap(), false) {
                Ok((a, b)) => vec![Some(a), Some(b)],
                Err(e) => {
                    self.issues.push(Issue::new("error", "BAD_LEG", format!("{}: {}", r, e)));
                    return;
                }
            }
        } else {
            options.iter().map(|o| o.as_ref().map(|o| {
                let mut best = o[0];
                for x in &o[1..] {
                    if (x.near, x.xy.0, x.xy.1) < (best.near, best.xy.0, best.xy.1) {
                        best = *x;
                    }
                }
                best
            })).collect()
        };
        for (i, option) in chosen.iter().enumerate() {
            if let Some(o) = option {
                let end = self.take(o, &format!("{} leg {}", r, i + 1), "");
                self.parts[pi].pins[i].hole = end.hole;
                self.parts[pi].pins[i].xy = Some(o.xy);
            }
        }
        let part = &self.parts[pi];
        if let (Some(a), Some(b)) = (part.pins[0].xy, part.pins[1].xy) {
            let round = part.kind.round_body();
            self.add_segment(a, b, &r);
            if aligned(a, b) {
                for k in self.between(a, b) {
                    self.covered.insert(k, r.clone());
                }
            }
            if let Some(rad) = round {
                let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                self.bodies.push(((cx - rad, cy - rad, cx + rad, cy + rad), r.clone()));
                self.tall.insert(r.clone());
            }
        }
    }

    fn route(&mut self, wi: usize) {
        let (sa, sb) = self.wires[wi].spec.clone();
        let who = format!("W{}", self.wires[wi].num);
        let flex = self.wires[wi].flex;
        let result: Result<(Opt, Opt, Vec<Xy>), String> = (|| {
            let via_xy: Vec<Xy> = self.wires[wi].via.iter().map(|v| self.via(v)).collect::<Result<_, _>>()?;
            let a = self.options(&sa, self.hint(&sb), !flex)?;
            let b = self.options(&sb, self.hint(&sa), !flex)?;
            if !via_xy.is_empty() {
                let pa = self.best(&a, &[Opt::at(None, via_xy[0])], false)?.0;
                let bb: Vec<Opt> = b.iter().copied().filter(|o| o.hole.is_none() || o.hole != pa.hole).collect();
                let pb = self.best(&bb, &[Opt::at(None, *via_xy.last().unwrap())], false)?.0;
                Ok((pa, pb, via_xy))
            } else {
                let (pa, pb) = self.best(&a, &b, flex)?;
                Ok((pa, pb, via_xy))
            }
        })();
        match result {
            Err(e) => {
                let n = self.wires[wi].num;
                self.issues.push(Issue::new("error", "BAD_WIRE", format!("W{} ({} -> {}): {}", n, sa, sb, e)));
                self.wires[wi].a = None;
                self.wires[wi].b = None;
            }
            Ok((pa, pb, via_xy)) => {
                self.wires[wi].via_xy = via_xy;
                let ea = self.take(&pa, &who, &sa);
                let eb = self.take(&pb, &who, &sb);
                self.wires[wi].a = Some(ea);
                self.wires[wi].b = Some(eb);
                if flex {
                    return;
                }
                let pts = self.wire_points(&self.wires[wi]);
                for s in pts.windows(2) {
                    self.add_segment(s[0], s[1], &who);
                }
            }
        }
    }

    /// The best pair of places for two ends: straight and clear, then over as few part bodies as possible, then
    /// nearest their pins, then shortest. Flying leads and flexible jumpers only care about the last two.
    pub fn best(&self, a: &[Opt], b: &[Opt], flex: bool) -> Result<(Opt, Opt), Unresolved> {
        let mut best: Option<((i32, usize, f64, f64, f64, f64, f64, f64), Opt, Opt)> = None;
        for p in a {
            for q in b {
                if !(p.hole.is_none() || p.hole != q.hole) {
                    continue;
                }
                let flying = flex || p.pin.is_some() || q.pin.is_some();
                let k = (if flying { 0 } else { self.shape(p.xy, q.xy).0 },
                         if flying { 0 } else { self.bodies_crossed(p.xy, q.xy).len() },
                         pyf::round(p.near + q.near, 6), pyf::round(pyf::dist(p.xy, q.xy), 6), p.xy.0, p.xy.1, q.xy.0, q.xy.1);
                if best.as_ref().map(|(bk, _, _)| k.partial_cmp(bk) == Some(std::cmp::Ordering::Less)).unwrap_or(true) {
                    best = Some((k, *p, *q));
                }
            }
        }
        best.map(|(_, p, q)| (p, q)).ok_or("both ends would need the same hole".to_string())
    }

    /// How a run from a to b would lie: 0 straight and clear, 1 straight but over holes in use, 2 slanted, 3
    /// straight along another wire or part; and what is in its way.
    pub fn shape(&self, a: Xy, b: Xy) -> (i32, Vec<String>) {
        if !aligned(a, b) {
            return (2, vec![]);
        }
        let along = self.along_who(a, b, "");
        if !along.is_empty() {
            return (3, along.into_iter().map(String::from).collect());
        }
        let over: Vec<String> = self.between(a, b).iter().filter_map(|k| self.used.get(k).cloned()).collect();
        if over.is_empty() { (0, over) } else { (1, over) }
    }

    /// Just the level of shape().
    pub fn shape_level(&self, a: Xy, b: Xy) -> i32 {
        if !aligned(a, b) {
            return 2;
        }
        if self.along_any(a, b, "") {
            return 3;
        }
        if self.between(a, b).iter().any(|k| self.used.contains_key(k)) { 1 } else { 0 }
    }

    pub fn add_segment(&mut self, a: Xy, b: Xy, who: &str) {
        self.segments.push((a, b, who.to_string()));
        if let Some((k, lo, hi)) = line(a, b) {
            self.lines.entry(k).or_default().push((lo, hi, who.to_string()));
        }
    }

    pub fn reindex(&mut self) {
        let segments = std::mem::take(&mut self.segments);
        self.lines.clear();
        for (a, b, who) in segments {
            self.add_segment(a, b, &who);
        }
    }

    /// What lies along the straight run a-b, sharing some of its length.
    pub fn along_who(&self, a: Xy, b: Xy, exclude: &str) -> Vec<&str> {
        match line(a, b) {
            None => vec![],
            Some((k, lo, hi)) => self.lines.get(&k).map(|v| v.iter()
                .filter(|(l2, h2, who)| who != exclude && pymin(hi, *h2) - pymax(lo, *l2) > EPS)
                .map(|(_, _, who)| who.as_str()).collect()).unwrap_or_default(),
        }
    }

    pub fn along_any(&self, a: Xy, b: Xy, exclude: &str) -> bool {
        match line(a, b) {
            None => false,
            Some((k, lo, hi)) => self.lines.get(&k).map(|v| v.iter()
                .any(|(l2, h2, who)| who != exclude && pymin(hi, *h2) - pymax(lo, *l2) > EPS)).unwrap_or(false),
        }
    }

    /// Hole positions strictly between a and b on a straight run.
    pub fn between(&self, a: Xy, b: Xy) -> Vec<K> {
        if (a.1 - b.1).abs() < EPS {
            let (lo, hi) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
            let start = (lo + EPS).floor() as i64 + 1;
            let end = (hi - EPS).ceil() as i64;
            let ky = pyf::round_scaled(a.1, 3);
            return (start..end).map(|x| (x * 1000, ky)).collect();
        }
        let (lo, hi) = if a.1 <= b.1 { (a.1, b.1) } else { (b.1, a.1) };
        let kx = pyf::round_scaled(a.0, 3);
        self.levels.iter().filter(|&&y| lo + EPS < y && y < hi - EPS).map(|&y| (kx, pyf::round_scaled(y, 3))).collect()
    }

    /// The bodies (indices into `bodies`) a straight run crosses.
    pub fn bodies_crossed(&self, a: Xy, b: Xy) -> Vec<usize> {
        let (x0, x1, y0, y1) = (pymin(a.0, b.0), pymax(a.0, b.0), pymin(a.1, b.1), pymax(a.1, b.1));
        (0..self.bodies.len()).filter(|&i| {
            let bx = self.bodies[i].0;
            x1 > bx.0 && x0 < bx.2 && y1 > bx.1 && y0 < bx.3 && crosses(a, b, bx)
        }).collect()
    }

    pub fn crosses_any_body(&self, a: Xy, b: Xy) -> bool {
        let (x0, x1, y0, y1) = (pymin(a.0, b.0), pymax(a.0, b.0), pymin(a.1, b.1), pymax(a.1, b.1));
        self.bodies.iter().any(|(bx, _)| x1 > bx.0 && x0 < bx.2 && y1 > bx.1 && y0 < bx.3 && crosses(a, b, *bx))
    }

    pub fn body_name(&self, i: usize) -> &str {
        &self.bodies[i].1
    }

    pub fn take(&mut self, option: &Opt, who: &str, spec: &str) -> End {
        if option.pin.is_some() {
            return End { spec: spec.into(), hole: None, pin: option.pin };
        }
        if !option.fixed {
            self.occupy(option.hole.unwrap(), who);
        }
        End { spec: spec.into(), hole: option.hole, pin: None }
    }

    /// Holes on the board under a cable's plug or its way off the board: the room it costs.
    pub fn cable_holes(&self) -> usize {
        let mut n = 0;
        for (bx, board) in &self.cables {
            let b = &self.boards[*board as usize];
            for h in b.holes() {
                let (x, y) = b.xy(h);
                if bx.0 < x && x < bx.2 && bx.1 < y && y < bx.3 {
                    n += 1;
                }
            }
        }
        n
    }

    pub fn wire_points(&self, w: &Wire) -> Vec<Xy> {
        let mut pts = vec![self.end_xy(w.a.as_ref().unwrap())];
        pts.extend(w.via_xy.iter().copied());
        pts.push(self.end_xy(w.b.as_ref().unwrap()));
        pts
    }

    // ---- slants ----

    fn find_slants(&mut self) {
        for wi in 0..self.wires.len() {
            let w = &self.wires[wi];
            if w.on_board() && w.via.is_empty() && w.slant.is_none() && !w.flex {
                let (name, specs) = (format!("W{}", w.num), vec![w.spec.0.clone(), w.spec.1.clone()]);
                let holes = (w.a.as_ref().unwrap().hole.unwrap(), w.b.as_ref().unwrap().hole.unwrap());
                let auto = w.auto;
                self.slant(&name, &specs, holes, auto);
            }
        }
        for pi in 0..self.parts.len() {
            let p = &self.parts[pi];
            if p.kind.is_lead() && p.slant.is_none() && p.pins.iter().all(|x| x.hole.is_some()) {
                let (name, specs, holes, auto) = (p.r#ref.clone(), p.legs.clone(), (p.pins[0].hole.unwrap(), p.pins[1].hole.unwrap()), p.auto);
                self.slant(&name, &specs, holes, auto);
            }
        }
    }

    fn slant(&mut self, name: &str, specs: &[String], holes: (Hole, Hole), auto: bool) {
        if holes.0.board != holes.1.board {
            return;
        }
        let board = &self.boards[holes.0.board as usize];
        if aligned(board.xy(holes.0), board.xy(holes.1)) {
            return;
        }
        let route = format!("{} slants ({} → {})", name, self.hole_name(holes.0), self.hole_name(holes.1));
        let accept = if auto { "or place the parts so it can run straight".to_string() } else {
            format!("or accept it with slant: <reason> on {}", name)
        };
        let strips = [board.strip_holes(board.strip(holes.0)), board.strip_holes(board.strip(holes.1))];
        let mut straight: Vec<(Hole, Hole)> = Vec::new();
        for &p in &strips[0] {
            for &q in &strips[1] {
                if p != q && aligned(board.xy(p), board.xy(q)) {
                    straight.push((p, q));
                }
            }
        }
        if straight.is_empty() {
            let rail = [holes.0.is_rail(), holes.1.is_rail()];
            if rail[0] != rail[1] {
                let i = if !rail[0] { 0 } else { 1 };
                let hs = [holes.0, holes.1];
                let spec = &specs[i];
                let (r, token) = match spec.split_once('.') {
                    Some((r, t)) => (r, t),
                    None => (spec.as_str(), ""),
                };
                let part = if token.is_empty() { None } else { self.part_index.get(r).map(|&k| &self.parts[k as usize]) };
                let fix = match part {
                    None => "use a column that has rail holes".to_string(),
                    Some(p) if p.kind.is_lead() => {
                        let leg = &p.pins[p.find(token)[0]];
                        format!("put {} leg {} ({}) in a column that has rail holes", r, leg.num, self.hole_name(leg.hole.unwrap()))
                    }
                    Some(_) => format!("move {}", r),
                };
                let gaps = (1..=board.columns).filter(|c| !board.rail_cols.contains(c)).map(|c| c.to_string())
                    .collect::<Vec<_>>().join(", ");
                let fixc = capitalize(&fix);
                self.issues.push(Issue::new("warning", "DIAGONAL", format!(
                    "{}: column {} has no {} hole (the rails have none in columns {}). {}, {}",
                    route, hs[i].col, hs[1 - i].row.name(), gaps, fixc, accept)));
            }
            return;
        }
        let mine = |who: &str| who == name || who.starts_with(&format!("{} ", name));
        let mut blockers: Vec<String> = Vec::new();
        let mut usable: Vec<(Hole, Hole)> = Vec::new();
        for &(p, q) in &straight {
            if self.pin_holes.contains(&p) || self.pin_holes.contains(&q) {
                continue;
            }
            let (pxy, qxy) = (board.xy(p), board.xy(q));
            let mut here: Vec<String> = Vec::new();
            for (h, now) in [(p, holes.0), (q, holes.1)] {
                if h != now {
                    if let Some(who) = self.occupied.get(&h) {
                        here.push(who.clone());
                    }
                }
            }
            here.extend(self.bodies_crossed(pxy, qxy).into_iter().map(|i| self.bodies[i].1.clone()));
            let (level, way) = self.shape(pxy, qxy);
            if level == 1 || level == 3 {
                here.extend(way);
            }
            here.retain(|w| !mine(w));
            if here.is_empty() {
                usable.push((p, q));
            } else {
                blockers.extend(here);
            }
        }
        if !usable.is_empty() {
            let changes = |pair: &(Hole, Hole)| -> Vec<(Hole, Hole)> {
                [(pair.0, holes.0), (pair.1, holes.1)].into_iter().filter(|(n, o)| n != o).collect()
            };
            let mut best = usable[0];
            let key = |pair: &(Hole, Hole)| (changes(pair).len(), pyf::dist(board.xy(pair.0), board.xy(pair.1)));
            let mut bk = key(&best);
            for pair in &usable[1..] {
                let k = key(pair);
                if k.partial_cmp(&bk) == Some(std::cmp::Ordering::Less) {
                    best = *pair;
                    bk = k;
                }
            }
            let fix = changes(&best).iter().map(|(n, o)| format!("{} instead of {}", self.hole_name(*n), self.hole_name(*o)))
                .collect::<Vec<_>>().join(" and ");
            self.issues.push(Issue::new("warning", "DIAGONAL", format!("{}: {} would make it straight", route, fix)));
            return;
        }
        let mut names: Vec<String> = Vec::new();
        for who in blockers {
            let mut w = who.as_str();
            if let Some(rest) = w.strip_prefix("the body of ") {
                w = rest;
            }
            let w = strip_leg_suffix(w);
            if !names.contains(&w) {
                names.push(w);
            }
        }
        let what = if names.is_empty() { String::new() } else { format!(" (by {})", names.iter().take(5).cloned().collect::<Vec<_>>().join(", ")) };
        let fix = if auto { "Move parts to make room".to_string() } else { format!("Move parts to make room, bend it with via:, {}", accept) };
        self.issues.push(Issue::new("warning", "DIAGONAL", format!("{}: every straight route is blocked{}. {}", route, what, fix)));
    }

    fn find_over_parts(&mut self) {
        let wire_names: HashSet<String> = self.wires.iter().map(|x| format!("W{}", x.num)).collect();
        let mut issues = Vec::new();
        for w in &self.wires {
            let name = format!("W{}", w.num);
            for end in [&w.a, &w.b].into_iter().flatten() {
                if let Some(h) = end.hole {
                    if let Some(r) = self.covered.get(&pyf::key(self.end_xy(end))) {
                        issues.push(Issue::new("warning", "OVER_PART", format!("{} ends in {}, under {}: move one of them",
                                                                               name, self.hole_name(h), r)));
                    }
                }
            }
            if w.flex && w.on_board() {
                let zones: Vec<&str> = self.bodies_crossed(self.end_xy(w.a.as_ref().unwrap()), self.end_xy(w.b.as_ref().unwrap()))
                    .into_iter().map(|i| self.body_name(i)).filter(|z| self.keepouts.contains(*z)).collect();
                if !zones.is_empty() {
                    issues.push(Issue::new("warning", "OVER_PART", format!("{} arches over {}, which needs that room: move one of them",
                                                                           name, zones.join(" and "))));
                }
            }
            if !w.on_board() || w.flex {
                continue;
            }
            let pts = self.wire_points(w);
            let ends = format!("({} → {})", self.hole_name(w.a.as_ref().unwrap().hole.unwrap()), self.hole_name(w.b.as_ref().unwrap().hole.unwrap()));
            let mut along: Vec<&str> = Vec::new();
            for s in pts.windows(2) {
                for who in self.along_who(s[0], s[1], &name) {
                    if !wire_names.contains(who) && !along.contains(&who) {
                        along.push(who);
                    }
                }
            }
            if !along.is_empty() {
                issues.push(Issue::new("warning", "OVER_PART", format!("{} runs along {} {}, hiding it: move one of them",
                                                                       name, along.join(" and "), ends)));
            }
            let mut refs: Vec<&str> = Vec::new();
            for s in pts.windows(2) {
                for i in self.bodies_crossed(s[0], s[1]) {
                    let r = self.body_name(i);
                    if !refs.contains(&r) {
                        refs.push(r);
                    }
                }
            }
            if refs.is_empty() {
                continue;
            }
            let route = format!("{} runs over {} {}", name, refs.join(" and "), ends);
            let detour = if !w.via.is_empty() { None } else {
                self.detour(w.a.as_ref().unwrap().hole.unwrap(), w.b.as_ref().unwrap().hole.unwrap(), &name)
            };
            let fix = match detour {
                Some(d) => format!("bend it around with via: [{}]", d.join(", ")),
                None if !w.via.is_empty() => "move the bends in its via".to_string(),
                None => "move a part to make room".to_string(),
            };
            issues.push(Issue::new("warning", "OVER_PART", format!("{}: {}", route, fix)));
        }
        for part in &self.parts {
            if part.kind.is_lead() && part.pins.iter().all(|p| p.xy.is_some()) {
                let refs: Vec<&str> = self.bodies_crossed(part.pins[0].xy.unwrap(), part.pins[1].xy.unwrap()).into_iter()
                    .map(|i| self.body_name(i)).filter(|r| *r != part.r#ref).collect();
                if !refs.is_empty() {
                    issues.push(Issue::new("warning", "OVER_PART", format!(
                        "{} lies over {} ({} → {}): put its legs where it clears the part", part.r#ref, refs.join(" and "),
                        self.hole_name(part.pins[0].hole.unwrap()), self.hole_name(part.pins[1].hole.unwrap()))));
                }
            }
        }
        self.issues.extend(issues);
    }

    fn find_under(&mut self) {
        let mut issues = Vec::new();
        for part in &self.parts {
            if !part.kind.is_lead() || !part.pins.iter().all(|p| p.xy.is_some()) {
                continue;
            }
            for pin in &part.pins {
                if let Some(r) = self.under.get(&pyf::key(pin.xy.unwrap())) {
                    issues.push(Issue::new("error", "HOLE_TAKEN", format!(
                        "{} leg in {}, under {}: only a flat wire goes under a raised module", part.r#ref,
                        self.hole_name(pin.hole.unwrap()), r)));
                }
            }
            let refs = self.raised_crossed(part.pins[0].xy.unwrap(), part.pins[1].xy.unwrap());
            if !refs.is_empty() {
                issues.push(Issue::new("warning", "OVER_PART", format!(
                    "{} lies across {}, which stands over it: put its legs where it clears the part", part.r#ref, refs.join(" and "))));
            }
        }
        for w in &self.wires {
            let lead = w.a.as_ref().map(|e| e.pin.is_some()).unwrap_or(false) || w.b.as_ref().map(|e| e.pin.is_some()).unwrap_or(false);
            if !(w.flex || lead) {
                continue;
            }
            for end in [&w.a, &w.b].into_iter().flatten() {
                if let Some(h) = end.hole {
                    if let Some(r) = self.under.get(&pyf::key(self.end_xy(end))) {
                        let what = if w.flex { "a flexible jumper's" } else { "a flying lead's" };
                        issues.push(Issue::new("error", "HOLE_TAKEN", format!(
                            "W{} ends in {}, under {}: {} plug won't go under a raised module", w.num, self.hole_name(h), r, what)));
                    }
                }
            }
        }
        self.issues.extend(issues);
    }

    /// The raised modules a straight run from a to b passes under (or a part would lie across).
    pub fn raised_crossed(&self, a: Xy, b: Xy) -> Vec<&str> {
        let (x0, x1, y0, y1) = (pymin(a.0, b.0), pymax(a.0, b.0), pymin(a.1, b.1), pymax(a.1, b.1));
        self.raised.iter().filter(|(bx, _)| x1 > bx.0 && x0 < bx.2 && y1 > bx.1 && y0 < bx.3 && crosses(a, b, *bx))
            .map(|(_, r)| r.as_str()).collect()
    }

    /// Positions of the holes a wire lying on the board covers: between its ends, and at its bends.
    pub fn lies_over(&self, w: &Wire) -> HashSet<K> {
        let mut out = HashSet::default();
        if w.flex || !w.on_board() {
            return out;
        }
        let pts = self.wire_points(w);
        for v in &w.via_xy {
            out.insert(pyf::key(*v));
        }
        for s in pts.windows(2) {
            if aligned(s[0], s[1]) {
                out.extend(self.between(s[0], s[1]));
            }
        }
        out
    }

    fn find_stacking(&mut self) {
        let rank = |w: &Wire| (w.flex, w.num);
        let flat: Vec<(usize, HashSet<K>)> = (0..self.wires.len()).filter(|&i| !self.wires[i].flex)
            .map(|i| (i, self.lies_over(&self.wires[i]))).collect();
        let mut issues = Vec::new();
        for (wi, w) in self.wires.iter().enumerate() {
            for end in [&w.a, &w.b].into_iter().flatten() {
                let h = match end.hole {
                    Some(h) => h,
                    None => continue,
                };
                let k = pyf::key(self.end_xy(end));
                for (oi, over) in &flat {
                    let other = &self.wires[*oi];
                    if *oi != wi && over.contains(&k) && rank(other) < rank(w) {
                        let what = if w.flex { "the jumper".to_string() } else { format!("W{}", w.num) };
                        issues.push(Issue::new("error", "END_COVERED", format!(
                            "W{} ends in {}, under W{}, which goes in first: {} can't be plugged in. Move one of them",
                            w.num, self.hole_name(h), other.num, what)));
                    }
                }
            }
        }
        for (i, (wi, over)) in flat.iter().enumerate() {
            let w = &self.wires[*wi];
            for (oi, _) in &flat[..i] {
                let other = &self.wires[*oi];
                let ends: Vec<K> = [&other.a, &other.b].into_iter().flatten().filter(|e| e.hole.is_some())
                    .map(|e| pyf::key(self.end_xy(e))).collect();
                if rank(other) < rank(w) && ends.iter().all(|k| over.contains(k))
                    && other.a.as_ref().map(|e| e.hole.is_some()).unwrap_or(false) {
                    issues.push(Issue::new("warning", "NESTED", format!(
                        "W{} lies over W{}, hiding it, and has to come out before it can: buildable, but hard to trace \
                         (nested: flex makes the outer one a flexible jumper)", w.num, other.num)));
                }
            }
        }
        self.issues.extend(issues);
    }

    pub fn detour(&self, a: Hole, b: Hole, who: &str) -> Option<Vec<String>> {
        self.detour_cost(a, b, who).map(|(_, names)| names)
    }

    /// Bends (one or two holes) that take a wire from a to b around every part body, lying along as few other
    /// wires and parts and passing as few holes in use or under parts as possible, then shortest.
    pub fn detour_cost(&self, a: Hole, b: Hole, who: &str) -> Option<((i64, f64), Vec<String>)> {
        if a.board != b.board {
            return None;
        }
        let board = &self.boards[a.board as usize];
        let mut routes: Vec<Vec<(Row, i32)>> = vec![vec![(a.row, b.col)], vec![(b.row, a.col)]];
        let lo = 1.max(a.col.min(b.col) - DETOUR_REACH);
        let hi = board.columns.min(a.col.max(b.col) + DETOUR_REACH);
        for col in lo..=hi {
            routes.push(vec![(a.row, col), (b.row, col)]);
        }
        let mut best: Option<((i64, f64), Vec<(Row, i32)>)> = None;
        for route in routes {
            if !route.iter().all(|&(row, col)| board.has(row, col)) {
                continue;
            }
            let mut pts = vec![board.xy(a)];
            pts.extend(route.iter().map(|&(row, col)| board.xy(Hole::new(board.index, row, col))));
            pts.push(board.xy(b));
            if pts.windows(2).any(|s| self.crosses_any_body(s[0], s[1])) {
                continue;
            }
            let along: usize = pts.windows(2).map(|s| self.along_who(s[0], s[1], who).len()).sum();
            let mut over = 10 * along as i64;
            for s in pts.windows(2) {
                for k in self.between(s[0], s[1]) {
                    if self.used.contains_key(&k) || self.covered.contains_key(&k) {
                        over += 1;
                    }
                }
            }
            let cost = (over, pyf::round(pyf::fsum(pts.windows(2).map(|s| pyf::dist(s[0], s[1]))), 6));
            if best.as_ref().map(|(bc, _)| cost.partial_cmp(bc) == Some(std::cmp::Ordering::Less)).unwrap_or(true) {
                best = Some((cost, route));
            }
        }
        best.map(|(cost, route)| (cost, route.iter().map(|&(row, col)| self.hole_name(Hole::new(board.index, row, col))).collect()))
    }

    // ---- resolving holes and pins ----

    fn board_of(&self, id: Option<&str>) -> Result<u8, Unresolved> {
        match id {
            None => Ok(self.default_board),
            Some(id) => self.board_by_id(id).ok_or(format!("there is no board '{}'", id)),
        }
    }

    pub fn hole_of(&self, text: &str) -> Result<Hole, Unresolved> {
        let parsed = parse_hole(text);
        let (bid, row, col) = match parsed {
            Some((b, r, Some(c))) => (b, r, c),
            _ => return Err(format!("'{}' is not a hole (like a12, top+5 or 2:f30)", text)),
        };
        let bi = self.board_of(bid.as_deref())?;
        let board = &self.boards[bi as usize];
        if !board.has(row, col) {
            return Err(format!("board {} has no hole {}{}", board.id, row.name(), col));
        }
        Ok(Hole::new(bi, row, col))
    }

    pub fn occupy(&mut self, hole: Hole, who: &str) {
        if let Some(prev) = self.occupied.get(&hole) {
            let name = self.boards[hole.board as usize].name(hole, self.default_board);
            let msg = format!("{}: {} and {} both want this hole", name, who, prev);
            self.issues.push(Issue::new("error", "HOLE_TAKEN", msg));
        } else {
            self.occupied.insert(hole, who.to_string());
            self.used.insert(pyf::key(self.boards[hole.board as usize].xy(hole)), who.to_string());
        }
    }

    fn reserve(&mut self, spec: &str, who: &str) {
        if let Some((_, _, Some(_))) = parse_hole(spec) {
            if let Ok(h) = self.hole_of(spec) {
                self.occupy(h, who);
            }
        }
    }

    fn waits_on_leads(&self, spec: &str) -> bool {
        let r = spec.split('.').next().unwrap();
        match self.part_index.get(r) {
            Some(&i) => {
                let p = &self.parts[i as usize];
                p.kind.is_lead() && spec.contains('.') && p.pins.iter().any(|x| x.hole.is_none())
            }
            None => false,
        }
    }

    pub fn pin_of(&self, spec: &str, hint: Hint) -> Result<PinId, Unresolved> {
        let (r, token) = match spec.split_once('.') {
            Some(x) => x,
            None => return Err(format!("'{}' is neither a hole (like a12 or top+5) nor a part pin (like U1.3 or U1.VCC)", spec)),
        };
        let pi = self.part_index.get(r).copied().or_else(|| {
            let low = r.to_lowercase();
            self.parts.iter().position(|p| p.r#ref.to_lowercase() == low).map(|i| i as u16)
        }).ok_or(format!("there is no part {}", r))?;
        let part = &self.parts[pi as usize];
        let pins = part.find(token);
        if pins.is_empty() {
            return Err(format!("{} has no pin {}", part.r#ref, crate::val::repr_str(token)));
        }
        if pins.len() == 1 {
            return Ok(PinId { part: pi, pin: pins[0] as u16 });
        }
        if let Some(bus) = part.bus_of(pins[0]) {
            if pins.iter().all(|p| bus.contains(p)) {
                let placed: Vec<usize> = pins.iter().copied().filter(|&p| part.pins[p].xy.is_some()).collect();
                if placed.is_empty() {
                    return Ok(PinId { part: pi, pin: pins[0] as u16 });
                }
                let mut best = placed[0];
                let mut bd = hint_dist(part.pins[best].xy.unwrap(), hint);
                for &p in &placed[1..] {
                    let d = hint_dist(part.pins[p].xy.unwrap(), hint);
                    if d < bd {
                        best = p;
                        bd = d;
                    }
                }
                return Ok(PinId { part: pi, pin: best as u16 });
            }
        }
        Err(format!("{}.{} is ambiguous: pins {}", part.r#ref, token,
                    pins.iter().map(|&p| part.pins[p].num.to_string()).collect::<Vec<_>>().join(", ")))
    }

    fn net_pins(&self, spec: &str) -> Result<Vec<PinId>, Unresolved> {
        let (r, token) = spec.split_once('.').unwrap_or((spec, ""));
        let pi = match (spec.contains('.'), self.part_index.get(r)) {
            (true, Some(&pi)) => pi,
            _ => return Err(format!("'{}' is not a part pin (like U1.3 or U1.VCC)", spec)),
        };
        let part = &self.parts[pi as usize];
        let pins = part.find(token);
        if pins.is_empty() {
            return Err(format!("{} has no pin {}", r, crate::val::repr_str(token)));
        }
        let mut out: Vec<PinId> = Vec::new();
        for p in pins {
            let unit: Vec<usize> = part.bus_of(p).cloned().unwrap_or(vec![p]);
            for q in unit {
                let id = PinId { part: pi, pin: q as u16 };
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        Ok(out)
    }

    pub fn hint(&self, spec: &str) -> Hint {
        if let Some((bid, row, col)) = parse_hole(spec) {
            let bi = match bid {
                None => self.default_board,
                Some(id) => self.board_by_id(&id)?,
            };
            let board = &self.boards[bi as usize];
            return match col {
                None => if row.is_rail() { Some((None, Some(board.y(row)))) } else { None },
                Some(col) => if board.has(row, col) {
                    let xy = board.xy(Hole::new(bi, row, col));
                    Some((Some(xy.0), Some(xy.1)))
                } else { None },
            };
        }
        match self.pin_of(spec, None) {
            Ok(p) => self.pin(p).xy.map(|xy| (Some(xy.0), Some(xy.1))),
            Err(_) => None,
        }
    }

    /// Every place one end could go: a hole written out, a free hole on a bare rail, a free hole in a pin's strip,
    /// or an off-board pin. Only a flat wire's end may go under a raised module.
    pub fn options(&self, spec: &str, hint: Hint, flat: bool) -> Result<Vec<Opt>, Unresolved> {
        if let Some((bid, row, col)) = parse_hole(spec) {
            if col.is_some() {
                let hole = self.hole_of(spec)?;
                let mut o = Opt::at(Some(hole), self.boards[hole.board as usize].xy(hole));
                o.fixed = true;
                return Ok(vec![o]);
            }
            if !row.is_rail() {
                return Err(format!("'{}' needs a column number", spec));
            }
            let bi = self.board_of(bid.as_deref())?;
            let holes: Vec<Hole> = self.boards[bi as usize].holes().into_iter().filter(|h| h.row == row).collect();
            return self.free(bi, &holes, None, &format!("the {} rail", row.name()), flat);
        }
        let pid = self.pin_of(spec, hint)?;
        let part = &self.parts[pid.part as usize];
        let pin = &part.pins[pid.pin as usize];
        if part.kind == Kind::Offboard {
            let mut o = Opt::at(None, pin.xy.unwrap());
            o.pin = Some(pid);
            return Ok(vec![o]);
        }
        let hole = pin.hole.ok_or(format!("{} is not on the board", part.pin_str(pid.pin as usize)))?;
        let board = &self.boards[hole.board as usize];
        let holes = board.strip_holes(board.strip(hole));
        self.free(hole.board, &holes, pin.xy.map(|xy| (Some(xy.0), Some(xy.1))),
                  &format!("the strip of {}", part.pin_str(pid.pin as usize)), flat)
    }

    fn free(&self, bi: u8, holes: &[Hole], near: Hint, what: &str, flat: bool) -> Result<Vec<Opt>, Unresolved> {
        let board = &self.boards[bi as usize];
        let free: Vec<Opt> = holes.iter().filter(|h| !self.occupied.contains_key(h)
            && (flat || !self.under.contains_key(&pyf::key(board.xy(**h))))).map(|&h| {
            let mut o = Opt::at(Some(h), board.xy(h));
            o.near = hint_dist(board.xy(h), near);
            o
        }).collect();
        if free.is_empty() {
            return Err(format!("no free hole left in {}", what));
        }
        Ok(free)
    }

    fn via(&self, spec: &str) -> Result<Xy, Unresolved> {
        let hole = self.hole_of(spec)?;
        Ok(self.boards[hole.board as usize].xy(hole))
    }

    #[inline]
    pub fn pin(&self, p: PinId) -> &parts::Pin {
        &self.parts[p.part as usize].pins[p.pin as usize]
    }

    pub fn pin_str(&self, p: PinId) -> String {
        self.parts[p.part as usize].pin_str(p.pin as usize)
    }

    #[inline]
    pub fn end_xy(&self, end: &End) -> Xy {
        match end.pin {
            Some(p) => self.pin(p).xy.unwrap(),
            None => {
                let h = end.hole.unwrap();
                self.boards[h.board as usize].xy(h)
            }
        }
    }

    pub fn hole_name(&self, hole: Hole) -> String {
        self.boards[hole.board as usize].name(hole, self.default_board)
    }

    #[inline]
    pub fn xy(&self, hole: Hole) -> Xy {
        self.boards[hole.board as usize].xy(hole)
    }

    pub fn step_key(a: Xy, b: Xy) -> (u64, u64, u64, u64) {
        let (p, q) = (bits(a), bits(b));
        (p.0, p.1, q.0, q.1)
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `re.sub(r" leg \d$", "", who)`
fn strip_leg_suffix(w: &str) -> String {
    let b = w.as_bytes();
    if b.len() >= 6 && b[b.len() - 1].is_ascii_digit() && w[..w.len() - 1].ends_with(" leg ") {
        return w[..w.len() - 6].to_string();
    }
    w.to_string()
}

pub fn library_for(data: &Val) -> Option<Vec<PathBuf>> {
    let parts = data.at("parts").as_map()?;
    if parts.iter().any(|(_, s)| s.as_map().is_some() && (s.at("symbol").truthy() || s.at("footprint").truthy())) {
        let extra = data.get("kicad").filter(|v| v.truthy()).map(Val::py_str);
        return Some(crate::kicad::library_roots(extra.as_deref()));
    }
    None
}

/// Whether a part in the file has no `at` (or two-lead, no `pins`), so bbgen has to place it.
pub fn needs_placing(data: &Val) -> bool {
    let parts = match data.at("parts").as_map() {
        Some(p) => p,
        None => return false,
    };
    if data.get("_placed").map(|v| !v.is_null()).unwrap_or(false) {
        return false;
    }
    parts.iter().any(|(_, spec)| {
        if spec.as_map().is_none() {
            return false;
        }
        let kind = if spec.at("type").truthy() { spec.at("type").py_str() } else if spec.at("symbol").truthy() || spec.at("footprint").truthy() {
            "kicad".into()
        } else { "None".into() };
        let k = Kind::parse(&kind);
        (k.map(|k| k.is_footprint()).unwrap_or(false) && !spec.at("at").truthy())
            || (k.map(|k| k.is_lead()).unwrap_or(false) && spec.at("pins").is_null())
    })
}

#[allow(dead_code)]
fn _rails_used() -> [Row; 4] {
    RAILS
}
