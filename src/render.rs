//! SVG drawing of the boards, the parts on them, the wires and any problems.

use crate::board::{Board, Row, Xy, RAILS, TERMINAL};
use crate::check::Result;
use crate::circuit::{color_hex, Circuit, Node, Wire};
use crate::parts::{body_rect, placed_box, rotate, Kind, Part};
use crate::pyf;

const P: f64 = 20.0;
const FONT: &str = "DejaVu Sans, Helvetica, Arial, sans-serif";
const HEADER: f64 = 3.4;
const INK: &str = "#262626";
const MUTED: &str = "#9a917f";
const BOARD: &str = "#f6f3ec";
const TRENCH_Y: (f64, f64) = (7.6, 9.4);
const BAND: [&str; 10] = ["#1a1a1a", "#7a4a2a", "#d0281c", "#f07c12", "#f4d21a", "#2e9a3a", "#2152c4", "#7d3fb0", "#8c8c8c", "#f9f9f9"];
const GOLD: &str = "#c9a227";
const SILVER: &str = "#b8bcc2";
const LEG: &str = "#c9cdd2";
const LEG_EDGE: &str = "#4d5257";

fn led_colors(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "red" => ("#f0473b", "#b3261c"),
        "green" => ("#3fc35a", "#1e8a35"),
        "yellow" => ("#f7d730", "#c4a40a"),
        "blue" => ("#4a8cf0", "#1f56b3"),
        "white" => ("#fbfbfb", "#c9c9c9"),
        "orange" => ("#ff9636", "#cc6510"),
        _ => return None,
    })
}

/// (fill, outline, thickness, longest body) for a two-lead part.
fn lead_style(kind: Kind) -> (&'static str, &'static str, f64, f64) {
    match kind {
        Kind::Resistor => ("#e6d2a4", "#8b7448", 0.5, 1.9),
        Kind::Capacitor => ("#e9c84a", "#94791c", 0.55, 0.8),
        Kind::Electrolytic => ("#253f8e", "#101c45", 0.85, 1.3),
        Kind::Diode => ("#2a2a2a", "#000000", 0.45, 1.3),
        _ => ("#e8463a", "#8e1d14", 0.8, 0.8),
    }
}

/// A number as Python's str() writes it: an int literal as an int, a float as its repr.
#[derive(Clone, Copy)]
pub struct N(pub f64, pub bool);

impl N {
    fn s(self) -> String {
        if self.1 { format!("{}", self.0 as i64) } else { pyf::repr(self.0) }
    }
}

const fn i(x: i64) -> N {
    N(x as f64, true)
}

const fn f(x: f64) -> N {
    N(x, false)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn ohms_match(s: &str) -> Option<(String, String, String)> {
    // ^(\d*)(?:[.,](\d+))?([rkm]?)$  |  ^(\d+)([rkm])(\d+)$
    let digits = |t: &str| t.chars().take_while(|c| c.is_ascii_digit()).count();
    let n = digits(s);
    let (whole, rest) = (&s[..n], &s[n..]);
    let mut frac = "";
    let mut r = rest;
    if let Some(after) = rest.strip_prefix(['.', ',']) {
        let m = digits(after);
        if m > 0 {
            frac = &after[..m];
            r = &after[m..];
        }
    }
    if r.is_empty() || (r.len() == 1 && "rkm".contains(r)) {
        return Some((whole.into(), frac.into(), r.into()));
    }
    // the second form: 4k7
    if n > 0 {
        let mut ch = rest.chars();
        if let Some(c) = ch.next() {
            let tail = ch.as_str();
            if "rkm".contains(c) && !tail.is_empty() && tail.chars().all(|x| x.is_ascii_digit()) {
                return Some((whole.into(), tail.into(), c.to_string()));
            }
        }
    }
    None
}

/// Colour bands for a resistance like 10k, 330R, 4k7, 2.2M or 100.
pub fn resistor_bands(value: &str) -> Option<Vec<&'static str>> {
    let s = value.trim().to_lowercase().replace('ω', "").replace("ohms", "").replace("ohm", "").replace(' ', "");
    let (whole, frac, suffix) = ohms_match(&s)?;
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let base: f64 = format!("{}.{}", if whole.is_empty() { "0" } else { &whole }, if frac.is_empty() { "0" } else { &frac }).parse().ok()?;
    let ohms = base * match suffix.as_str() { "k" => 1e3, "m" => 1e6, _ => 1.0 };
    if ohms == 0.0 {
        return Some(vec![BAND[0]]);
    }
    let (mut sig, mut exp, mut tolerance) = (0i64, 0i64, GOLD);
    for (digits, tol) in [(2i64, GOLD), (3, BAND[1])] {
        exp = ohms.log10().floor() as i64 - digits + 1;
        let p = if exp >= 0 { 10f64.powi(exp as i32) } else { 10f64.powf(exp as f64) };
        sig = (ohms / p).round_ties_even() as i64;
        if sig == 10i64.pow(digits as u32) {
            sig /= 10;
            exp += 1;
        }
        tolerance = tol;
        if (sig as f64 * 10f64.powf(exp as f64) - ohms).abs() <= ohms * 1e-9 || digits == 3 {
            break;
        }
    }
    if !(-2..=9).contains(&exp) {
        return None;
    }
    let mult = if exp == -1 { GOLD } else if exp == -2 { SILVER } else { BAND[exp as usize] };
    let mut out: Vec<&'static str> = sig.to_string().chars().map(|d| BAND[d.to_digit(10).unwrap() as usize]).collect();
    out.push(mult);
    out.push(tolerance);
    Some(out)
}

