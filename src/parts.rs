//! Parts, their pins, and where those pins sit relative to pin 1.
//!
//! Footprint parts (pico, dip, sip, KiCad footprints) are placed by the hole of pin 1 plus a rotation.
//! Two-lead parts (resistor, capacitor, ...) name a hole, rail or part pin for each leg. Off-board parts are
//! connectors elsewhere, drawn beside the breadboard and reached with flying leads.
//!
//! Offsets are in grid units: x along the columns, y down the rows. Unrotated, a DIP has pin 1 bottom-left
//! with its notch to the left, and a Pico has its USB to the left with pins 1-20 along the bottom.

use crate::board::{Hole, Xy};
use crate::kicad::{self, MM};
use crate::val::Val;
use std::path::PathBuf;

const PICO_PINS: [&str; 40] = [
    "GP0", "GP1", "GND", "GP2", "GP3", "GP4", "GP5", "GND", "GP6", "GP7", "GP8", "GP9", "GND", "GP10", "GP11",
    "GP12", "GP13", "GND", "GP14", "GP15", "GP16", "GP17", "GND", "GP18", "GP19", "GP20", "GP21", "GND", "GP22",
    "RUN", "GP26", "GP27", "AGND", "GP28", "ADC_VREF", "3V3", "3V3_EN", "GND", "VSYS", "VBUS",
];
const PICO_GND: [usize; 7] = [3, 8, 13, 18, 23, 28, 38];
const PICO_USB_KEEPOUT: Rect = (-9.08, -5.76, -1.41, -1.24);
pub const LED_COLORS: [&str; 6] = ["red", "green", "yellow", "blue", "white", "orange"];

pub type Rect = (f64, f64, f64, f64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pico,
    Dip,
    Sip,
    Kicad,
    Resistor,
    Capacitor,
    Electrolytic,
    Diode,
    Led,
    Offboard,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "pico" => Kind::Pico,
            "dip" => Kind::Dip,
            "sip" => Kind::Sip,
            "kicad" => Kind::Kicad,
            "resistor" => Kind::Resistor,
            "capacitor" => Kind::Capacitor,
            "electrolytic" => Kind::Electrolytic,
            "diode" => Kind::Diode,
            "led" => Kind::Led,
            "offboard" => Kind::Offboard,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Pico => "pico",
            Kind::Dip => "dip",
            Kind::Sip => "sip",
            Kind::Kicad => "kicad",
            Kind::Resistor => "resistor",
            Kind::Capacitor => "capacitor",
            Kind::Electrolytic => "electrolytic",
            Kind::Diode => "diode",
            Kind::Led => "led",
            Kind::Offboard => "offboard",
        }
    }

    pub fn is_footprint(self) -> bool {
        matches!(self, Kind::Pico | Kind::Dip | Kind::Sip | Kind::Kicad)
    }

    pub fn is_lead(self) -> bool {
        matches!(self, Kind::Resistor | Kind::Capacitor | Kind::Electrolytic | Kind::Diode | Kind::Led)
    }

    fn lead_names(self) -> [&'static str; 2] {
        match self {
            Kind::Electrolytic => ["+", "-"],
            Kind::Diode | Kind::Led => ["A", "K"],
            _ => ["1", "2"],
        }
    }

    /// Standing up, about two holes across (`ROUND_BODY`): half its width in holes.
    pub fn round_body(self) -> Option<f64> {
        matches!(self, Kind::Led | Kind::Electrolytic).then_some(0.9)
    }
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub num: i64,
    pub name: String,
    pub offset: Xy,
    pub label_dir: (i32, i32),
    pub hole: Option<Hole>,
    pub xy: Option<Xy>,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub r#ref: String,
    pub kind: Kind,
    pub title: String,
    pub value: String,
    pub color: Option<String>,
    pub pins: Vec<Pin>,
    /// pins joined inside the part, by index
    pub buses: Vec<Vec<usize>>,
    pub at: Option<String>,
    pub rot: i32,
    pub width: i64,
    pub legs: Vec<String>,
    pub slant: Option<String>,
    pub near: Option<String>,
    pub raised: bool,
    pub auto: bool,
    pub keepouts: Vec<(String, Rect)>,
    pub body: Option<Rect>,
    pub source: String,
    pub adapter: String,
    /// the KiCad symbol it's named by (`symbol:`), for the circuit view
    pub symbol: String,
    /// on an adapter: (adapter pin, the chip pad on it), every pad the symbol numbers
    pub adapter_pads: Vec<(i64, i64)>,
    pub cols: i64,
    pub numbering: String,
    pub place: String,
    pub r#box: Option<Rect>,
    pub extent: Option<Rect>,
}

