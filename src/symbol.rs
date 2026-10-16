//! Schematic symbols: what each part looks like in the circuit view, and where its pins attach.
//!
//! A part named from KiCad (`symbol:`) is drawn with its own symbol from KiCad's library, as KiCad draws it
//! (`kicad::drawing` reads it). bbgen's own parts get symbols made here in the same style, so they draw the same
//! without KiCad installed: two-lead parts like KiCad's Device:R, C, C_Polarized, D and LED, the Pico as a box
//! with the pins where KiCad's RaspberryPi_Pico has them, a DIP or SIP given only labels (or a part with only a
//! footprint) as a plain box, and an off-board part as a connector.
//!
//! Coordinates are millimetres with y down, on KiCad's 1.27 mm grid (KiCad's libraries have y up; kicad.rs flips
//! it). A pin's `at` is where a wire attaches, and the pin runs from there into the body along `dir`. A symbol is
//! drawn turned clockwise by 0, 90, 180 or 270 degrees, and its text always reads left to right or bottom to top.

use crate::kicad;
use crate::parts::{Kind, Part};
use std::path::PathBuf;

pub type Xy = (f64, f64);

/// Pixels per millimetre: the board's 20 px per 0.1 in, so a schematic and a board drawn side by side match.
pub const PX: f64 = 20.0 / 2.54;
const GRID: f64 = 2.54;
const TEXT: f64 = 1.27;
/// KiCad's default line, and its symbol outlines
const THIN: f64 = 0.1524;
const BODY: f64 = 0.254;
/// how far pin numbers sit off their pin's line
const MARGIN: f64 = 0.2;
/// average width of a character, in its size: as render.rs estimates labels
const CHAR: f64 = 0.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    /// filled with the line colour
    Outline,
    /// KiCad's body colour: a light tint here
    Background,
}

/// Text in a symbol: `dir` is the way it reads, `h` where `at` is along it (-1 start, 0 middle, 1 end) and `v`
/// where it is across it (-1 the top of the text, 0 its middle, 1 its baseline).
#[derive(Clone, Debug)]
pub struct Text {
    pub at: Xy,
    pub dir: (i32, i32),
    pub text: String,
    pub size: f64,
    pub h: i8,
    pub v: i8,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rect { a: Xy, b: Xy, width: f64, fill: Fill },
    Poly { pts: Vec<Xy>, width: f64, fill: Fill },
    Circle { c: Xy, r: f64, width: f64, fill: Fill },
    Arc { a: Xy, mid: Xy, b: Xy, width: f64 },
    Text(Text),
}

#[derive(Clone, Debug)]
pub struct SymPin {
    /// as the symbol numbers it (a KiCad symbol's pad number, else the part's pin number)
    pub number: String,
    pub name: String,
    /// KiCad's electrical type: input, output, bidirectional, tri_state, passive, power_in, power_out, ...
    pub kind: String,
    /// KiCad's graphic style: line, inverted, clock, inverted_clock, ...
    pub shape: String,
    pub at: Xy,
    pub dir: (i32, i32),
    pub len: f64,
    /// not drawn: a pin stacked on another (KiCad's extra GND pins), or one that's never connected
    pub hidden: bool,
    pub name_size: f64,
    pub number_size: f64,
    /// the part's pins (indices into `Part::pins`) that attach here
    pub pins: Vec<usize>,
}

/// One unit of a symbol: an op-amp of an LM358, or the whole of most parts.
#[derive(Clone, Debug)]
pub struct Unit {
    pub unit: i64,
    pub shapes: Vec<Shape>,
    pub pins: Vec<SymPin>,
}

/// Where the symbol puts its reference (U1) or value (its title).
#[derive(Clone, Debug)]
pub struct Field {
    pub at: Xy,
    pub dir: (i32, i32),
    pub h: i8,
    pub v: i8,
    pub size: f64,
    pub hidden: bool,
}

/// A symbol as drawn, before it's given to a part.
#[derive(Clone, Debug)]
pub struct Drawing {
    pub units: Vec<Unit>,
    /// how far pin names sit inside the body; 0 puts them above the pin's line instead
    pub name_offset: f64,
    pub names: bool,
    pub numbers: bool,
    pub reference: Option<Field>,
    pub value: Option<Field>,
}