/// Dark outline around a wire, or a light one when the wire itself is dark.
fn outline(color: &str) -> &'static str {
    if color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        let v = |k: usize| i64::from_str_radix(&color[k..k + 2], 16).unwrap() as f64 / 255.0;
        if 0.2126 * v(1) + 0.7152 * v(3) + 0.0722 * v(5) < 0.25 {
            return "#c2c2c2";
        }
    }
    "#1d1d1d"
}

fn text_box(gx: f64, gy: f64, s: &str, size: f64, anchor: &str, vertical: bool) -> (f64, f64, f64, f64) {
    let n = s.chars().count() as f64;
    let (w, h) = (n * size * 0.55 / P, size * 0.8 / P);
    let lo = match anchor {
        "start" => 0.0,
        "end" => -w,
        _ => -w / 2.0,
    };
    if vertical {
        return (gx - h / 2.0, gy - lo - w, gx + h / 2.0, gy - lo);
    }
    (gx + lo, gy - h / 2.0, gx + lo + w, gy + h / 2.0)
}

fn seg_hits_box(a: Xy, b: Xy, bx: (f64, f64, f64, f64)) -> bool {
    let (x0, y0, x1, y1) = bx;
    let n = 2i64.max((pyf::dist(a, b) * 4.0) as i64);
    (0..=n).any(|t| {
        let x = a.0 + (b.0 - a.0) * t as f64 / n as f64;
        let y = a.1 + (b.1 - a.1) * t as f64 / n as f64;
        x0 < x && x < x1 && y0 < y && y < y1
    })
}

pub struct Canvas {
    x0: f64,
    y0: f64,
    pub out: Vec<String>,
    late: Vec<String>,
    /// (box, text) in grid units for each label over the wires, and wire tags
    pub labels: Vec<((f64, f64, f64, f64), String)>,
    spots: Vec<Xy>,
    segs: Vec<(Xy, Xy, String)>,
}

#[derive(Clone, Copy)]
struct T<'a> {
    size: N,
    anchor: &'a str,
    fill: &'a str,
    weight: &'a str,
    vertical: bool,
    halo: Option<&'a str>,
    late: bool,
}

impl<'a> T<'a> {
    fn new(size: N) -> T<'a> {
        T { size, anchor: "middle", fill: INK, weight: "normal", vertical: false, halo: None, late: false }
    }
    fn anchor(mut self, a: &'a str) -> Self {
        self.anchor = a;
        self
    }
    fn fill(mut self, f: &'a str) -> Self {
        self.fill = f;
        self
    }
    fn bold(mut self) -> Self {
        self.weight = "bold";
        self
    }
    fn weight(mut self, w: &'a str) -> Self {
        self.weight = w;
        self
    }
    fn vertical(mut self, v: bool) -> Self {
        self.vertical = v;
        self
    }
    fn halo(mut self, h: Option<&'a str>) -> Self {
        self.halo = h;
        self
    }
    fn late(mut self) -> Self {
        self.late = true;
        self
    }
}

impl Canvas {
    fn clash(&self, bx: (f64, f64, f64, f64), owner: &str) -> f64 {
        let (x0, y0, x1, y1) = bx;
        let mut n = 3.0 * self.spots.iter().filter(|(x, y)| x0 < *x && *x < x1 && y0 < *y && *y < y1).count() as f64;
        n += 3.0 * self.labels.iter().filter(|(b, _)| x0 < b.2 && b.0 < x1 && y0 < b.3 && b.1 < y1).count() as f64;
        n += self.segs.iter().filter(|(a, b, who)| who != owner && seg_hits_box(*a, *b, bx)).count() as f64;
        n
    }

    /// A label at the first of spots that hides least.
    fn label(&mut self, spots: &[(f64, f64, &str)], s: &str, t: T, owner: &str) {
        let mut best = 0;
        let mut bk = f64::INFINITY;
        for (k, sp) in spots.iter().enumerate() {
            let c = self.clash(text_box(sp.0, sp.1, s, t.size.0, sp.2, false), owner);
            if c < bk {
                bk = c;
                best = k;
            }
        }
        let (gx, gy, anchor) = spots[best];
        self.text(gx, gy, s, t.anchor(anchor).late());
    }

    fn x(&self, gx: f64) -> f64 {
        (gx - self.x0) * P
    }

    fn y(&self, gy: f64) -> f64 {
        (gy - self.y0) * P
    }

    fn add(&mut self, svg: String) {
        self.out.push(svg);
    }

    fn text(&mut self, gx: f64, gy: f64, s: &str, t: T) {
        if t.late {
            self.labels.push((text_box(gx, gy, s, t.size.0, t.anchor, t.vertical), s.to_string()));
        }
        let (mut x, mut y) = (self.x(gx), self.y(gy));
        let mut tr = String::new();
        if t.vertical {
            x += 0.35 * t.size.0;
            tr = format!(" transform=\"rotate(-90 {:.1} {:.1})\"", x, y);
        } else {
            y += 0.35 * t.size.0;
        }
        let extra = match t.halo {
            Some(h) => format!(" stroke=\"{}\" stroke-width=\"2.6\" stroke-linejoin=\"round\" paint-order=\"stroke\"", h),
            None => String::new(),
        };
        let svg = format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{}\" text-anchor=\"{}\" fill=\"{}\" font-weight=\"{}\"{}{}>{}</text>",
                          x, y, t.size.s(), t.anchor, t.fill, t.weight, tr, extra, escape(s));
        if t.late { self.late.push(svg) } else { self.out.push(svg) }
    }

    #[allow(clippy::too_many_arguments)]
    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, fill: &str, stroke: &str, width: N, rx: f64, extra: &str) {
        let svg = format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>",
                          self.x(x0), self.y(y0), (x1 - x0) * P, (y1 - y0) * P, rx * P, fill, stroke, width.s(), extra);
        self.add(svg);
    }

    fn circle(&mut self, gx: f64, gy: f64, r: f64, fill: &str, stroke: &str, width: N) {
        let svg = format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
                          self.x(gx), self.y(gy), r * P, fill, stroke, width.s());
        self.add(svg);
    }

    fn line(&mut self, ax: f64, ay: f64, bx: f64, by: f64, stroke: &str, width: f64) {
        let svg = format!("<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\"/>",
                          self.x(ax), self.y(ay), self.x(bx), self.y(by), stroke, width * P);
        self.add(svg);
    }
}