impl Part {
    fn new(r: &str, kind: Kind, title: String, value: String, color: Option<String>) -> Part {
        Part {
            r#ref: r.to_string(), kind, title, value, color, pins: Vec::new(), buses: Vec::new(), at: None, rot: 0,
            width: 0, legs: Vec::new(), slant: None, near: None, raised: false, auto: false, keepouts: Vec::new(),
            body: None, source: String::new(), adapter: String::new(), symbol: String::new(), adapter_pads: Vec::new(),
            cols: 1, numbering: "row".into(), place: "right".into(), r#box: None, extent: None,
        }
    }

    /// Pins a token names: a number, else a name (exactly, then ignoring case).
    pub fn find(&self, token: &str) -> Vec<usize> {
        let token = token.trim();
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
            let n: i64 = token.parse().unwrap_or(-1);
            return (0..self.pins.len()).filter(|&i| self.pins[i].num == n).collect();
        }
        let exact: Vec<usize> = (0..self.pins.len()).filter(|&i| self.pins[i].name == token).collect();
        if !exact.is_empty() {
            return exact;
        }
        let low = token.to_lowercase();
        (0..self.pins.len()).filter(|&i| self.pins[i].name.to_lowercase() == low).collect()
    }

    pub fn bus_of(&self, pin: usize) -> Option<&Vec<usize>> {
        self.buses.iter().find(|b| b.contains(&pin))
    }

    /// `str(pin)`: U1.3, or U1.3 VCC when the pin has a name of its own.
    pub fn pin_str(&self, i: usize) -> String {
        let p = &self.pins[i];
        if p.name == p.num.to_string() {
            format!("{}.{}", self.r#ref, p.num)
        } else {
            format!("{}.{} {}", self.r#ref, p.num, p.name)
        }
    }
}

/// Rotate an offset clockwise on screen by rot degrees (a multiple of 90).
pub fn rotate(v: Xy, rot: i32) -> Xy {
    let (mut x, mut y) = v;
    for _ in 0..(rot.div_euclid(90).rem_euclid(4)) {
        let t = x;
        x = -y;
        y = t;
    }
    (x, y)
}

/// The part's outline relative to pin 1, unrotated.
pub fn body_rect(part: &Part) -> Option<Rect> {
    let n = part.pins.len() as i64;
    match part.kind {
        Kind::Pico => Some((-0.54, -7.64, 19.54, 0.64)),
        Kind::Dip => Some((-0.5, -(part.width as f64) - 0.5, (n / 2) as f64 - 0.5, 0.5)),
        Kind::Sip => Some((-0.45, -0.45, n as f64 - 0.55, 0.45)),
        _ => part.body,
    }
}

/// A rect given relative to pin 1 in the part's own frame, rotated and placed.
pub fn placed_box(part: &Part, rect: Rect) -> Rect {
    let (ax, ay) = part.pins[0].xy.unwrap();
    let mut xs = Vec::with_capacity(4);
    let mut ys = Vec::with_capacity(4);
    for x in [rect.0, rect.2] {
        for y in [rect.1, rect.3] {
            let (cx, cy) = rotate((x, y), part.rot);
            xs.push(ax + cx);
            ys.push(ay + cy);
        }
    }
    (min(&xs), min(&ys), max(&xs), max(&ys))
}

pub fn min(v: &[f64]) -> f64 {
    let mut m = v[0];
    for &x in &v[1..] {
        if x < m {
            m = x;
        }
    }
    m
}

pub fn max(v: &[f64]) -> f64 {
    let mut m = v[0];
    for &x in &v[1..] {
        if x > m {
            m = x;
        }
    }
    m
}

fn count(spec: &Val, r: &str) -> Result<i64, String> {
    let mut n = spec.get("pins").cloned().unwrap_or(Val::Null);
    if n.is_null() && spec.at("type").as_str() == Some("offboard") {
        n = match spec.at("labels") {
            Val::List(l) => Val::Int(l.len() as i64),
            Val::Map(m) => Val::Int(m.iter().filter_map(|(k, _)| k.py_int()).max().unwrap_or(0)),
            _ => Val::Int(0),
        };
    }
    match n {
        Val::Int(i) if i >= 1 => Ok(i),
        _ => Err(format!("parts.{}: needs pins: <count>", r)),
    }
}

fn labels(spec: &Val, n: i64, r: &str) -> Result<Vec<String>, String> {
    let raw = spec.at("labels");
    let mut names: Vec<Option<Val>> = vec![None; n as usize + 1];
    match raw {
        Val::Null => {}
        Val::List(l) if l.is_empty() => {}
        Val::Map(m) if m.is_empty() => {}
        Val::List(l) => {
            if l.len() as i64 != n {
                return Err(format!("parts.{}: {} labels for {} pins", r, l.len(), n));
            }
            for (i, v) in l.iter().enumerate() {
                names[i + 1] = Some(v.clone());
            }
        }
        Val::Map(m) => {
            for (k, v) in m {
                let k = k.py_int().ok_or(format!("parts.{}: labels must be a list or a {{pin: name}} mapping", r))?;
                if !(1..=n).contains(&k) {
                    return Err(format!("parts.{}: label for a pin outside 1..{}", r, n));
                }
                names[k as usize] = Some(v.clone());
            }
        }
        _ => return Err(format!("parts.{}: labels must be a list or a {{pin: name}} mapping", r)),
    }
    Ok((1..=n).map(|k| match &names[k as usize] {
        Some(v) if !v.is_null() && v.as_str() != Some("") => v.py_str(),
        _ => k.to_string(),
    }).collect())
}

/// The reason given for a slanted wire or part (`slant: <reason>`), which silences DIAGONAL.
pub fn slant_reason(spec: &Val, where_: &str) -> Result<Option<String>, String> {
    match spec.get("slant") {
        None | Some(Val::Null) => Ok(None),
        Some(Val::Str(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        _ => Err(format!("{}: slant takes the reason it can't be straight, like slant: crosses the trench", where_)),
    }
}

pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A part from its spec; `symbol:` or `footprint:` take it from KiCad's library.
pub fn make_part(r: &str, spec: &Val, library: Option<&[PathBuf]>) -> Result<Part, String> {
    if spec.as_map().is_none() {
        return Err(format!("parts.{}: expected a mapping", r));
    }
    let from_kicad = spec.at("symbol").truthy() || spec.at("footprint").truthy();
    let kind_name = if spec.at("type").truthy() {
        spec.at("type").py_str()
    } else if from_kicad && spec.at("adapter").truthy() {
        "dip".into()
    } else if from_kicad {
        "kicad".into()
    } else {
        "None".into()
    };
    let kind = Kind::parse(&kind_name).ok_or_else(|| {
        format!("parts.{}: unknown type {} (expected one of pico, dip, sip, kicad, resistor, capacitor, electrolytic, \
                 diode, led, offboard, or a KiCad symbol: Library:Name)",
                r, if spec.at("type").truthy() { spec.at("type").py_repr() } else { "None".into() })
    })?;
    let opt_str = |k: &str| if spec.at(k).truthy() { spec.at(k).py_str() } else { String::new() };
    let color = spec.get("color").filter(|v| !v.is_null()).map(Val::py_str);
    let mut part = Part::new(r, kind, opt_str("title"), opt_str("value"), color);
    if kind == Kind::Led {
        if let Some(c) = &part.color {
            let name = c.to_lowercase();
            if !LED_COLORS.contains(&name.as_str()) && !is_hex_color(&name) {
                return Err(format!("parts.{}: an LED's color is red, green, yellow, blue, white, orange or #rrggbb", r));
            }
            part.color = Some(name);
        }
    } else if part.color.as_deref().is_some_and(|c| !c.is_empty() && !crate::circuit::is_color(c)) {
        return Err(format!("parts.{}: unknown color {}", r, spec.at("color").py_repr()));
    }
    part.near = if spec.at("near").truthy() { Some(spec.at("near").py_str()) } else { None };
    if kind.is_footprint() {
        part.at = spec.get("at").filter(|v| !v.is_null()).map(Val::py_str);
        part.rot = spec.get("rot").map(|v| v.py_int().unwrap_or(-1)).unwrap_or(0) as i32;
        part.raised = spec.at("raised").truthy();
        if ![0, 90, 180, 270].contains(&part.rot) {
            return Err(format!("parts.{}: rot must be 0, 90, 180 or 270", r));
        }
    }
    match kind {
        Kind::Pico => {
            if part.title.is_empty() {
                part.title = "Raspberry Pi Pico".into();
            }
            for (i, name) in PICO_PINS.iter().enumerate() {
                let n = i as i64 + 1;
                let bottom = n <= 20;
                part.pins.push(Pin {
                    num: n, name: name.to_string(),
                    offset: if bottom { ((n - 1) as f64, 0.0) } else { ((40 - n) as f64, -7.0) },
                    label_dir: if bottom { (0, -1) } else { (0, 1) }, hole: None, xy: None,
                });
            }
            part.buses.push(PICO_GND.iter().map(|&n| n - 1).collect());
            part.keepouts.push(("USB cable".into(), PICO_USB_KEEPOUT));
        }
        Kind::Kicad => from_kicad_part(&mut part, spec, library)?,
        Kind::Dip => {
            let (n, names) = if from_kicad {
                adapter(&mut part, spec, library)?
            } else {
                let n = count(spec, r)?;
                (n, labels(spec, n, r)?)
            };
            if n % 2 != 0 {
                return Err(format!("parts.{}: a dip needs an even pin count", r));
            }
            let adapter_width = match spec.at("adapter") {
                Val::Map(_) => spec.at("adapter").get("width").cloned().unwrap_or(Val::Int(3)),
                _ => Val::Int(3),
            };
            part.width = spec.get("width").cloned().unwrap_or(adapter_width).py_int().unwrap_or(3);
            for k in 1..=n {
                let (offset, dir) = if k <= n / 2 {
                    (((k - 1) as f64, 0.0), (0, -1))
                } else {
                    (((n - k) as f64, -(part.width as f64)), (0, 1))
                };
                part.pins.push(Pin { num: k, name: names[(k - 1) as usize].clone(), offset, label_dir: dir, hole: None, xy: None });
            }
        }
        Kind::Sip => {
            let n = count(spec, r)?;
            let names = labels(spec, n, r)?;
            for k in 1..=n {
                part.pins.push(Pin { num: k, name: names[(k - 1) as usize].clone(), offset: ((k - 1) as f64, 0.0),
                                     label_dir: (0, -1), hole: None, xy: None });
            }
        }
        k if k.is_lead() => {
            let legs = spec.at("pins");
            match legs {
                Val::Null => {}
                Val::List(l) if l.len() == 2 => {}
                _ => return Err(format!("parts.{}: pins is [<leg 1>, <leg 2>] (holes, rails or part pins), or leave it \
                                         out and bbgen places the legs from the nets", r)),
            }
            part.legs = legs.as_list().map(|l| l.iter().map(Val::py_str).collect()).unwrap_or_default();
            part.slant = slant_reason(spec, &format!("parts.{}", r))?;
            let names: Vec<String> = match spec.at("labels") {
                v if v.truthy() => v.as_list().map(|l| l.iter().map(Val::py_str).collect()).unwrap_or_default(),
                _ => k.lead_names().iter().map(|s| s.to_string()).collect(),
            };
            for i in 0..2 {
                part.pins.push(Pin { num: i as i64 + 1, name: names[i].clone(), offset: (0.0, 0.0), label_dir: (0, -1),
                                     hole: None, xy: None });
            }
        }
        _ => {
            // offboard
            let n = count(spec, r)?;
            let names = labels(spec, n, r)?;
            if part.title.is_empty() {
                part.title = r.to_string();
            }
            part.cols = spec.get("cols").and_then(Val::py_int).unwrap_or(1);
            part.numbering = spec.get("numbering").map(Val::py_str).unwrap_or("row".into());
            part.place = spec.get("place").map(Val::py_str).unwrap_or("right".into());
            if !["row", "column"].contains(&part.numbering.as_str()) || !["right", "below"].contains(&part.place.as_str()) {
                return Err(format!("parts.{}: numbering is row|column and place is right|below", r));
            }
            for k in 1..=n {
                part.pins.push(Pin { num: k, name: names[(k - 1) as usize].clone(), offset: (0.0, 0.0), label_dir: (0, -1),
                                     hole: None, xy: None });
            }
        }
    }
    Ok(part)
}

// ---- parts from KiCad's library ----

fn powerish(name: &str) -> bool {
    let u = name.to_uppercase();
    let ground = |s: &str| s.starts_with("GND") || s.starts_with("VSS") || s.starts_with("0V");
    ground(&u) || (u.len() > 1 && matches!(&u[..1], "A" | "D" | "P") && ground(&u[1..]))
        || ["VCC", "VDD", "VBUS", "VSYS"].contains(&u.as_str())
}

fn library(library: Option<&[PathBuf]>) -> Vec<PathBuf> {
    match library {
        Some(l) if !l.is_empty() => l.to_vec(),
        _ => kicad::library_roots(None),
    }
}

fn kicad_parts(r: &str, spec: &Val, lib: Option<&[PathBuf]>)
               -> Result<(Option<kicad::Symbol>, String, Option<kicad::Footprint>), String> {
    let roots = library(lib);
    let wrap = |e: String| format!("parts.{}: {}", r, e);
    let sym = if spec.at("symbol").truthy() { Some(kicad::symbol(&spec.at("symbol").py_str(), &roots).map_err(wrap)?) } else { None };
    let fp_ref = if spec.at("footprint").truthy() {
        spec.at("footprint").py_str()
    } else {
        sym.as_ref().map(|s| s.footprint.clone()).unwrap_or_default()
    };
    if fp_ref.is_empty() {
        return Err(format!("parts.{}: KiCad's {} names no footprint (it comes in several packages); give one, like \
                            footprint: Package_DIP:DIP-8_W7.62mm", r, spec.at("symbol").py_str()));
    }
    let fp = if spec.at("adapter").truthy() { None } else { Some(kicad::footprint(&fp_ref, &roots).map_err(wrap)?) };
    Ok((sym, fp_ref, fp))
}

fn symbol_names(sym: &Option<kicad::Symbol>) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    if let Some(sym) = sym {
        for p in &sym.pins {
            if !p.number.is_empty() && p.number.chars().all(|c| c.is_ascii_digit()) {
                let n: i64 = p.number.parse().unwrap();
                let name = kicad::clean_name(&p.name);
                let name = if name.is_empty() { p.number.clone() } else { name };
                if let Some(slot) = out.iter_mut().find(|(k, _)| *k == n) {
                    slot.1 = name;
                } else {
                    out.push((n, name));
                }
            }
        }
    }
    out
}

fn from_kicad_part(part: &mut Part, spec: &Val, lib: Option<&[PathBuf]>) -> Result<(), String> {
    let (sym, fp_ref, fp) = kicad_parts(&part.r#ref, spec, lib)?;
    let fp = fp.unwrap();
    if fp.smd {
        return Err(format!("parts.{}: {} is surface-mount; put it on an SMD-to-DIP adapter, like adapter: {{pins: 6}}",
                           part.r#ref, fp_ref));
    }
    let mut pads: Vec<(i64, (f64, f64))> = Vec::new();
    for (n, xy) in &fp.pads {
        if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
            let k: i64 = n.parse().unwrap();
            if let Some(slot) = pads.iter_mut().find(|(j, _)| *j == k) {
                slot.1 = *xy;
            } else {
                pads.push((k, *xy));
            }
        }
    }
    if pads.is_empty() {
        return Err(format!("parts.{}: {} has no numbered through-hole pads", part.r#ref, fp_ref));
    }
    let first = pads.iter().find(|(k, _)| *k == 1).or_else(|| pads.iter().min_by_key(|(k, _)| *k)).unwrap().1;
    let (x1, y1) = first;
    let grid = |x: f64, y: f64| ((y - y1) / MM, -(x - x1) / MM);
    let grid_box = |b: Rect| {
        let corners: Vec<Xy> = [b.0, b.2].iter().flat_map(|&x| [b.1, b.3].map(|y| grid(x, y))).collect();
        let xs: Vec<f64> = corners.iter().map(|c| c.0).collect();
        let ys: Vec<f64> = corners.iter().map(|c| c.1).collect();
        (min(&xs), min(&ys), max(&xs), max(&ys))
    };
    let names = symbol_names(&sym);
    let mut sorted = pads.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut offsets: Vec<(i64, (i64, i64))> = Vec::new();
    for (n, (x, y)) in &sorted {
        let (gx, gy) = grid(*x, *y);
        if (gx - gx.round_ties_even()).abs() > 0.05 || (gy - gy.round_ties_even()).abs() > 0.05 {
            return Err(format!("parts.{}: {}'s pads aren't on the 0.1 in grid of a breadboard (pad {}); pick a footprint \
                                whose pads are, like TO-92_Inline_Wide for a TO-92", part.r#ref, fp_ref, n));
        }
        offsets.push((*n, (gx.round_ties_even() as i64, gy.round_ties_even() as i64)));
    }
    let mut ys: Vec<i64> = offsets.iter().map(|o| o.1 .1).collect();
    ys.sort();
    ys.dedup();
    let middle = (ys[0] + ys[ys.len() - 1]) as f64 / 2.0;
    for (n, off) in &offsets {
        let down = ys.len() == 1 || off.1 as f64 > middle;
        let name = names.iter().find(|(k, _)| k == n).map(|(_, s)| s.clone()).unwrap_or(n.to_string());
        part.pins.push(Pin { num: *n, name, offset: (off.0 as f64, off.1 as f64),
                             label_dir: if down { (0, -1) } else { (0, 1) }, hole: None, xy: None });
    }
    let xs: Vec<i64> = offsets.iter().map(|o| o.1 .0).collect();
    let ys2: Vec<i64> = offsets.iter().map(|o| o.1 .1).collect();
    let mut body = [*xs.iter().min().unwrap() as f64 - 0.5, *ys2.iter().min().unwrap() as f64 - 0.5,
                    *xs.iter().max().unwrap() as f64 + 0.5, *ys2.iter().max().unwrap() as f64 + 0.5];
    if let Some(outline) = fp.outline {
        let ob = grid_box(outline);
        body = [min(&[body[0], ob.0]), min(&[body[1], ob.1]), max(&[body[2], ob.2]), max(&[body[3], ob.3])];
    }
    part.body = Some((body[0], body[1], body[2], body[3]));
    part.keepouts = fp.keepouts.iter().map(|(what, b)| (what.clone(), grid_box(*b))).collect();
    let mut by_name: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, pin) in part.pins.iter().enumerate() {
        match by_name.iter_mut().find(|(n, _)| *n == pin.name) {
            Some(slot) => slot.1.push(i),
            None => by_name.push((pin.name.clone(), vec![i])),
        }
    }
    let kinds: Vec<(i64, String)> = sym.as_ref().map(|s| s.pins.iter()
        .filter(|p| !p.number.is_empty() && p.number.chars().all(|c| c.is_ascii_digit()))
        .map(|p| (p.number.parse().unwrap(), p.kind.clone())).collect()).unwrap_or_default();
    let kind_of = |n: i64| kinds.iter().rev().find(|(k, _)| *k == n).map(|(_, s)| s.as_str()).unwrap_or("");
    for (name, pins) in &by_name {
        if pins.len() > 1 && (powerish(name) || pins.iter().any(|&i| kind_of(part.pins[i].num).starts_with("power"))) {
            part.buses.push(pins.clone());
        }
    }
    if part.title.is_empty() {
        part.title = sym.as_ref().map(|s| s.name.clone()).unwrap_or(fp.name.clone());
    }
    part.source = if sym.is_some() { format!("{} on {}", spec.at("symbol").py_str(), fp_ref) } else { fp_ref };
    if sym.is_some() {
        part.symbol = spec.at("symbol").py_str();
    }
    Ok(())
}

fn adapter(part: &mut Part, spec: &Val, lib: Option<&[PathBuf]>) -> Result<(i64, Vec<String>), String> {
    let raw = spec.at("adapter");
    let adapter: Val = match raw {
        Val::Int(_) => Val::Map(vec![(Val::Str("pins".into()), raw.clone())]),
        _ => raw.clone(),
    };
    let n = match (adapter.as_map(), adapter.at("pins")) {
        (Some(_), Val::Int(n)) => *n,
        _ => return Err(format!("parts.{}: adapter is the adapter's pin count, or {{pins: 6, map: {{5: 6}}}}", part.r#ref)),
    };
    let (sym, fp_ref, _) = kicad_parts(&part.r#ref, spec, lib)?;
    let moves: Vec<(i64, i64)> = adapter.at("map").as_map().map(|m| m.iter()
        .filter_map(|(k, v)| Some((k.py_int()?, v.py_int()?))).collect()).unwrap_or_default();
    let mut names: Vec<String> = (1..=n).map(|k| k.to_string()).collect();
    let mut taken: Vec<(i64, i64)> = Vec::new();
    let mut sn = symbol_names(&sym);
    sn.sort_by(|a, b| a.0.cmp(&b.0));
    for (pad, name) in sn {
        let pin = moves.iter().rev().find(|(k, _)| *k == pad).map(|(_, v)| *v).unwrap_or(pad);
        if !(1..=n).contains(&pin) {
            return Err(format!("parts.{}: chip pad {} has no adapter pin {} (the adapter has {}); map it, like map: \
                                {{{}: {}}}", part.r#ref, pad, pin, n, pad, n));
        }
        if let Some((_, other)) = taken.iter().find(|(p, _)| *p == pin) {
            return Err(format!("parts.{}: chip pads {} and {} both on adapter pin {}", part.r#ref, other, pad, pin));
        }
        taken.push((pin, pad));
        names[(pin - 1) as usize] = name;
    }
    if part.title.is_empty() {
        part.title = sym.as_ref().map(|s| s.name.clone()).unwrap_or(fp_ref.rsplit(':').next().unwrap().to_string());
    }
    let mut mv = moves.clone();
    mv.sort();
    mv.dedup_by_key(|m| m.0);
    let shown = mv.iter().map(|(k, v)| format!("pad {} on pin {}", k, v)).collect::<Vec<_>>().join(", ");
    part.adapter = format!("{} on a {}-pin adapter: chip pad n on adapter pin n{}. Check this against your adapter",
                           fp_ref.rsplit(':').next().unwrap(), n, if shown.is_empty() { String::new() } else { format!(", except {}", shown) });
    part.source = if sym.is_some() { format!("{} on {}", spec.at("symbol").py_str(), fp_ref) } else { fp_ref };
    if sym.is_some() {
        part.symbol = spec.at("symbol").py_str();
        taken.sort();
        part.adapter_pads = taken;
    }
    Ok((n, names))
}
