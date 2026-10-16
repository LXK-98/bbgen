//! Reading parts from KiCad's libraries: a symbol's pins, its footprint's pads, outline and keep-outs.
//!
//! Only what bbgen needs is read, and only the symbol or footprint asked for: the library's symbol files run
//! to megabytes. Results are cached, since placement builds a circuit many times over. The library is looked
//! for where KiCad installs it (system packages, Flatpak, macOS) and where its environment variables point;
//! $BBGEN_KICAD or a circuit's `kicad:` names it outright.

use crate::symbol::{Drawing, Field, Fill, Shape, SymPin, Text, Unit, Xy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub const MM: f64 = 2.54;

#[derive(Clone, Debug)]
pub struct SymbolPin {
    pub number: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub pins: Vec<SymbolPin>,
    pub footprint: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct Footprint {
    pub name: String,
    /// through-hole pad number -> (x, y) in mm, in file order
    pub pads: Vec<(String, (f64, f64))>,
    pub smd: bool,
    pub outline: Option<(f64, f64, f64, f64)>,
    pub keepouts: Vec<(String, (f64, f64, f64, f64))>,
}

// ---- finding the library ----

fn glob_dirs(pattern: &str) -> Vec<PathBuf> {
    // only `*` as a whole path component, which is all the patterns use
    let mut paths = vec![PathBuf::from("/")];
    for part in pattern.trim_start_matches('/').split('/') {
        let mut next = Vec::new();
        for p in &paths {
            if part == "*" {
                if let Ok(rd) = std::fs::read_dir(p) {
                    let mut names: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                    names.sort();
                    next.extend(names);
                }
            } else {
                let q = p.join(part);
                if q.exists() {
                    next.push(q);
                }
            }
        }
        paths = next;
    }
    paths
}

fn expand_user(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(p)
}

/// Folders holding KiCad's `symbols/` and `footprints/`, most specific first.
pub fn library_roots(extra: Option<&str>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let env_kicad = std::env::var("BBGEN_KICAD").ok();
    for base in [extra.map(String::from), env_kicad].into_iter().flatten() {
        if !base.is_empty() {
            found.push(expand_user(&base));
        }
    }
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.sort();
    for (var, value) in vars {
        if let Some(rest) = var.strip_prefix("KICAD") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let tail = &rest[digits.len()..];
            if tail == "_SYMBOL_DIR" || tail == "_FOOTPRINT_DIR" {
                if let Some(parent) = Path::new(&value).parent() {
                    found.push(parent.to_path_buf());
                }
            }
        }
    }
    let home_flatpak = expand_user("~/.local/share/flatpak/runtime/org.kicad.kicad.Library/*/stable/*/files");
    let patterns = [
        "/var/lib/flatpak/runtime/org.kicad.kicad.Library/*/stable/*/files".to_string(),
        home_flatpak.to_string_lossy().into_owned(),
        "/usr/share/kicad".into(), "/usr/local/share/kicad".into(),
        "/Applications/KiCad/KiCad.app/Contents/SharedSupport".into(),
    ];
    for pattern in patterns {
        let mut hits = glob_dirs(&pattern);
        hits.reverse();
        found.extend(hits);
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in found {
        if root.join("symbols").is_dir() || root.join("footprints").is_dir() {
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// A KiCad pin name without its markup: ~{RST} (an overbar) is /RST, V_{IN} is VIN.
pub fn clean_name(name: &str) -> String {
    fn sub(s: &str, lead: &[char], with_slash: bool) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if lead.contains(&chars[i]) && i + 1 < chars.len() && chars[i + 1] == '{' {
                if let Some(end) = chars[i + 2..].iter().position(|&c| c == '}') {
                    let inner: String = chars[i + 2..i + 2 + end].iter().collect();
                    if with_slash {
                        out.push('/');
                    }
                    out.push_str(&inner);
                    i += end + 3;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }
    let s = sub(name, &['~'], true);
    sub(&s, &['_', '^'], false).trim().to_string()
}

fn find(roots: &[PathBuf], kind: &str, lib: &str) -> Result<PathBuf, String> {
    let suffix = if kind == "symbols" { ".kicad_sym" } else { ".pretty" };
    for root in roots {
        let path = root.join(kind).join(format!("{}{}", lib, suffix));
        if path.exists() {
            return Ok(path);
        }
    }
    let where_ = if roots.is_empty() {
        "nowhere: no KiCad library found (set BBGEN_KICAD)".to_string()
    } else {
        roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")
    };
    Err(format!("no KiCad {} library '{}' (looked in {})", &kind[..kind.len() - 1], lib, where_))
}

fn split<'a>(r: &'a str, what: &str) -> Result<(&'a str, &'a str), String> {
    match r.split_once(':') {
        Some((lib, name)) if !lib.is_empty() && !name.is_empty() => Ok((lib, name)),
        _ => Err(format!("a KiCad {} is named Library:Name, like Timer:NE555P, not '{}'", what, r)),
    }
}

// ---- S-expressions ----

#[derive(Clone, Debug)]
pub enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

impl Sx {
    fn atom(&self) -> Option<&str> {
        match self {
            Sx::Atom(s) => Some(s),
            _ => None,
        }
    }

    fn list(&self) -> Option<&Vec<Sx>> {
        match self {
            Sx::List(l) => Some(l),
            _ => None,
        }
    }

    fn head(&self) -> Option<&str> {
        self.list().and_then(|l| l.first()).and_then(Sx::atom)
    }

    fn children(&self, head: &str) -> Vec<&Sx> {
        self.list().map(|l| l.iter().filter(|c| c.head() == Some(head)).collect()).unwrap_or_default()
    }

    fn child(&self, head: &str) -> Option<&Sx> {
        self.list().and_then(|l| l.iter().find(|c| c.head() == Some(head)))
    }

    /// The i-th element, as a string (an atom).
    fn s(&self, i: usize) -> Option<&str> {
        self.list().and_then(|l| l.get(i)).and_then(Sx::atom)
    }

    fn len(&self) -> usize {
        self.list().map(Vec::len).unwrap_or(0)
    }
}

/// The one S-expression starting at text[start] (an opening parenthesis), as nested lists.
fn parse(text: &str, start: usize) -> Result<Sx, String> {
    let b = text.as_bytes();
    let mut stack: Vec<Vec<Sx>> = Vec::new();
    let mut i = start;
    while i < b.len() {
        let c = b[i];
        if c == b'(' {
            stack.push(Vec::new());
            i += 1;
        } else if c == b')' {
            let done = stack.pop().ok_or("unbalanced parentheses in a KiCad file")?;
            match stack.last_mut() {
                None => return Ok(Sx::List(done)),
                Some(top) => top.push(Sx::List(done)),
            }
            i += 1;
        } else if c == b'"' {
            // "(?:\\.|[^"\\])*"
            let mut j = i + 1;
            let mut closed = false;
            while j < b.len() {
                if b[j] == b'\\' && j + 1 < b.len() {
                    j += 2;
                    continue;
                }
                if b[j] == b'"' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                i += 1; // a lone quote matches nothing: skipped
                continue;
            }
            let raw = &text[i + 1..j];
            let tok = raw.replace("\\\"", "\"").replace("\\\\", "\\");
            if let Some(top) = stack.last_mut() {
                top.push(Sx::Atom(tok));
            }
            i = j + 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else {
            let mut j = i;
            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'(' && b[j] != b')' && b[j] != b'"' {
                j += 1;
            }
            if let Some(top) = stack.last_mut() {
                top.push(Sx::Atom(text[i..j].to_string()));
            }
            i = j;
        }
    }
    Err("unbalanced parentheses in a KiCad file".into())
}

// ---- symbols ----

fn cache<T: Clone>(map: &'static OnceLock<Mutex<HashMap<String, T>>>, key: &str, make: impl FnOnce() -> T) -> T {
    let m = map.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = m.lock().unwrap().get(key) {
        return v.clone();
    }
    let v = make();
    m.lock().unwrap().insert(key.to_string(), v.clone());
    v
}

static TEXTS: OnceLock<Mutex<HashMap<String, std::sync::Arc<String>>>> = OnceLock::new();
static SYMBOLS: OnceLock<Mutex<HashMap<String, Result<Symbol, String>>>> = OnceLock::new();
static FOOTPRINTS: OnceLock<Mutex<HashMap<String, Result<Footprint, String>>>> = OnceLock::new();

fn symbol_text(path: &Path) -> Result<std::sync::Arc<String>, String> {
    let key = path.display().to_string();
    let text = cache(&TEXTS, &key, || std::sync::Arc::new(std::fs::read_to_string(path).unwrap_or_default()));
    Ok(text)
}

fn symbol_node(text: &str, name: &str) -> Result<Option<Sx>, String> {
    // ^\t?\(symbol "NAME"  (multiline)
    let needle = format!("(symbol \"{}\"", name);
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let rest = line.strip_prefix('\t').unwrap_or(line);
        if rest.starts_with(&needle) {
            let at = line_start + (line.len() - rest.len());
            return parse(text, at).map(Some);
        }
        line_start += line.len();
    }
    Ok(None)
}

fn props_of(node: &Sx) -> Vec<(String, String)> {
    node.children("property").into_iter().filter(|p| p.len() > 2)
        .filter_map(|p| Some((p.s(1)?.to_string(), p.s(2)?.to_string()))).collect()
}

pub fn symbol(r: &str, roots: &[PathBuf]) -> Result<Symbol, String> {
    let key = format!("{}|{:?}", r, roots);
    cache(&SYMBOLS, &key, || symbol_uncached(r, roots))
}

fn symbol_uncached(r: &str, roots: &[PathBuf]) -> Result<Symbol, String> {
    let (lib, name) = split(r, "symbol")?;
    let text = symbol_text(&find(roots, "symbols", lib)?)?;
    let node = symbol_node(&text, name)?.ok_or(format!("no symbol '{}' in KiCad's {} library", name, lib))?;
    let mut props: Vec<(String, String)> = Vec::new();
    let merge = |props: &mut Vec<(String, String)>, new: Vec<(String, String)>| {
        for (k, v) in new {
            if let Some(slot) = props.iter_mut().find(|(k2, _)| *k2 == k) {
                slot.1 = v;
            } else {
                props.push((k, v));
            }
        }
    };
    merge(&mut props, props_of(&node));
    let mut base = node;
    let mut seen = vec![name.to_string()];
    while let Some(ext) = base.child("extends") {
        let parent = ext.s(1).unwrap_or("").to_string();
        if seen.contains(&parent) {
            return Err(format!("symbol '{}' extends itself", name));
        }
        seen.push(parent.clone());
        let next = symbol_node(&text, &parent)?
            .ok_or(format!("symbol '{}' extends '{}', which isn't in {}", name, parent, lib))?;
        // the base's properties first, the derived symbol's over them
        let mut merged = props_of(&next);
        merge(&mut merged, props.clone());
        props = merged;
        base = next;
    }
    let mut pins: Vec<SymbolPin> = Vec::new();
    let mut units = vec![&base];
    units.extend(base.children("symbol"));
    for unit in units {
        for pin in unit.children("pin") {
            let number = match pin.child("number") {
                Some(n) => n.s(1).unwrap_or("").to_string(),
                None => continue,
            };
            let label = pin.child("name").and_then(|n| n.s(1)).unwrap_or("").to_string();
            if !pins.iter().any(|p| p.number == number) {
                pins.push(SymbolPin { number, name: if label == "~" { String::new() } else { label },
                                      kind: pin.s(1).unwrap_or("").to_string() });
            }
        }
    }
    let get = |k: &str| props.iter().find(|(k2, _)| k2 == k).map(|(_, v)| v.clone()).unwrap_or_default();
    Ok(Symbol { name: name.to_string(), pins, footprint: get("Footprint"), description: get("Description") })
}

// ---- symbols as drawn, for the circuit view ----

static DRAWINGS: OnceLock<Mutex<HashMap<String, Result<Arc<Drawing>, String>>>> = OnceLock::new();

/// A symbol as KiCad draws it: each unit's lines, shapes, text and pins, and where its reference and value go,
/// in millimetres with y down. Apart from `symbol`, which every build of a circuit reads: placement doesn't
/// need the graphics.
pub fn drawing(r: &str, roots: &[PathBuf]) -> Result<Arc<Drawing>, String> {
    let key = format!("{}|{:?}", r, roots);
    cache(&DRAWINGS, &key, || drawing_uncached(r, roots).map(Arc::new))
}

/// (x, y) of a child like (at x y) or (start x y), y turned down.
fn xy(node: &Sx, head: &str) -> Option<Xy> {
    node.child(head).map(|c| (num(c.s(1)), -num(c.s(2))))
}

/// `(hide yes)`, or a bare `hide` as older files write it, on the node or in its effects.
fn hidden(node: &Sx) -> bool {
    let here = |n: &Sx| n.list().map(|l| l.iter().any(|c| c.atom() == Some("hide") || (c.head() == Some("hide") && c.s(1) != Some("no"))))
        .unwrap_or(false);
    here(node) || node.child("effects").map(here).unwrap_or(false)
}

fn font_size(node: &Sx) -> f64 {
    node.child("effects").and_then(|e| e.child("font")).and_then(|f| f.child("size")).map(|s| num(s.s(1)))
        .filter(|s| s.is_finite()).unwrap_or(1.27)
}

/// (along, across) from `(justify left|right top|bottom)`: -1 start or top, 1 end or bottom, 0 centred.
fn justify(node: &Sx) -> (i8, i8) {
    let words: Vec<&str> = node.child("effects").and_then(|e| e.child("justify"))
        .map(|j| j.list().unwrap()[1..].iter().filter_map(Sx::atom).collect()).unwrap_or_default();
    let h = if words.contains(&"left") { -1 } else if words.contains(&"right") { 1 } else { 0 };
    let v = if words.contains(&"top") { -1 } else if words.contains(&"bottom") { 1 } else { 0 };
    (h, v)
}

/// The way text at this angle (KiCad's, counter-clockwise) reads on the page.
fn reading(angle: f64) -> (i32, i32) {
    match (angle.rem_euclid(360.0) / 90.0).round() as i32 % 4 {
        1 => (0, -1),
        2 => (-1, 0),
        3 => (0, 1),
        _ => (1, 0),
    }
}

fn stroke_fill(node: &Sx) -> (f64, Fill) {
    let width = node.child("stroke").and_then(|s| s.child("width")).map(|w| num(w.s(1))).filter(|w| w.is_finite()).unwrap_or(0.0);
    let fill = match node.child("fill").and_then(|f| f.child("type")).and_then(|t| t.s(1)) {
        Some("outline") => Fill::Outline,
        Some("background") | Some("color") => Fill::Background,
        _ => Fill::None,
    };
    (width, fill)
}

fn shapes_of(unit: &Sx) -> Vec<Shape> {
    let mut out = Vec::new();
    for item in unit.list().map(|l| l.as_slice()).unwrap_or(&[]) {
        let (width, fill) = stroke_fill(item);
        let shape = match item.head() {
            Some("rectangle") => match (xy(item, "start"), xy(item, "end")) {
                (Some(a), Some(b)) => Shape::Rect { a, b, width, fill },
                _ => continue,
            },
            Some("polyline") | Some("bezier") => {
                let pts: Vec<Xy> = item.child("pts").map(|p| p.children("xy").iter().map(|q| (num(q.s(1)), -num(q.s(2)))).collect())
                    .unwrap_or_default();
                if pts.len() < 2 {
                    continue;
                }
                Shape::Poly { pts, width, fill }
            }
            Some("circle") => match (xy(item, "center"), item.child("radius").map(|r| num(r.s(1)))) {
                (Some(c), Some(r)) => Shape::Circle { c, r, width, fill },
                _ => continue,
            },
            Some("arc") => match (xy(item, "start"), xy(item, "mid"), xy(item, "end")) {
                (Some(a), Some(mid), Some(b)) => Shape::Arc { a, mid, b, width },
                _ => continue,
            },
            Some("text") => {
                let at = match item.child("at") {
                    Some(at) => at,
                    None => continue,
                };
                // a symbol's text angles are in tenths of a degree
                let mut angle = num(at.s(3));
                if angle.abs() >= 360.0 {
                    angle /= 10.0;
                }
                let (h, v) = justify(item);
                Shape::Text(Text { at: (num(at.s(1)), -num(at.s(2))), dir: reading(if angle.is_finite() { angle } else { 0.0 }),
                                   text: item.s(1).unwrap_or("").to_string(), size: font_size(item), h, v })
            }
            _ => continue,
        };
        out.push(shape);
    }
    out
}

fn pins_of(unit: &Sx) -> Vec<SymPin> {
    let mut out = Vec::new();
    for p in unit.children("pin") {
        let at = match p.child("at") {
            Some(at) => at,
            None => continue,
        };
        let name = p.child("name");
        let number = p.child("number");
        out.push(SymPin {
            number: number.and_then(|n| n.s(1)).unwrap_or("").to_string(),
            name: name.and_then(|n| n.s(1)).unwrap_or("").to_string(),
            kind: p.s(1).unwrap_or("").to_string(),
            shape: p.s(2).unwrap_or("line").to_string(),
            at: (num(at.s(1)), -num(at.s(2))),
            dir: reading(num(at.s(3))),
            len: p.child("length").map(|l| num(l.s(1))).filter(|l| l.is_finite()).unwrap_or(2.54),
            hidden: hidden(p),
            name_size: name.map(font_size).unwrap_or(1.27),
            number_size: number.map(font_size).unwrap_or(1.27),
            pins: vec![],
        });
    }
    out
}

fn field_of(node: &Sx) -> Option<Field> {
    let at = node.child("at")?;
    let (h, v) = justify(node);
    Some(Field { at: (num(at.s(1)), -num(at.s(2))), dir: reading(num(at.s(3))), h, v, size: font_size(node), hidden: hidden(node) })
}

fn drawing_uncached(r: &str, roots: &[PathBuf]) -> Result<Drawing, String> {
    let (lib, name) = split(r, "symbol")?;
    let text = symbol_text(&find(roots, "symbols", lib)?)?;
    let node = symbol_node(&text, name)?.ok_or(format!("no symbol '{}' in KiCad's {} library", name, lib))?;
    // a derived symbol (`extends`) is drawn as its base; its own fields win
    let mut chain = vec![node];
    while let Some(ext) = chain.last().unwrap().child("extends") {
        let parent = ext.s(1).unwrap_or("").to_string();
        if chain.len() > 8 {
            return Err(format!("symbol '{}' extends itself", name));
        }
        chain.push(symbol_node(&text, &parent)?.ok_or(format!("symbol '{}' extends '{}', which isn't in {}", name, parent, lib))?);
    }
    let base = chain.last().unwrap();
    let field = |key: &str| chain.iter().find_map(|n| n.children("property").into_iter().find(|p| p.s(1) == Some(key)))
        .and_then(field_of);
    let names = base.child("pin_names");
    let numbers = base.child("pin_numbers");
    let name_offset = names.and_then(|n| n.child("offset")).map(|o| num(o.s(1))).filter(|o| o.is_finite()).unwrap_or(0.508);
    // units are named NAME_<unit>_<style>: unit 0 belongs to every unit, style 2 is De Morgan's (not drawn)
    let mut common: (Vec<Shape>, Vec<SymPin>) = (vec![], vec![]);
    let mut units: Vec<Unit> = Vec::new();
    for sub in base.children("symbol") {
        let full = sub.s(1).unwrap_or("");
        let mut tail = full.rsplitn(3, '_');
        let style: i64 = tail.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let unit: i64 = tail.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        if style == 2 {
            continue;
        }
        let (shapes, pins) = (shapes_of(sub), pins_of(sub));
        if unit == 0 {
            common.0.extend(shapes);
            common.1.extend(pins);
        } else if let Some(u) = units.iter_mut().find(|u| u.unit == unit) {
            u.shapes.extend(shapes);
            u.pins.extend(pins);
        } else {
            units.push(Unit { unit, shapes, pins });
        }
    }
    if units.is_empty() {
        units.push(Unit { unit: 1, shapes: vec![], pins: vec![] });
    }
    units.sort_by_key(|u| u.unit);
    for u in &mut units {
        let mut shapes = common.0.clone();
        shapes.append(&mut u.shapes);
        u.shapes = shapes;
        u.pins.extend(common.1.iter().cloned());
    }
    Ok(Drawing {
        units, name_offset,
        names: !names.map(hidden).unwrap_or(false),
        numbers: !numbers.map(hidden).unwrap_or(false),
        reference: field("Reference"),
        value: field("Value"),
    })
}

// ---- footprints ----

fn bbox(points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let mut b = (points[0].0, points[0].1, points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        // Python's min()/max(): the first of equal values (only -0.0 against 0.0 could tell)
        if x < b.0 { b.0 = x; }
        if y < b.1 { b.1 = y; }
        if x > b.2 { b.2 = x; }
        if y > b.3 { b.3 = y; }
    }
    Some(b)
}

fn num(s: Option<&str>) -> f64 {
    s.and_then(|s| s.parse().ok()).unwrap_or(f64::NAN)
}

fn layer_points(node: &Sx, layer: &str) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for item in node.list().map(|l| l.as_slice()).unwrap_or(&[]) {
        if !item.head().map(|h| h.starts_with("fp_")).unwrap_or(false) {
            continue;
        }
        match item.child("layer") {
            Some(lay) if lay.s(1) == Some(layer) => {}
            _ => continue,
        }
        for key in ["start", "end", "center", "mid"] {
            if let Some(p) = item.child(key) {
                points.push((num(p.s(1)), num(p.s(2))));
            }
        }
        if let Some(pts) = item.child("pts") {
            for xy in pts.children("xy") {
                points.push((num(xy.s(1)), num(xy.s(2))));
            }
        }
    }
    points
}

pub fn footprint(r: &str, roots: &[PathBuf]) -> Result<Footprint, String> {
    let key = format!("{}|{:?}", r, roots);
    cache(&FOOTPRINTS, &key, || footprint_uncached(r, roots))
}

fn footprint_uncached(r: &str, roots: &[PathBuf]) -> Result<Footprint, String> {
    let (lib, name) = split(r, "footprint")?;
    let path = find(roots, "footprints", lib)?.join(format!("{}.kicad_mod", name));
    if !path.exists() {
        return Err(format!("no footprint '{}' in KiCad's {} library", name, lib));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let node = parse(&text, text.find('(').ok_or("not a KiCad footprint")?)?;
    let mut pads: Vec<(String, (f64, f64))> = Vec::new();
    let mut smd = 0;
    for pad in node.children("pad") {
        let number = pad.s(1).unwrap_or("").to_string();
        let kind = pad.s(2).unwrap_or("");
        let at = pad.child("at");
        if kind == "smd" {
            smd += 1;
        } else if kind == "thru_hole" && !number.is_empty() {
            if let Some(at) = at {
                if !pads.iter().any(|(n, _)| *n == number) {
                    pads.push((number, (num(at.s(1)), num(at.s(2)))));
                }
            }
        }
    }
    let outline = bbox(&layer_points(&node, "F.Fab")).or_else(|| bbox(&layer_points(&node, "F.CrtYd")));
    let mut keepouts = Vec::new();
    for zone in node.children("zone") {
        let mut layers: Vec<Option<&Sx>> = vec![zone.child("layer")];
        layers.extend(zone.children("layers").into_iter().map(Some));
        let named = zone.child("name");
        let crtyd = layers.iter().flatten().any(|lay| {
            let words: Vec<&str> = lay.list().unwrap()[1..].iter().filter_map(Sx::atom).collect();
            words.join(" ").contains("CrtYd")
        });
        if let (Some(named), true) = (named, crtyd) {
            let pts = zone.child("polygon").and_then(|p| p.child("pts"));
            let b = pts.and_then(|pts| bbox(&pts.children("xy").iter().map(|xy| (num(xy.s(1)), num(xy.s(2)))).collect::<Vec<_>>()));
            if let Some(b) = b {
                keepouts.push((named.s(1).unwrap_or("").to_string(), b));
            }
        }
    }
    let has_pads = !pads.is_empty();
    Ok(Footprint { name: name.to_string(), pads, smd: smd > 0 && !has_pads, outline, keepouts })
}

// ---- searching ----

/// (Library:Name, footprint, description) for symbols whose name, description or keywords hold every word,
/// names that hold them first.
pub fn search(words: &[String], roots: &[PathBuf], limit: usize) -> Vec<(String, String, String)> {
    let words: Vec<String> = words.iter().filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect();
    let mut seen: Vec<String> = Vec::new();
    let mut hits: Vec<(i64, String, String, String)> = Vec::new();
    for root in roots {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("symbols")).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "kicad_sym").unwrap_or(false)).collect()).unwrap_or_default();
        paths.sort();
        for path in paths {
            let lib = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            // ^\t\(symbol "([^"]+)"
            let mut starts: Vec<(usize, String)> = Vec::new();
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                if let Some(rest) = line.strip_prefix("\t(symbol \"") {
                    if let Some(end) = rest.find('"') {
                        if end > 0 {
                            starts.push((at, rest[..end].to_string()));
                        }
                    }
                }
                at += line.len();
            }
            for (k, (start, name)) in starts.iter().enumerate() {
                let r = format!("{}:{}", lib, name);
                if seen.contains(&r) {
                    continue;
                }
                let stop = starts.get(k + 1).map(|s| s.0).unwrap_or(text.len());
                let head: String = text[*start..stop].chars().take(6000).collect();
                let props = properties(&head);
                let get = |k: &str| props.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
                let about = format!("{} {} {}", r, get("Description"), get("ki_keywords")).to_lowercase();
                if words.iter().all(|w| about.contains(w.as_str())) {
                    seen.push(r.clone());
                    let in_name = words.iter().filter(|w| r.to_lowercase().contains(w.as_str())).count() as i64;
                    hits.push((-in_name, r, get("Footprint"), get("Description")));
                }
            }
        }
    }
    hits.sort();
    hits.into_iter().take(limit).map(|(_, r, fp, about)| (r, fp, about)).collect()
}

/// \(property "(Description|ki_keywords|Footprint)" "((?:\\.|[^"\\])*)" over a symbol's text.
fn properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find("(property \"") {
        let start = i + off + "(property \"".len();
        i = start;
        let key = ["Description", "ki_keywords", "Footprint"].iter().find(|k| text[start..].starts_with(&format!("{}\" \"", k)));
        if let Some(k) = key {
            let vstart = start + k.len() + 3;
            let mut j = vstart;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            if j < b.len() {
                out.push((k.to_string(), text[vstart..j].to_string()));
                i = j + 1;
            }
        }
    }
    out
}