const W1: N = f(1.0); // the default stroke width (1.0)

fn board(cv: &mut Canvas, c: &Circuit, result: &Result, b: &Board) {
    let top = b.y0;
    cv.rect(b.left(), b.top(), b.right(), b.bottom(), BOARD, "#d5cdbb", f(1.5), 0.5, "");
    let last = b.x0 + b.columns as f64 - 1.0;
    cv.rect(b.x0 - 0.9, top + TRENCH_Y.0, last + 0.9, top + TRENCH_Y.1, "#e7e1d3", "none", W1, 0.2, "");
    if c.boards.len() > 1 {
        cv.text(b.left() + 0.4, b.top() + 0.5, &format!("board {}", b.id), T::new(f(6.5)).anchor("start").fill(MUTED).bold());
    }
    for rail in RAILS {
        let ry = b.rail_y(rail);
        let y = top + ry as f64;
        let upper = ry == if rail.side() == "top" { 0 } else { 16 };
        let color = if rail.sign() == '+' { "#d33a2c" } else { "#2a5db0" };
        let (xa, xb) = (b.x0 + b.rail_cols[0] as f64 - 1.7, b.x0 + *b.rail_cols.last().unwrap() as f64 - 0.3);
        let off = if upper { -0.55 } else { 0.55 };
        cv.line(xa, y + off, xb, y + off, color, 0.06);
        let sign = if rail.sign() == '+' { "+" } else { "−" };
        cv.text(xa - 0.4, y, sign, T::new(i(9)).fill(color).bold());
        cv.text(xb + 0.4, y, sign, T::new(i(9)).fill(color).bold());
        let mut halves: Vec<crate::board::Strip> = Vec::new();
        for h in b.holes().into_iter().filter(|h| h.row == rail) {
            let s = b.strip(h);
            if !halves.contains(&s) {
                halves.push(s);
            }
        }
        halves.sort_by_key(|s| s.n);
        let lx = crate::circuit::pymin(b.x0 - 1.2, xa - 1.0);
        let rx = crate::circuit::pymax(last + 1.2, xb + 1.0);
        for s in halves {
            let nets = result.nets_at(Node::Strip(s));
            if nets.len() == 1 {
                let at_right = s.n == 1;
                cv.text(if at_right { rx } else { lx }, y, &nets[0], T::new(i(6)).anchor(if at_right { "start" } else { "end" }).fill(color).bold());
                if !b.split_rails {
                    cv.text(rx, y, &nets[0], T::new(i(6)).anchor("start").fill(color).bold());
                }
            }
        }
    }
    for h in b.holes() {
        let (x, y) = b.xy(h);
        cv.rect(x - 0.17, y - 0.17, x + 0.17, y + 0.17, "#4b4b4b", "none", W1, 0.06, "");
    }
    for row in TERMINAL {
        cv.text(b.x0 - 1.1, b.y(row), row.name(), T::new(f(6.5)).fill(MUTED));
        cv.text(last + 1.1, b.y(row), row.name(), T::new(f(6.5)).fill(MUTED));
    }
    let mut cols = vec![1];
    cols.extend((5..=b.columns).step_by(5));
    for col in cols {
        cv.text(b.x0 + col as f64 - 1.0, top + 2.05, &col.to_string(), T::new(f(5.5)).fill(MUTED));
        cv.text(b.x0 + col as f64 - 1.0, top + 14.95, &col.to_string(), T::new(f(5.5)).fill(MUTED));
    }
}

fn keepouts(cv: &mut Canvas, c: &Circuit, part: &Part) {
    if part.keepouts.is_empty() || part.pins[0].xy.is_none() || part.pins[0].hole.is_none() {
        return;
    }
    let b = &c.boards[part.pins[0].hole.unwrap().board as usize];
    for (what, rect) in &part.keepouts {
        let (x0, y0, x1, y1) = placed_box(part, *rect);
        let (x0, x1) = (crate::circuit::pymax(x0, b.left() + 0.3), crate::circuit::pymin(x1, b.right() - 0.3));
        let (y0, y1) = (crate::circuit::pymax(y0, b.top() + 0.3), crate::circuit::pymin(y1, b.bottom() - 0.3));
        if x1 - x0 < 0.3 || y1 - y0 < 0.3 {
            continue;
        }
        cv.rect(x0, y0, x1, y1, "url(#keepout)", "#b0a58c", i(1), 0.15, " stroke-dasharray=\"4 3\"");
        cv.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, what, T::new(i(6)).fill("#7d7258").bold().halo(Some(BOARD)).late());
    }
}