/// A part's symbol: its drawing, with each pin knowing which of the part's pins it carries.
#[derive(Clone, Debug)]
pub struct PartSymbol {
    pub r#ref: String,
    pub value: String,
    /// where the drawing came from: a KiCad symbol, or bbgen's own
    pub source: String,
    pub drawing: Drawing,
    /// the part's pins no pin of the symbol carries (indices)
    pub unmapped: Vec<usize>,
    /// why a KiCad part fell back to a plain box
    pub note: Option<String>,
}

impl PartSymbol {
    /// U1, or U1A for the first unit of a symbol with several
    pub fn unit_ref(&self, u: usize) -> String {
        if self.drawing.units.len() > 1 {
            format!("{}{}", self.r#ref, unit_letter(self.drawing.units[u].unit))
        } else {
            self.r#ref.clone()
        }
    }
}

pub fn unit_letter(unit: i64) -> String {
    let mut n = unit.max(1);
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

// ---- a part's symbol ----

/// The symbol a part is drawn with in the circuit view.
pub fn of_part(part: &Part, library: Option<&[PathBuf]>) -> PartSymbol {
    // a resistor shows its value (10k), a chip its title
    let either = |a: &String, b: &String| if a.is_empty() { b.clone() } else { a.clone() };
    let value = if part.kind.is_lead() { either(&part.value, &part.title) } else { either(&part.title, &part.value) };
    let mut note = None;
    if !part.symbol.is_empty() {
        let roots = match library {
            Some(l) if !l.is_empty() => l.to_vec(),
            _ => kicad::library_roots(None),
        };
        match kicad::drawing(&part.symbol, &roots) {
            Ok(d) => {
                // on an adapter, adapter pin k carries the chip pad the adapter map puts there
                let pads = &part.adapter_pads;
                let number = |k: i64| match pads.is_empty() {
                    true => k.to_string(),
                    false => pads.iter().find(|(pin, _)| *pin == k).map(|(_, pad)| pad.to_string()).unwrap_or_default(),
                };
                return finish(part, (*d).clone(), part.symbol.clone(), value, None, number);
            }
            Err(e) => note = Some(format!("drawn as a box: {}", e)),
        }
    }
    let (drawing, source) = match part.kind {
        Kind::Resistor => (resistor(), "resistor"),
        Kind::Capacitor => (capacitor(), "capacitor"),
        Kind::Electrolytic => (electrolytic(), "electrolytic"),
        Kind::Diode => (diode(false), "diode"),
        Kind::Led => (diode(true), "led"),
        Kind::Pico => (pico(part), "pico"),
        Kind::Offboard => (connector(part), "connector"),
        Kind::Kicad if part.source.starts_with("Potentiometer") && part.pins.len() == 3 => (potentiometer(), "potentiometer"),
        _ => (plain_box(part), "box"),
    };
    // bbgen's own symbols number their pins as the part does
    finish(part, drawing, source.into(), value, note, |k| k.to_string())
}

/// The part's symbol, each of its pins given the part's pins whose number (as `number_of` writes it) it carries.
fn finish(part: &Part, mut drawing: Drawing, source: String, value: String, note: Option<String>,
          number_of: impl Fn(i64) -> String) -> PartSymbol {
    for (i, pin) in part.pins.iter().enumerate() {
        let n = number_of(pin.num);
        for sp in drawing.units.iter_mut().flat_map(|u| u.pins.iter_mut()).filter(|sp| sp.number == n) {
            if !sp.pins.contains(&i) {
                sp.pins.push(i);
            }
        }
    }
    let carried = |i: &usize| drawing.units.iter().any(|u| u.pins.iter().any(|p| p.pins.contains(i)));
    let unmapped = (0..part.pins.len()).filter(|i| !carried(i)).collect();
    PartSymbol { r#ref: part.r#ref.clone(), value, source, drawing, unmapped, note }
}

// ---- bbgen's own symbols ----

fn pin(number: &str, name: &str, kind: &str, at: Xy, dir: (i32, i32), len: f64) -> SymPin {
    SymPin { number: number.into(), name: name.into(), kind: kind.into(), shape: "line".into(), at, dir, len, hidden: false,
             name_size: TEXT, number_size: TEXT, pins: vec![] }
}

fn field(at: Xy, h: i8) -> Field {
    Field { at, dir: (1, 0), h, v: 0, size: TEXT, hidden: false }
}

/// A two-lead part standing up, pin 1 at the top: its pins, body, and its ref and value to the right of it.
fn two_lead(shapes: Vec<Shape>, len: f64, names: [&str; 2], kind: &str, text_x: f64) -> Drawing {
    let pins = vec![pin("1", names[0], kind, (0.0, -3.81), (0, 1), len), pin("2", names[1], kind, (0.0, 3.81), (0, -1), len)];
    Drawing { units: vec![Unit { unit: 1, shapes, pins }], name_offset: 0.0, names: false, numbers: false,
              reference: Some(field((text_x, -0.635), -1)), value: Some(field((text_x, 1.27), -1)) }
}

fn resistor() -> Drawing {
    let body = Shape::Rect { a: (-1.016, -2.54), b: (1.016, 2.54), width: BODY, fill: Fill::None };
    two_lead(vec![body], 1.27, ["~", "~"], "passive", 2.032)
}

fn plate(y: f64) -> Shape {
    Shape::Poly { pts: vec![(-2.032, y), (2.032, y)], width: 0.508, fill: Fill::None }
}

fn capacitor() -> Drawing {
    two_lead(vec![plate(-0.762), plate(0.762)], 3.048, ["~", "~"], "passive", 2.794)
}

fn electrolytic() -> Drawing {
    let shapes = vec![
        Shape::Rect { a: (-2.286, -1.016), b: (2.286, -0.508), width: BODY, fill: Fill::None },
        Shape::Rect { a: (-2.286, 0.508), b: (2.286, 1.016), width: BODY, fill: Fill::Outline },
        // the + by the positive plate
        Shape::Poly { pts: vec![(-1.778, -2.032), (-0.762, -2.032)], width: BODY, fill: Fill::None },
        Shape::Poly { pts: vec![(-1.27, -2.54), (-1.27, -1.524)], width: BODY, fill: Fill::None },
    ];
    two_lead(shapes, 2.794, ["+", "-"], "passive", 3.048)
}

/// A diode pointing down, anode on top; an LED adds its two arrows.
fn diode(led: bool) -> Drawing {
    let mut shapes = vec![
        Shape::Poly { pts: vec![(-1.27, -1.016), (1.27, -1.016), (0.0, 1.016), (-1.27, -1.016)], width: BODY, fill: Fill::None },
        Shape::Poly { pts: vec![(-1.27, 1.016), (1.27, 1.016)], width: BODY, fill: Fill::None },
    ];
    if led {
        for y in [-0.508, 0.762] {
            let (x0, y0, x1, y1) = (1.778, y, 2.794, y - 1.016);
            shapes.push(Shape::Poly { pts: vec![(x0, y0), (x1, y1)], width: THIN, fill: Fill::None });
            shapes.push(Shape::Poly { pts: vec![(x1, y1), (x1 - 0.635, y1 + 0.127), (x1 - 0.127, y1 + 0.635), (x1, y1)],
                                      width: THIN, fill: Fill::Outline });
        }
    }
    two_lead(shapes, 2.794, ["A", "K"], "passive", if led { 3.556 } else { 2.286 })
}

/// A potentiometer standing up like a resistor, its ends 1 and 3, its wiper (2) coming in from the right.
fn potentiometer() -> Drawing {
    let mut d = two_lead(vec![Shape::Rect { a: (-1.016, -2.54), b: (1.016, 2.54), width: BODY, fill: Fill::None }], 1.27,
                         ["~", "~"], "passive", -2.032);
    d.units[0].pins[1].number = "3".into();
    d.units[0].pins.push(pin("2", "~", "passive", (3.81, 0.0), (-1, 0), 1.27));
    d.units[0].shapes.push(Shape::Poly { pts: vec![(2.54, 0.0), (1.524, 0.0)], width: THIN, fill: Fill::None });
    d.units[0].shapes.push(Shape::Poly { pts: vec![(1.016, 0.0), (1.778, -0.381), (1.778, 0.381), (1.016, 0.0)], width: THIN,
                                         fill: Fill::Outline });
    for f in [&mut d.reference, &mut d.value] {
        f.as_mut().unwrap().h = 1;
    }
    d
}

/// A pin of a box: on a side, carrying some of the part's pins. A slot of None leaves a gap.
struct BoxPin {
    number: String,
    name: String,
    kind: String,
    pins: Vec<usize>,
}

fn name_len(name: &str, size: f64) -> f64 {
    plain(name).chars().count() as f64 * size * CHAR
}

fn up_to(x: f64, step: f64) -> f64 {
    (x / step - 1e-9).ceil() * step
}

/// A rectangle with pins down its left and right sides (top to bottom), along its top and its bottom (left to
/// right), sized to fit their names inside.
fn boxed(left: Vec<Option<BoxPin>>, right: Vec<Option<BoxPin>>, top: Vec<BoxPin>, bottom: Vec<BoxPin>, len: f64,
         names: bool, refs: (&str, &str)) -> Drawing {
    let offset = 0.508;
    let longest = |side: &[Option<BoxPin>]| side.iter().flatten().map(|p| name_len(&p.name, TEXT)).fold(0.0, f64::max);
    let tallest = |side: &[BoxPin]| side.iter().map(|p| name_len(&p.name, TEXT)).fold(0.0, f64::max);
    let room = |l: f64| if names && l > 0.0 { l + offset + 0.5 } else { 0.0 };
    let top_room = if top.is_empty() { 0.0 } else { room(tallest(&top)) };
    let bottom_room = if bottom.is_empty() { 0.0 } else { room(tallest(&bottom)) };
    let rows = left.len().max(right.len()).max(1);
    // the first side pin a grid step below the top's names, the body a grid step past the last
    let first = up_to(top_room + GRID, GRID);
    let height = up_to(first + (rows - 1) as f64 * GRID + GRID + bottom_room, GRID);
    let sides = [&left, &right].iter().filter(|s| s.iter().any(Option::is_some)).count() as f64;
    let across = if names { longest(&left) + longest(&right) + sides * offset + sides.max(1.0) * GRID } else { 2.0 * GRID };
    let width = up_to(across.max((top.len().max(bottom.len()) + 1) as f64 * GRID), 2.0 * GRID);
    let (x0, x1) = (-width / 2.0, width / 2.0);
    let mut pins = Vec::new();
    let mut add = |p: BoxPin, at: Xy, dir: (i32, i32)| {
        let mut sp = pin(&p.number, &p.name, &p.kind, at, dir, len);
        sp.pins = p.pins;
        pins.push(sp);
    };
    for (side, x, dir) in [(left, x0 - len, (1, 0)), (right, x1 + len, (-1, 0))] {
        for (k, p) in side.into_iter().enumerate() {
            if let Some(p) = p {
                add(p, (x, first + k as f64 * GRID), dir);
            }
        }
    }
    for (side, y, dir) in [(top, -len, (0, 1)), (bottom, height + len, (0, -1))] {
        let n = side.len();
        for (k, p) in side.into_iter().enumerate() {
            let x = ((k as f64 - (n - 1) as f64 / 2.0) * GRID / 1.27).round() * 1.27;
            add(p, (x, y), dir);
        }
    }
    let body = Shape::Rect { a: (x0, 0.0), b: (x1, height), width: BODY, fill: Fill::Background };
    // the reference over the body's left corner and the value under it, or past the pins there when they'd reach
    // them (a pin's number is on its left)
    let clear = |y: f64, v: i8, text: &str| {
        let xs: Vec<f64> = pins.iter().filter(|p| p.dir.0 == 0 && (p.at.1 < 0.0) == (y < 0.0)).map(|p| p.at.0).collect();
        let fits = xs.iter().all(|&x| x0 + name_len(text, TEXT) < x - TEXT);
        let x = if fits { x0 } else { xs.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)) + 0.762 };
        Field { at: (x, y), dir: (1, 0), h: -1, v, size: TEXT, hidden: false }
    };
    let (reference, value) = (clear(-0.508, 1, refs.0), clear(height + 0.508, -1, refs.1));
    Drawing { units: vec![Unit { unit: 1, shapes: vec![body], pins }], name_offset: offset, names, numbers: true,
              reference: Some(reference), value: Some(value) }
}

/// A supply's name: what goes along a box's top.
fn supply(name: &str) -> bool {
    let u = name.to_uppercase();
    ["VCC", "VDD", "VBUS", "VSYS", "VIN", "V+", "VS", "VBAT", "3V3", "5V", "+5V", "+3V3", "12V", "+12V", "AVCC"].contains(&u.as_str())
}

/// A ground's name: what goes along a box's bottom.
fn ground(name: &str) -> bool {
    let u = name.to_uppercase();
    u.starts_with("GND") || u.starts_with("VSS") || u == "V-" || u == "0V" || u == "AGND" || u == "DGND" || u == "PGND"
}

/// The part's pins as box pins: pins joined on the part (its buses) as one.
fn box_pins(part: &Part) -> Vec<(usize, BoxPin)> {
    let mut out: Vec<(usize, BoxPin)> = Vec::new();
    for (i, p) in part.pins.iter().enumerate() {
        if let Some(bus) = part.bus_of(i) {
            if bus[0] != i {
                continue;
            }
        }
        let pins = part.bus_of(i).cloned().unwrap_or(vec![i]);
        out.push((i, BoxPin { number: p.num.to_string(), name: p.name.clone(), kind: String::new(), pins }));
    }
    out
}

/// A DIP, SIP, or a part with only a footprint: supplies along the top, grounds along the bottom, and the rest
/// on the side the pinout has them: a DIP's first half down the left and the second up the right, so pin n sits
/// opposite pin 1 as in a datasheet.
fn plain_box(part: &Part) -> Drawing {
    let n = part.pins.len();
    let named = part.pins.iter().any(|p| p.name != p.num.to_string());
    let (mut left, mut right, mut top, mut bottom) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut rest: Vec<(usize, BoxPin)> = Vec::new();
    for (i, p) in box_pins(part) {
        if named && supply(&p.name) {
            top.push(p);
        } else if named && ground(&p.name) {
            bottom.push(p);
        } else {
            rest.push((i, p));
        }
    }
    let two_rows = part.kind == Kind::Dip || (part.kind == Kind::Kicad && n > 3);
    for (i, p) in rest {
        if two_rows && i >= n.div_ceil(2) {
            right.push(Some(p));
        } else {
            left.push(Some(p));
        }
    }
    right.reverse();
    boxed(left, right, top, bottom, GRID, named, (&part.r#ref, &part.title))
}

/// The Pico, its pins where KiCad's MCU_Module:RaspberryPi_Pico has them: GP0-15 down the left under RUN and
/// 3V3_EN, GP16-28 down the right with ADC_VREF and AGND, supplies on top, the grounds (joined) below.
fn pico(part: &Part) -> Drawing {
    let pins = box_pins(part);
    let get = |name: &str| pins.iter().find(|(_, p)| p.name == name).map(|(_, p)| BoxPin {
        number: p.number.clone(), name: p.name.clone(), kind: p.kind.clone(), pins: p.pins.clone() });
    let side = |names: &[&str]| names.iter().map(|n| if n.is_empty() { None } else { get(n) }).collect::<Vec<_>>();
    let left = side(&["RUN", "3V3_EN", "", "GP0", "GP1", "GP2", "GP3", "GP4", "GP5", "GP6", "GP7", "GP8", "GP9", "GP10",
                      "GP11", "GP12", "GP13", "GP14", "GP15"]);
    let right = side(&["", "", "", "", "GP16", "GP17", "GP18", "GP19", "GP20", "GP21", "GP22", "", "ADC_VREF", "",
                       "GP26", "GP27", "GP28", "", "AGND"]);
    let top: Vec<BoxPin> = ["VSYS", "VBUS", "3V3"].iter().filter_map(|n| get(n)).collect();
    let bottom: Vec<BoxPin> = get("GND").into_iter().collect();
    boxed(left, right, top, bottom, 3.81, true, (&part.r#ref, &part.title))
}

/// An off-board part, like KiCad's Conn_01xNN: its pins down the right, facing the circuit; with `cols: 2`, odd
/// pins on the left and even on the right (numbered along rows), or the first half left (along columns).
fn connector(part: &Part) -> Drawing {
    let pins: Vec<BoxPin> = box_pins(part).into_iter().map(|(_, p)| p).collect();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    if part.cols >= 2 {
        let half = pins.len().div_ceil(2);
        for (k, p) in pins.into_iter().enumerate() {
            let to_left = if part.numbering == "column" { k < half } else { k % 2 == 0 };
            if to_left { left.push(Some(p)) } else { right.push(Some(p)) }
        }
    } else {
        right = pins.into_iter().map(Some).collect();
    }
    let named = part.pins.iter().any(|p| p.name != p.num.to_string());
    boxed(left, right, vec![], vec![], GRID, named, (&part.r#ref, &part.title))
}

// ---- drawing ----

/// A symbol turned clockwise by `rot` degrees (0, 90, 180, 270) about its origin, mirrored left to right first
/// when `mirror`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Orient {
    pub rot: i32,
    pub mirror: bool,
}

impl Orient {
    pub fn point(self, (mut x, mut y): Xy) -> Xy {
        if self.mirror {
            x = -x;
        }
        for _ in 0..self.rot.div_euclid(90).rem_euclid(4) {
            (x, y) = (-y, x);
        }
        (x, y)
    }

    pub fn dir(self, (x, y): (i32, i32)) -> (i32, i32) {
        let (a, b) = self.point((x as f64, y as f64));
        (a.round() as i32, b.round() as i32)
    }
}

/// What a symbol is drawn with, in millimetres on the page: the input to both the SVG and its bounds.
#[derive(Clone, Debug)]
pub enum Prim {
    Line { pts: Vec<Xy>, width: f64, fill: Fill },
    Circle { c: Xy, r: f64, width: f64, fill: Fill },
    Arc { a: Xy, mid: Xy, b: Xy, width: f64 },
    /// reading left to right, or bottom to top when `up`
    Text { at: Xy, up: bool, text: String, size: f64, anchor: &'static str },
    Begin(String),
    End,
}

fn add(a: Xy, b: Xy) -> Xy {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(d: (i32, i32), k: f64) -> Xy {
    (d.0 as f64 * k, d.1 as f64 * k)
}

/// Text turned with the symbol, then turned round if it would read right to left or downwards.
fn text_prim(t: &Text, o: Orient, at: Xy) -> Prim {
    let (mut h, mut v) = (t.h, t.v);
    if o.mirror {
        // mirrored, text still reads the same way, from the other side of its point
        if t.dir.0 != 0 { h = -h } else { v = -v }
    }
    let mut d = Orient { rot: o.rot, mirror: false }.dir(t.dir);
    if d == (-1, 0) || d == (0, 1) {
        d = (-d.0, -d.1);
        h = -h;
        v = -v;
    }
    // the baseline's offset across the text, along its "down" (d turned clockwise)
    let down = (-d.1, d.0);
    let k = match v {
        -1 => 0.72,
        0 => 0.36,
        _ => 0.0,
    } * t.size;
    let at = add(at, scale(down, k));
    let anchor = match h {
        -1 => "start",
        1 => "end",
        _ => "middle",
    };
    Prim::Text { at, up: d == (0, -1), text: t.text.clone(), size: t.size, anchor }
}

/// The text along a pin: its name and number, where KiCad puts them.
fn pin_texts(p: &SymPin, d: &Drawing) -> Vec<Text> {
    let mut out = Vec::new();
    // the way text along this pin reads, before turning: across for a horizontal pin, upwards for a vertical one
    let along = if p.dir.1 == 0 { (1, 0) } else { (0, -1) };
    let above = (along.1, -along.0); // left of the text's direction
    let inner = add(p.at, scale(p.dir, p.len));
    let mid = add(p.at, scale(p.dir, p.len / 2.0));
    let name = if p.name == "~" { "" } else { p.name.as_str() };
    let show_name = d.names && !name.is_empty();
    if show_name && d.name_offset > 0.0 {
        let h = if p.dir == along { -1 } else { 1 };
        out.push(Text { at: add(inner, scale(p.dir, d.name_offset)), dir: along, text: name.into(), size: p.name_size, h, v: 0 });
    } else if show_name {
        out.push(Text { at: add(mid, scale(above, MARGIN)), dir: along, text: name.into(), size: p.name_size, h: 0, v: 1 });
    }
    if d.numbers && !p.number.is_empty() {
        let (side, v) = if show_name && d.name_offset == 0.0 { (-MARGIN, -1) } else { (MARGIN, 1) };
        out.push(Text { at: add(mid, scale(above, side)), dir: along, text: p.number.clone(), size: p.number_size, h: 0, v });
    }
    out
}

/// One unit of a part's symbol, turned by `o` and with its origin at `origin` (mm).
pub fn prims(sym: &PartSymbol, u: usize, o: Orient, origin: Xy) -> Vec<Prim> {
    let d = &sym.drawing;
    let unit = &d.units[u];
    let at = |p: Xy| add(origin, o.point(p));
    let mut out = Vec::new();
    for s in &unit.shapes {
        out.push(match s {
            Shape::Rect { a, b, width, fill } => {
                let pts = vec![at(*a), at((b.0, a.1)), at(*b), at((a.0, b.1)), at(*a)];
                Prim::Line { pts, width: *width, fill: *fill }
            }
            Shape::Poly { pts, width, fill } => Prim::Line { pts: pts.iter().map(|p| at(*p)).collect(), width: *width, fill: *fill },
            Shape::Circle { c, r, width, fill } => Prim::Circle { c: at(*c), r: *r, width: *width, fill: *fill },
            Shape::Arc { a, mid, b, width } => Prim::Arc { a: at(*a), mid: at(*mid), b: at(*b), width: *width },
            Shape::Text(t) => text_prim(t, o, at(t.at)),
        });
    }
    for p in unit.pins.iter().filter(|p| !p.hidden) {
        out.push(Prim::Begin(format!("data-pin=\"{}.{}\"", escape(&sym.r#ref), escape(&p.number))));
        let deco = p.number_size.max(0.5) / 2.0;
        let inner = add(p.at, scale(p.dir, p.len));
        let bubble = p.shape.contains("inverted");
        let end = if bubble { add(p.at, scale(p.dir, p.len - 2.0 * deco)) } else { inner };
        out.push(Prim::Line { pts: vec![at(p.at), at(end)], width: THIN, fill: Fill::None });
        if bubble {
            out.push(Prim::Circle { c: at(add(inner, scale(p.dir, -deco))), r: deco, width: THIN, fill: Fill::None });
        }
        if p.shape.contains("clock") {
            let across = (p.dir.1 as f64 * deco, -p.dir.0 as f64 * deco);
            let pts = vec![at(add(inner, across)), at(add(inner, scale(p.dir, deco))), at(add(inner, (-across.0, -across.1)))];
            out.push(Prim::Line { pts, width: THIN, fill: Fill::None });
        }
        for t in pin_texts(p, d) {
            out.push(text_prim(&t, o, at(t.at)));
        }
        out.push(Prim::End);
    }
    for (f, text) in [(&d.reference, sym.unit_ref(u)), (&d.value, sym.value.clone())] {
        if let Some(f) = f {
            if !f.hidden && !text.is_empty() {
                let t = Text { at: f.at, dir: f.dir, text, size: f.size, h: f.h, v: f.v };
                out.push(text_prim(&t, o, at(f.at)));
            }
        }
    }
    out
}

/// What the prims cover, text included as estimated: (x0, y0, x1, y1) in mm.
pub fn bounds(prims: &[Prim]) -> (f64, f64, f64, f64) {
    let mut b = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut take = |x: f64, y: f64| {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    };
    for p in prims {
        match p {
            Prim::Line { pts, .. } => pts.iter().for_each(|p| take(p.0, p.1)),
            Prim::Circle { c, r, .. } => {
                take(c.0 - r, c.1 - r);
                take(c.0 + r, c.1 + r);
            }
            Prim::Arc { a, mid, b, .. } => [a, mid, b].iter().for_each(|p| take(p.0, p.1)),
            Prim::Text { at, up, text, size, anchor } => {
                let w = name_len(text, *size);
                let lo = match *anchor {
                    "start" => 0.0,
                    "end" => -w,
                    _ => -w / 2.0,
                };
                if *up {
                    take(at.0 - size * 0.8, at.1 - lo);
                    take(at.0, at.1 - lo - w);
                } else {
                    take(at.0 + lo, at.1 - size * 0.8);
                    take(at.0 + lo + w, at.1 + size * 0.2);
                }
            }
            Prim::Begin(_) | Prim::End => {}
        }
    }
    b
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A number for SVG: two decimals at most, no trailing zeros.
fn n(x: f64) -> String {
    let s = format!("{:.2}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// A name without KiCad's markup, as it reads: ~{RST} is RST (with a bar over it when drawn), V_{IN} is VIN.
fn plain(s: &str) -> String {
    kicad::clean_name(s).replace('/', "")
}

/// KiCad's markup as SVG: ~{X} with a bar over it, X_{n} and X^{n} lowered and raised.
fn markup(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if "~_^".contains(c) && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(len) = chars[i + 2..].iter().position(|&x| x == '}') {
                let inner: String = chars[i + 2..i + 2 + len].iter().collect();
                let style = match c {
                    '~' => "text-decoration=\"overline\"",
                    '_' => "baseline-shift=\"sub\" font-size=\"75%\"",
                    _ => "baseline-shift=\"super\" font-size=\"75%\"",
                };
                out.push_str(&format!("<tspan {}>{}</tspan>", style, escape(&inner)));
                i += len + 3;
                continue;
            }
        }
        out.push_str(&escape(&c.to_string()));
        i += 1;
    }
    out
}

fn fill_attr(fill: Fill) -> &'static str {
    match fill {
        Fill::None => "fill=\"none\"",
        Fill::Outline => "fill=\"currentColor\"",
        Fill::Background => "fill=\"currentColor\" fill-opacity=\"0.07\"",
    }
}

/// The prims as SVG elements, in pixels, in the drawing's colour (`currentColor`).
pub fn svg(prims: &[Prim]) -> String {
    let px = |p: Xy| format!("{},{}", n(p.0 * PX), n(p.1 * PX));
    let w = |width: f64| n(width.max(THIN) * PX);
    let mut out = String::new();
    for p in prims {
        out.push_str(&match p {
            Prim::Line { pts, width, fill } => {
                let closed = pts.len() > 2 && pts[0] == pts[pts.len() - 1];
                let tag = if closed { "polygon" } else { "polyline" };
                let pts = if closed { &pts[..pts.len() - 1] } else { &pts[..] };
                format!("<{} points=\"{}\" {} stroke=\"currentColor\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                        tag, pts.iter().map(|p| px(*p)).collect::<Vec<_>>().join(" "), fill_attr(*fill), w(*width))
            }
            Prim::Circle { c, r, width, fill } => format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" {} stroke=\"currentColor\" stroke-width=\"{}\"/>",
                                                          n(c.0 * PX), n(c.1 * PX), n(r * PX), fill_attr(*fill), w(*width)),
            Prim::Arc { a, mid, b, width } => format!("<path d=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"{}\"/>",
                                                      arc_d(*a, *mid, *b), w(*width)),
            Prim::Text { at, up, text, size, anchor } => {
                let (x, y) = (n(at.0 * PX), n(at.1 * PX));
                let turn = if *up { format!(" transform=\"rotate(-90 {} {})\"", x, y) } else { String::new() };
                format!("<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"{}\" fill=\"currentColor\"{}>{}</text>",
                        x, y, n(size * PX), anchor, turn, markup(text))
            }
            Prim::Begin(attrs) => format!("<g {}>", attrs),
            Prim::End => "</g>".into(),
        });
    }
    out
}

/// An SVG path for the arc from a through mid to b.
fn arc_d(a: Xy, mid: Xy, b: Xy) -> String {
    let (ax, ay, bx, by, cx, cy) = (a.0 * PX, a.1 * PX, mid.0 * PX, mid.1 * PX, b.0 * PX, b.1 * PX);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-9 {
        return format!("M{},{} L{},{}", n(ax), n(ay), n(cx), n(cy));
    }
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();
    let turn = |x: f64, y: f64| (y - uy).atan2(x - ux);
    let tau = std::f64::consts::TAU;
    let norm = |t: f64| t.rem_euclid(tau);
    let (ts, tm, te) = (turn(ax, ay), turn(bx, by), turn(cx, cy));
    let (dm, de) = (norm(tm - ts), norm(te - ts));
    let (sweep, span) = if dm < de { (1, de) } else { (0, tau - de) };
    let large = if span > std::f64::consts::PI { 1 } else { 0 };
    format!("M{},{} A{},{} 0 {} {} {},{}", n(ax), n(ay), n(r), n(r), large, sweep, n(cx), n(cy))
}