fn footprint(cv: &mut Canvas, part: &Part) {
    if part.pins[0].xy.is_none() {
        return;
    }
    let (x0, y0, x1, y1) = placed_box(part, body_rect(part).unwrap());
    let big = part.kind == Kind::Pico || (part.kind == Kind::Kicad && crate::circuit::pymin(x1 - x0, y1 - y0) >= 6.0);
    let fill = match part.kind {
        Kind::Pico => "#2f6e3d",
        Kind::Kicad if big => "#2f6e3d",
        Kind::Dip | Kind::Kicad => "#24467a",
        _ => "#3d3d3d",
    };
    cv.rect(x0, y0, x1, y1, part.color.as_deref().filter(|c| !c.is_empty()).unwrap_or(fill), "#111", i(1), 0.15, "");
    if part.kind == Kind::Pico {
        let (a, b, cc, d) = placed_box(part, (-1.05, -4.5, 0.75, -2.5));
        cv.rect(a, b, cc, d, "#c9c9c9", "#6f6f6f", i(1), 0.08, "");
    }
    let rows: Vec<i64> = {
        let mut v: Vec<i64> = part.pins.iter().map(|p| p.offset.1 as i64).collect();
        v.sort();
        v.dedup();
        v
    };
    let inside = matches!(part.kind, Kind::Pico | Kind::Dip) || (part.kind == Kind::Kicad && rows.len() > 1);
    if inside {
        let dot = rotate((-0.36, 0.36), part.rot);
        let p0 = part.pins[0].xy.unwrap();
        cv.circle(p0.0 + dot.0, p0.1 + dot.1, 0.09, "#ffffff", "none", W1);
    }
    for pin in &part.pins {
        let (x, y) = pin.xy.unwrap();
        if pin.num == 1 {
            cv.rect(x - 0.19, y - 0.19, x + 0.19, y + 0.19, "#d9b44a", "#8a6d1c", f(0.6), 0.0, "");
        } else {
            cv.circle(x, y, 0.19, "#d9b44a", "#8a6d1c", f(0.6));
        }
        let (dx, dy) = rotate((pin.label_dir.0 as f64, pin.label_dir.1 as f64), part.rot);
        let fill = if inside { "#f1f1f1" } else { INK };
        let halo = if inside { None } else { Some("#ffffff") };
        if dx == 0.0 {
            cv.text(x, y + dy * 0.38, &pin.name, T::new(f(5.4)).anchor(if dy < 0.0 { "start" } else { "end" }).fill(fill).vertical(true).halo(halo));
        } else {
            cv.text(x + dx * 0.38, y, &pin.name, T::new(f(5.4)).anchor(if dx > 0.0 { "start" } else { "end" }).fill(fill).halo(halo));
        }
    }
    let label = format!("{} {}", part.r#ref, part.title).trim().to_string();
    if big {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        cv.text(cx, cy - 0.4, &part.r#ref, T::new(i(9)).fill("#ffffff").bold());
        cv.text(cx, cy + 0.45, &part.title, T::new(f(6.5)).fill("#dfe9df"));
    } else {
        let above = ((x0 + x1) / 2.0, y0 - 0.5, "middle");
        let below = ((x0 + x1) / 2.0, y1 + 0.55, "middle");
        let mut spots = if part.kind == Kind::Sip { vec![below, above] } else { vec![above, below] };
        for y in [spots[0].1, spots[1].1] {
            spots.push((x0, y, "start"));
            spots.push((x1, y, "end"));
        }
        for k in 0..2 {
            let (x, y, _) = spots[k];
            spots.push((x, y + if y > (y0 + y1) / 2.0 { 0.5 } else { -0.5 }, "middle"));
        }
        cv.label(&spots, &label, T::new(f(6.5)).bold().halo(Some("#ffffff")), &part.r#ref);
    }
}

fn lead(cv: &mut Canvas, part: &Part) {
    if part.pins.iter().any(|p| p.xy.is_none()) {
        return;
    }
    let ((ax, ay), (bx, by)) = (part.pins[0].xy.unwrap(), part.pins[1].xy.unwrap());
    let mut length = pyf::hypot(bx - ax, by - ay);
    if length == 0.0 {
        length = 1e-9;
    }
    let (ux, uy) = ((bx - ax) / length, (by - ay) / length);
    let (mut nx, mut ny) = (-uy, ux);
    if ny > 0.0 || (ny == 0.0 && nx > 0.0) {
        nx = -nx;
        ny = -ny;
    }
    if part.kind == Kind::Electrolytic {
        can(cv, part, (ax, ay), (bx, by), (ux, uy), (nx, ny));
        return;
    }
    if part.kind == Kind::Led {
        led(cv, part, (ax, ay), (bx, by), (ux, uy), (nx, ny));
        return;
    }
    let (fill, stroke, thick, longest) = lead_style(part.kind);
    let (cx, cy, blen) = if length < 1.8 {
        ((ax + bx) / 2.0 + nx * 0.5, (ay + by) / 2.0 + ny * 0.5, crate::circuit::pymin(longest, 0.9))
    } else {
        ((ax + bx) / 2.0, (ay + by) / 2.0, crate::circuit::pymin(longest, length - 0.6))
    };
    let e1 = (cx - ux * blen / 2.0, cy - uy * blen / 2.0);
    let e2 = (cx + ux * blen / 2.0, cy + uy * blen / 2.0);
    cv.line(ax, ay, e1.0, e1.1, "#8d8d8d", 0.1);
    cv.line(bx, by, e2.0, e2.1, "#8d8d8d", 0.1);
    let angle = uy.atan2(ux).to_degrees();
    let (w, h) = (blen * P, thick * P);
    let g = format!("<g transform=\"translate({:.1} {:.1}) rotate({:.1})\">", cv.x(cx), cv.y(cy), angle);
    cv.add(g);
    cv.add(format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>",
                   -w / 2.0, -h / 2.0, w, h, h / 3.0, part.color.as_deref().filter(|c| !c.is_empty()).unwrap_or(fill), stroke));
    let bands = if part.kind == Kind::Resistor { resistor_bands(&part.value) } else { None };
    if let Some(bands) = bands.filter(|b| !b.is_empty()) {
        let span = w - 2.0 * h / 3.0;
        let bw = crate::circuit::pymin(span / (bands.len() + 2) as f64, 3.4);
        let mut at: Vec<f64> = (0..bands.len() - 1).map(|k| -span / 2.0 + (k as f64 + 0.5) * span / (bands.len() + 1) as f64).collect();
        at.push(span / 2.0 - bw / 2.0);
        if ux < -1e-6 || (ux.abs() <= 1e-6 && uy < 0.0) {
            at = at.iter().map(|x| -x).collect();
        }
        for (x, band) in at.iter().zip(&bands) {
            cv.add(format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" stroke=\"#00000040\" stroke-width=\"0.5\"/>",
                           x - bw / 2.0, -h / 2.0 + 0.5, bw, h - 1.0, band));
        }
    }
    cv.add("</g>".into());
    if part.kind == Kind::Diode {
        let k = (e2.0 - ux * 0.18, e2.1 - uy * 0.18);
        cv.line(k.0 - nx * thick / 2.0, k.1 - ny * thick / 2.0, k.0 + nx * thick / 2.0, k.1 + ny * thick / 2.0, "#d9d9d9", 0.1);
    }
    for (x, y) in [(ax, ay), (bx, by)] {
        cv.circle(x, y, 0.13, "#9a9a9a", "#555", f(0.6));
    }
    part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), thick / 2.0 + 0.5, blen / 2.0);
}

fn part_label(cv: &mut Canvas, part: &Part, centre: Xy, u: Xy, n: Xy, off: f64, half: f64) {
    let ((cx, cy), (ux, uy), (nx, ny)) = (centre, u, n);
    let end = half + 0.35;
    let mut spots: Vec<(f64, f64, &str)> = vec![(cx + nx * off, cy + ny * off, "middle"), (cx - nx * off, cy - ny * off, "middle")];
    for k in [1.0f64, -1.0] {
        let (x, y) = (cx + ux * end * k, cy + uy * end * k);
        let anchor = if ux.abs() < 0.5 { "middle" } else if ux * k > 0.0 { "start" } else { "end" };
        let dy = if ux.abs() < 0.5 && uy * k > 0.0 { 0.35 } else if ux.abs() < 0.5 { -0.35 } else { 0.0 };
        spots.push((x, y + dy, anchor));
    }
    let label = format!("{} {}", part.r#ref, part.value).trim().to_string();
    cv.label(&spots, &label, T::new(f(6.2)).bold().halo(Some("#ffffff")), &part.r#ref);
}

fn shade(hex: &str, k: f64) -> String {
    let v = |j: usize| i64::from_str_radix(&hex[j..j + 2], 16).unwrap() as f64;
    let (r, g, b) = (v(1), v(3), v(5));
    let t = |x: f64| if k <= 1.0 { (x * k) as i64 } else { (x + (255.0 - x) * (k - 1.0)) as i64 };
    format!("#{:02x}{:02x}{:02x}", t(r), t(g), t(b))
}

fn legs(cv: &mut Canvas, a: Xy, b: Xy, mid: Xy) {
    for (x, y) in [a, b] {
        cv.line(x, y, mid.0, mid.1, LEG_EDGE, 0.17);
        cv.line(x, y, mid.0, mid.1, LEG, 0.09);
    }
}

fn leg_ends(cv: &mut Canvas, a: Xy, b: Xy) {
    for (x, y) in [a, b] {
        cv.circle(x, y, 0.14, LEG, LEG_EDGE, i(1));
    }
}

fn led(cv: &mut Canvas, part: &Part, a: Xy, b: Xy, u: Xy, n: Xy) {
    let ((ux, uy), (nx, ny)) = (u, n);
    let color = part.color.clone().filter(|c| !c.is_empty()).unwrap_or("red".into());
    let (lens, flange) = match led_colors(&color) {
        Some((l, f)) => (l.to_string(), f.to_string()),
        None => (shade(&color, 1.25), shade(&color, 0.7)),
    };
    let (cx, cy, r) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, 0.75f64);
    legs(cv, a, b, (cx, cy));
    let d = r * 0.78;
    let h = (r * r - d * d).sqrt();
    let p1 = (cx + ux * d + nx * h, cy + uy * d + ny * h);
    let p2 = (cx + ux * d - nx * h, cy + uy * d - ny * h);
    let sweep = if ux * ny - uy * nx > 0.0 { 1 } else { 0 };
    cv.add(format!("<path d=\"M {:.1} {:.1} A {:.1} {:.1} 0 1 {} {:.1} {:.1} Z\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>",
                   cv.x(p1.0), cv.y(p1.1), r * P, r * P, sweep, cv.x(p2.0), cv.y(p2.1), flange, shade(&flange, 0.6)));
    cv.circle(cx, cy, r * 0.72, &lens, &shade(&flange, 0.8), f(0.8));
    cv.circle(cx - r * 0.25, cy - r * 0.25, r * 0.18, "#ffffff", "none", i(0));
    leg_ends(cv, a, b);
    part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), r + 0.45, r);
}

fn can(cv: &mut Canvas, part: &Part, a: Xy, b: Xy, u: Xy, n: Xy) {
    let ((ux, uy), (nx, ny)) = (u, n);
    let (cx, cy, r) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, 0.75f64);
    legs(cv, a, b, (cx, cy));
    cv.circle(cx, cy, r, part.color.as_deref().filter(|c| !c.is_empty()).unwrap_or("#1c1c1c"), "#000000", i(1));
    let (ang, spread) = (uy.atan2(ux), 40f64.to_radians());
    let (x1, y1) = (cx + r * (ang - spread).cos(), cy + r * (ang - spread).sin());
    let (x2, y2) = (cx + r * (ang + spread).cos(), cy + r * (ang + spread).sin());
    cv.add(format!("<path d=\"M {:.1} {:.1} L {:.1} {:.1} A {:.1} {:.1} 0 0 1 {:.1} {:.1} Z\" fill=\"#ececec\"/>",
                   cv.x(cx), cv.y(cy), cv.x(x1), cv.y(y1), r * P, r * P, cv.x(x2), cv.y(y2)));
    cv.circle(cx, cy, r * 0.7, "#c9cdd2", "#8e959c", f(0.8));
    cv.text(cx + ux * r * 0.86, cy + uy * r * 0.86, "\u{2212}", T::new(i(6)).fill("#1c1c1c").bold());
    let arm = r * 0.42;
    for (dx, dy) in [(1.0f64, 1.0f64), (1.0, -1.0)] {
        let k = arm / 2f64.sqrt();
        cv.line(cx - dx * k, cy - dy * k, cx + dx * k, cy + dy * k, "#8e959c", 0.05);
    }
    leg_ends(cv, a, b);
    part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), r + 0.45, r);
}

fn offboard(cv: &mut Canvas, part: &Part) {
    let (x0, y0, x1, _) = part.r#box.unwrap();
    let bx = part.r#box.unwrap();
    cv.rect(bx.0, bx.1, bx.2, bx.3, "#2b2b2b", "#111", i(1), 0.12, "");
    cv.text((x0 + x1) / 2.0, y0 - 0.7, &part.title, T::new(i(7)).bold());
    for pin in &part.pins {
        let (x, y) = pin.xy.unwrap();
        cv.rect(x - 0.18, y - 0.18, x + 0.18, y + 0.18, "#d9b44a", "#8a6d1c", f(0.6), 0.0, "");
        let (dx, dy) = pin.label_dir;
        let warn = pin.name.ends_with('!');
        let (fill, weight) = if warn { ("#c62828", "bold") } else { (INK, "normal") };
        let text = if pin.name == pin.num.to_string() {
            pin.name.clone()
        } else if dx > 0 || (dx == 0 && dy < 0) {
            format!("{} {}", pin.num, pin.name)
        } else {
            format!("{} {}", pin.name, pin.num)
        };
        if dx == 0 {
            cv.text(x, y + dy as f64 * 0.85, &text, T::new(i(6)).anchor(if dy > 0 { "end" } else { "start" }).fill(fill).weight(weight)
                .vertical(true).halo(Some("#ffffff")).late());
        } else {
            cv.text(x + dx as f64 * 0.85, y, &text, T::new(i(6)).anchor(if dx > 0 { "start" } else { "end" }).fill(fill).weight(weight)
                .halo(Some("#ffffff")).late());
        }
    }
}

enum Path {
    Lines,
    Curve(f64, f64, f64, f64, f64, f64),
}

fn wire_path(c: &Circuit, w: &Wire) -> (Path, Vec<Xy>, Xy) {
    let pts = c.wire_points(w);
    if pts.len() > 2 {
        let k = pts.len() / 2;
        let mid = ((pts[k - 1].0 + pts[k].0) / 2.0, (pts[k - 1].1 + pts[k].1) / 2.0);
        return (Path::Lines, pts, mid);
    }
    let ((ax, ay), (bx, by)) = (pts[0], pts[1]);
    let length = pyf::hypot(bx - ax, by - ay);
    let off_board = w.a.as_ref().unwrap().pin.is_some() || w.b.as_ref().unwrap().pin.is_some();
    let mut bend = if off_board && length > 2.0 {
        0.12 * length
    } else if w.flex {
        crate::circuit::pymin(crate::circuit::pymax(0.18 * length, 0.8), 2.0)
    } else {
        0.0
    };
    bend *= if w.num % 2 != 0 { 1.0 } else { -1.0 };
    let (nx, ny) = if length != 0.0 { (-(by - ay) / length, (bx - ax) / length) } else { (0.0, 0.0) };
    let (qx, qy) = ((ax + bx) / 2.0 + nx * bend, (ay + by) / 2.0 + ny * bend);
    let mid = (0.25 * ax + 0.5 * qx + 0.25 * bx, 0.25 * ay + 0.5 * qy + 0.25 * by);
    (Path::Curve(ax, ay, qx, qy, bx, by), pts, mid)
}

fn wire_d(cv: &Canvas, c: &Circuit, w: &Wire) -> (String, Vec<Xy>, Xy) {
    let (path, pts, mid) = wire_path(c, w);
    match path {
        Path::Curve(ax, ay, qx, qy, bx, by) => (format!("M {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}", cv.x(ax), cv.y(ay), cv.x(qx), cv.y(qy), cv.x(bx), cv.y(by)), pts, mid),
        Path::Lines => ("M ".to_string() + &pts.iter().map(|(x, y)| format!("{:.1} {:.1}", cv.x(*x), cv.y(*y))).collect::<Vec<_>>().join(" L "), pts, mid),
    }
}

/// The point a fraction t along a wire as drawn.
fn along(c: &Circuit, w: &Wire, t: f64) -> Xy {
    let (path, pts, _) = wire_path(c, w);
    if let Path::Curve(ax, ay, qx, qy, bx, by) = path {
        let s = (1.0 - t).powf(2.0);
        return (s * ax + 2.0 * (1.0 - t) * t * qx + t * t * bx, s * ay + 2.0 * (1.0 - t) * t * qy + t * t * by);
    }
    let lengths: Vec<f64> = pts.windows(2).map(|s| pyf::dist(s[0], s[1])).collect();
    let mut goal = t * pyf::fsum(lengths.iter().copied());
    let last = *lengths.last().unwrap();
    for (s, &d) in pts.windows(2).zip(&lengths) {
        let (p, q) = (s[0], s[1]);
        if goal <= d || d == last {
            let k = if d != 0.0 { goal / d } else { 0.0 };
            return (p.0 + (q.0 - p.0) * k, p.1 + (q.1 - p.1) * k);
        }
        goal -= d;
    }
    *pts.last().unwrap()
}

fn wires(cv: &mut Canvas, c: &Circuit, result: &Result, highlight: Option<&str>) {
    let mut order: Vec<&Wire> = c.wires.iter().filter(|w| w.a.is_some() && w.b.is_some()).collect();
    order.sort_by_key(|w| (highlight.is_some() && Some(w.group.as_str()) == highlight, w.flex));
    let mut drawn: Vec<(&Wire, Xy, String)> = Vec::new();
    for w in order {
        let dim = highlight.is_some() && Some(w.group.as_str()) != highlight;
        let color = color_hex(&result.wire_color(c, w));
        let (d, pts, mid) = wire_d(cv, c, w);
        cv.add(format!("<g opacity=\"{}\">", if dim { "0.14" } else { "1" }));
        for (stroke, width) in [(outline(&color), 0.34), (color.as_str(), 0.24)] {
            cv.add(format!("<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                           d, stroke, width * P));
        }
        for (x, y) in [pts[0], *pts.last().unwrap()] {
            if w.flex {
                cv.rect(x - 0.2, y - 0.2, x + 0.2, y + 0.2, "#2b2b2b", &color, f(1.6), 0.05, "");
            } else {
                cv.circle(x, y, 0.15, &color, outline(&color), i(1));
            }
        }
        cv.add("</g>".into());
        if !dim {
            drawn.push((w, mid, color));
        }
    }
    for (w, (x, y), color) in drawn {
        let label = format!("W{}", w.num);
        let half = (label.chars().count() as f64 * 5.2 * 0.62 + 4.0) / 2.0 / P;
        let mut spots: Vec<Xy> = vec![(x, y)];
        for t in [0.3, 0.7, 0.2, 0.8, 0.4, 0.6, 0.1, 0.9] {
            spots.push(along(c, w, t));
        }
        let (a, b) = (along(c, w, 0.45), along(c, w, 0.55));
        let side = if (b.0 - a.0).abs() >= (b.1 - a.1).abs() { (0.0, 0.5) } else { (half + 0.3, 0.0) };
        spots.push((x + side.0, y + side.1));
        spots.push((x - side.0, y - side.1));
        let mut best = 0;
        let mut bk = (f64::INFINITY, usize::MAX);
        for p in spots.iter() {
            let idx = spots.iter().position(|q| q == p).unwrap(); // list.index: the first equal one
            let cl = cv.clash((p.0 - half, p.1 - 0.24, p.0 + half, p.1 + 0.24), &label) + 0.5 * ((idx > 8) as i32 as f64);
            if (cl, idx).partial_cmp(&bk) == Some(std::cmp::Ordering::Less) {
                bk = (cl, idx);
                best = idx;
            }
        }
        let (x, y) = spots[best];
        cv.rect(x - half, y - 0.24, x + half, y + 0.24, "#ffffff", &color, i(1), 0.1, " opacity=\"0.92\"");
        cv.labels.push(((x - half, y - 0.24, x + half, y + 0.24), label.clone()));
        cv.text(x, y, &label, T::new(f(5.2)).bold());
    }
}

fn marks(cv: &mut Canvas, c: &Circuit, result: &Result) {
    for issue in &result.issues {
        let color = if issue.level == "error" { "#e01b1b" } else { "#f08a00" };
        let mut spots: Vec<Xy> = issue.pins.iter().filter_map(|&p| c.pin(p).xy).collect();
        spots.extend(issue.holes.iter().flatten().map(|&h| c.xy(h)));
        for (x, y) in spots {
            cv.circle(x, y, 0.5, "none", color, f(2.2));
        }
    }
}

const FOCUS: &str = "#ff8c00";

/// Ring what a step changed: parts by ref, wires by W<num>.
fn focus_rings(cv: &mut Canvas, c: &Circuit, focus: &[String]) {
    for name in focus {
        if let Some(&pi) = c.part_index.get(name) {
            let part = &c.parts[pi as usize];
            // the last body of that name, as a dict of them keeps
            let body = c.bodies.iter().rev().find(|(_, r)| r == name).map(|(b, _)| *b);
            let bx = if let Some(b) = body {
                b
            } else if part.kind.is_lead() && part.pins.iter().all(|p| p.xy.is_some()) {
                let xs: Vec<f64> = part.pins.iter().map(|p| p.xy.unwrap().0).collect();
                let ys: Vec<f64> = part.pins.iter().map(|p| p.xy.unwrap().1).collect();
                (crate::parts::min(&xs), crate::parts::min(&ys), crate::parts::max(&xs), crate::parts::max(&ys))
            } else if let Some(e) = part.extent {
                e
            } else {
                continue;
            };
            cv.rect(bx.0 - 0.6, bx.1 - 0.6, bx.2 + 0.6, bx.3 + 0.6, "none", FOCUS, i(3), 0.4, "");
            continue;
        }
        for w in &c.wires {
            if format!("W{}", w.num) == *name && w.a.is_some() && w.b.is_some() {
                let (d, _, _) = wire_d(cv, c, w);
                cv.add(format!("<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\"0.45\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                               d, FOCUS, 0.8 * P));
            }
        }
    }
}

/// The drawing's SVG, and the canvas it was drawn on (whose labels the measures look at).
pub fn draw(c: &Circuit, result: &Result, highlight: Option<&str>) -> (String, Canvas) {
    draw_full(c, result, highlight, None, &[])
}

/// draw(), with a video frame's caption and the parts and wires the step changed.
pub fn draw_full(c: &Circuit, result: &Result, highlight: Option<&str>, caption: Option<&str>, focus: &[String]) -> (String, Canvas) {
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for b in &c.boards {
        xs.push(b.left());
        xs.push(b.right());
        ys.push(b.top());
        ys.push(b.bottom());
    }
    for part in &c.parts {
        if let Some(e) = part.extent {
            xs.push(e.0);
            xs.push(e.2);
            ys.push(e.1);
            ys.push(e.3);
        }
    }
    for (bx, _) in c.bodies.iter().chain(c.raised.iter()) {
        xs.push(bx.0);
        xs.push(bx.2);
        ys.push(bx.1);
        ys.push(bx.3);
    }
    let (minx, maxx) = (crate::parts::min(&xs) - 1.0, crate::parts::max(&xs) + 1.0);
    let (miny, maxy) = (crate::parts::min(&ys) - HEADER, crate::parts::max(&ys) + 1.0);
    let mut cv = Canvas { x0: minx, y0: miny, out: vec![], late: vec![], labels: vec![], spots: vec![], segs: vec![] };
    let legend_y = maxy + 0.6;
    let mut entries: Vec<(&str, String)> = c.nets.values().map(|n| ("net", n.name.clone())).collect();
    if c.wires.iter().any(|w| w.flex) {
        entries.push(("flex", "flexible jumper (plug in last)".into()));
    }
    let mut legend: Vec<(f64, i64, &str, String)> = Vec::new();
    let (mut x, mut line) = (minx + 0.8, 0i64);
    for (kind, text) in entries {
        let size = 1.4 + text.chars().count() as f64 * 0.24 + 1.2;
        if x + size > maxx && x > minx + 0.8 {
            x = minx + 0.8;
            line += 1;
        }
        legend.push((x, line, kind, text));
        x += size;
    }
    let width = (maxx - minx) * P;
    let height = (legend_y + 1.6 + 0.8 * line as f64 - miny) * P;
    let (errors, warnings) = (result.errors(), result.warnings());
    let mut status = if errors == 0 { "wiring check passed".to_string() } else { format!("{} error{}", errors, if errors != 1 { "s" } else { "" }) };
    if warnings > 0 {
        status += &format!(", {} warning{}", warnings, if warnings != 1 { "s" } else { "" });
    }
    let mut sub = format!("{} parts · {} wires · {}", c.parts.len(), c.wires.len(), status);
    if let Some(h) = highlight {
        sub += &format!(" · step: {}", h);
    }
    cv.text(minx + 0.8, miny + 1.1, &c.title, T::new(i(15)).anchor("start").bold());
    cv.text(minx + 0.8, miny + 2.3, &sub, T::new(i(8)).anchor("start").fill(if errors > 0 { "#c62828" } else { "#555" }));
    if let Some(cap) = caption.filter(|s| !s.is_empty()) {
        cv.text(maxx - 0.8, miny + 1.1, cap, T::new(i(11)).anchor("end").fill("#b35c00").bold());
    }
    for w in &c.wires {
        if w.a.is_some() && w.b.is_some() {
            let pts = c.wire_points(w);
            cv.spots.push(pts[0]);
            cv.spots.push(*pts.last().unwrap());
            cv.spots.extend(w.via_xy.iter().copied());
            for s in pts.windows(2) {
                cv.segs.push((s[0], s[1], format!("W{}", w.num)));
            }
        }
    }
    for b in &c.boards {
        board(&mut cv, c, result, b);
    }
    for part in &c.parts {
        keepouts(&mut cv, c, part);
    }
    for part in &c.parts {
        if part.kind.is_footprint() && !part.raised {
            footprint(&mut cv, part);
        } else if part.kind == Kind::Offboard {
            offboard(&mut cv, part);
        }
    }
    for part in &c.parts {
        if part.kind.is_lead() {
            lead(&mut cv, part);
        }
    }
    wires(&mut cv, c, result, highlight);
    for part in &c.parts {
        if part.kind.is_footprint() && part.raised {
            cv.add("<g opacity=\"0.78\">".into());
            footprint(&mut cv, part);
            cv.add("</g>".into());
        }
    }
    focus_rings(&mut cv, c, focus);
    let late = std::mem::take(&mut cv.late);
    cv.out.extend(late);
    marks(&mut cv, c, result);
    for (x, line, kind, text) in &legend {
        let y = legend_y + 0.8 * *line as f64;
        if *kind == "net" {
            let swatch = color_hex(&c.nets[text.as_str()].color);
            cv.rect(*x, y - 0.25, x + 0.9, y + 0.25, &swatch, outline(&swatch), i(1), 0.1, "");
        } else {
            let svg = format!("<path d=\"M {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}\" fill=\"none\" stroke=\"#777777\" stroke-width=\"{:.1}\"/>",
                              cv.x(x + 0.2), cv.y(y), cv.x(x + 0.6), cv.y(y - 0.7), cv.x(x + 1.0), cv.y(y), 0.2 * P);
            cv.add(svg);
            for gx in [x + 0.2, x + 1.0] {
                cv.rect(gx - 0.2, y - 0.2, gx + 0.2, y + 0.2, "#2b2b2b", "#777777", f(1.6), 0.05, "");
            }
        }
        cv.text(x + 1.4, y, text, T::new(f(6.5)).anchor("start"));
    }
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {:.0} {:.0}\" font-family=\"{}\">\n\
                       <defs><pattern id=\"keepout\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\">\
                       <rect width=\"8\" height=\"8\" fill=\"#efe9dc\"/><line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"8\" stroke=\"#d3c9b1\" stroke-width=\"3\"/>\
                       </pattern></defs>\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n{}\n</svg>\n",
                      width, height, width, height, FONT, cv.out.join("\n"));
    (svg, cv)
}

#[allow(dead_code)]
fn _rows(_: Row) {}
