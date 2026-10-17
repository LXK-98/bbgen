//! The build checklist, in building order (`steps`): wires that parts go over, chips and modules, small parts,
//! the wires, flexible jumpers, and last the flying leads to parts off the board.

use crate::board::{Row, Xy, RAILS};
use crate::check::Result;
use crate::circuit::{Circuit, End, Node, Wire};
use crate::parts::{Kind, Part};
use crate::pyf;

fn points(rot: i32) -> &'static str {
    match rot {
        90 => "up",
        180 => "right",
        270 => "down",
        _ => "left",
    }
}

fn runs(rot: i32) -> &'static str {
    match rot {
        90 => "downward",
        180 => "to the left",
        270 => "upward",
        _ => "to the right",
    }
}

/// Python's `{x:g}` for the small numbers it's used on here.
fn g(x: f64) -> String {
    let s = pyf::repr(x);
    s.strip_suffix(".0").map(String::from).unwrap_or(s)
}

fn where_(c: &Circuit, part: &Part) -> String {
    let first = part.pins.first().and_then(|p| p.hole);
    let at = match first {
        Some(h) => format!("`{}`", c.hole_name(h)),
        None => format!("`{}` (not placed)", part.at.clone().unwrap_or("None".into())),
    };
    match part.kind {
        Kind::Pico => format!("pin 1 in {}, USB pointing {}{}", at, points(part.rot), keep_clear(c, part)),
        Kind::Dip => format!("pin 1 in {}, notch (pin-1 end) pointing {}; {} pins, rows {} in apart{}", at, points(part.rot),
                             part.pins.len(), g(part.width as f64 / 10.0),
                             if part.adapter.is_empty() { String::new() } else { format!(". ASSUMED: {}", part.adapter) }),
        Kind::Kicad => {
            let end = if part.source.contains("Package_DIP:") { "notch (pin-1 end)" } else { "pin-1 end" };
            format!("pin 1 in {}, {} pointing {}{} ({})", at, end, points(part.rot), keep_clear(c, part), part.source)
        }
        Kind::Sip => format!("pin 1 in {}, pins running {}", at, runs(part.rot)),
        k if k.is_lead() => {
            let legs: Vec<String> = part.pins.iter().map(|p| p.hole.map(|h| format!("`{}`", c.hole_name(h))).unwrap_or("?".into())).collect();
            let polarity = match k {
                Kind::Electrolytic => " (+ leg first)",
                Kind::Diode | Kind::Led => " (anode first)",
                _ => "",
            };
            format!("legs in {} and {}{}", legs[0], legs[1], polarity)
        }
        _ => "off the breadboard: reached with flying leads".into(),
    }
}

/// '; keep e1-e5 and f1-f5 clear for its USB cable', for the holes a part's keep-outs reserve.
fn keep_clear(c: &Circuit, part: &Part) -> String {
    let mut out = String::new();
    for (what, _) in &part.keepouts {
        let who = format!("{}'s {}", part.r#ref, what);
        let mut rows: Vec<(Row, Vec<i32>)> = Vec::new();
        for (h, w) in &c.occupied {
            if *w == who {
                match rows.iter_mut().find(|(r, _)| *r == h.row) {
                    Some(slot) => slot.1.push(h.col),
                    None => rows.push((h.row, vec![h.col])),
                }
            }
        }
        rows.sort_by(|a, b| a.0.name().cmp(b.0.name()));
        let spans: Vec<String> = rows.iter().map(|(r, cols)| {
            if cols.len() > 1 {
                format!("`{}{}`-`{}{}`", r.name(), cols.iter().min().unwrap(), r.name(), cols.iter().max().unwrap())
            } else {
                format!("`{}{}`", r.name(), cols[0])
            }
        }).collect();
        if !spans.is_empty() {
            out += &format!("; keep {} clear for its {}", spans.join(" and "), what);
        }
    }
    out
}

fn end(c: &Circuit, result: &Result, e: &End) -> String {
    if let Some(p) = e.pin {
        return c.pin_str(p);
    }
    let h = e.hole.unwrap();
    if h.is_rail() {
        return format!("{} rail", h.row.name());
    }
    let pins = result.strip_pins.get(&c.boards[h.board as usize].strip(h)).cloned().unwrap_or_default();
    let s = pins.iter().map(|&p| c.pin_str(p)).collect::<Vec<_>>().join(", ");
    if s.is_empty() { "jumper junction".into() } else { s }
}

/// A part's checklist line after its ref: `24LC256: pin 1 in `f30`, notch ...`.
pub fn part_text(c: &Circuit, part: &Part) -> String {
    let name = [part.title.as_str(), part.value.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    let name = if name.is_empty() { part.kind.name().to_string() } else { name };
    let raised = if part.raised { "; it stands on its headers, over the wires that run under it" } else { "" };
    format!("{}: {}{}", name, where_(c, part), raised)
}

/// A wire's checklist line after its number: `blue: `b22` → `b33` · SDA (Pico.21 GP16 ↔ U1.5 SDA)`.
pub fn wire_text(c: &Circuit, result: &Result, w: &Wire) -> String {
    let (a, b) = match (&w.a, &w.b) {
        (Some(a), Some(b)) => (a, b),
        _ => return format!("`{}` → `{}`: could not be placed (see Check)", w.spec.0, w.spec.1),
    };
    let name = |e: &End| e.hole.map(|h| c.hole_name(h)).unwrap_or_else(|| c.pin_str(e.pin.unwrap()));
    let nets = result.wire_nets(c, w).join(" / ");
    let nets = if nets.is_empty() { "no net".to_string() } else { nets };
    let mut bends = if w.via.is_empty() { String::new() } else {
        format!(", bent over {}", w.via.iter().map(|v| format!("`{}`", v)).collect::<Vec<_>>().join(", "))
    };
    if w.flex {
        bends = format!(", {} flexible jumper", jumper_length(c, w));
    }
    let under = c.under_of(w);
    let beneath = if under.is_empty() { String::new() } else {
        format!(" · runs under {}: lay it flat, before {} in", under.join(" and "), if under.len() > 1 { "they go" } else { "it goes" })
    };
    format!("{}: `{}` → `{}`{} · {} ({} ↔ {}){}", result.wire_color(c, w), name(a), name(b), bends,
            nets, end(c, result, a), end(c, result, b), beneath)
}

/// The build's stages in order: a step's section, the checklist's heading for it, and what it says first.
pub const SECTIONS: [(&str, &str, &str); 6] = [
    ("under", "Wires under parts", "Lay these flat first: the parts that go over them come next."),
    ("chips", "Chips and modules", "Left to right; a raised module last, over the wires laid under it."),
    ("parts", "Small parts", "Lowest first: resistors and diodes, capacitors, transistors, buttons and the like, LEDs, \
                              electrolytics. Like parts of one value together, left to right."),
    ("wires", "Wires", "Shortest first, so the longer ones lie over them."),
    ("flex", "Flexible jumpers", "Shortest first: they arch over the rest."),
    ("leads", "Flying leads", "Last, one part off the board at a time, in its pins' order."),
];

/// Where a thing sits, for building left to right: its board, then the leftmost and topmost of its points, in
/// hundredths of a pixel (so it sorts).
type Spot = (u8, i64, i64);

fn spot(boards: impl Iterator<Item = u8>, xys: impl Iterator<Item = Xy>) -> Spot {
    let (mut x, mut y) = (f64::INFINITY, f64::INFINITY);
    for p in xys {
        x = x.min(p.0);
        y = y.min(p.1);
    }
    let h = |v: f64| if v.is_finite() { (v * 100.0).round() as i64 } else { i64::MAX };
    (boards.min().unwrap_or(u8::MAX), h(x), h(y))
}

fn part_spot(part: &Part) -> Spot {
    spot(part.pins.iter().filter_map(|p| p.hole.map(|h| h.board)), part.pins.iter().filter_map(|p| p.xy))
}

/// A part's stage: chips and modules (more than four pins, a body ten holes long, or raised) are `chips`; the rest
/// on the board (two-lead parts, transistors, regulators, pots, buttons) are `parts`; off-board parts go with their
/// flying leads.
fn part_section(part: &Part) -> &'static str {
    let long = crate::parts::body_rect(part).map(|r| (r.2 - r.0).max(r.3 - r.1) >= 10.0).unwrap_or(false);
    match part.kind {
        Kind::Offboard => "leads",
        k if k.is_lead() => "parts",
        Kind::Pico => "chips",
        _ if part.raised || part.pins.len() > 4 || long => "chips",
        _ => "parts",
    }
}

/// How high a small part stands, lowest first: they go in in this order, so fingers reach past what's in.
fn height(kind: Kind) -> u8 {
    match kind {
        Kind::Resistor | Kind::Diode => 0,
        Kind::Capacitor => 1,
        Kind::Led => 3,
        Kind::Electrolytic => 4,
        _ => 2,
    }
}

/// A wire's length along its path, in pixels; unplaced wires last.
fn length(c: &Circuit, w: &Wire) -> i64 {
    if w.a.is_none() || w.b.is_none() {
        return i64::MAX;
    }
    let pts = c.wire_points(w);
    (pts.windows(2).map(|s| pyf::dist(s[0], s[1])).sum::<f64>() * 100.0).round() as i64
}

fn wire_spot(c: &Circuit, w: &Wire) -> Spot {
    let ends: Vec<&End> = [&w.a, &w.b].into_iter().flatten().collect();
    spot(ends.iter().filter_map(|e| e.hole.map(|h| h.board)), ends.iter().map(|e| c.end_xy(e)))
}

/// The checklist as steps, in building order: (section, what it's about, its text). Parts are `part:<ref>`, wires
/// `wire:W<num>`; the sections are SECTIONS' and come in their order:
///
/// - `under`: flat wires a part goes over (a raised module, or a two-lead part lying across them), by the part
///   they're under in the order the parts go in, then shortest first.
/// - `chips`: left to right, raised modules last (they stand over what's laid before them).
/// - `parts`: the lowest first (`height`), like parts of one value together (in the order the leftmost of each
///   value comes), left to right.
/// - `wires`, `flex`: shortest first, left to right among equals. Jumpers between boards are `flex`.
/// - `leads`: each off-board part, left to right, then its leads in its pins' order.
pub fn steps(c: &Circuit, result: &Result) -> Vec<(String, String, String)> {
    let part = |p: &Part, section: &str| (section.to_string(), format!("part:{}", p.r#ref), part_text(c, p));
    let wire = |w: &Wire, section: &str| (section.to_string(), format!("wire:W{}", w.num), wire_text(c, result, w));

    let mut chips: Vec<&Part> = c.parts.iter().filter(|p| part_section(p) == "chips").collect();
    chips.sort_by_key(|p| (p.raised, part_spot(p)));
    let mut small: Vec<&Part> = c.parts.iter().filter(|p| part_section(p) == "parts").collect();
    let first_of_value = |p: &Part| small.iter().filter(|q| q.kind == p.kind && q.value == p.value).map(|q| part_spot(q)).min();
    let mut keyed: Vec<(_, &Part)> = small.iter().map(|p| ((height(p.kind), first_of_value(p), part_spot(p)), *p)).collect();
    keyed.sort_by_key(|k| k.0);
    small = keyed.into_iter().map(|k| k.1).collect();
    let order: Vec<&str> = chips.iter().chain(small.iter()).map(|p| p.r#ref.as_str()).collect();
    let rank = |r: &str| order.iter().position(|x| *x == r).unwrap_or(usize::MAX);

    // the first part (in building order) each flat wire goes under
    let below = |w: &Wire| -> Option<usize> {
        if w.flex || !w.on_board() {
            return None;
        }
        c.under_of(w).iter().map(|r| rank(r)).min()
    };
    let offboard = |w: &Wire| [&w.a, &w.b].into_iter().flatten().find_map(|e| e.pin).filter(|p| c.parts[p.part as usize].kind == Kind::Offboard);

    let mut under: Vec<(usize, i64, Spot, &Wire)> = Vec::new();
    let (mut flat, mut flex): (Vec<(i64, Spot, &Wire)>, Vec<(i64, Spot, &Wire)>) = (Vec::new(), Vec::new());
    let mut leads: Vec<(u16, i64, &Wire)> = Vec::new();
    for w in &c.wires {
        if let Some(p) = offboard(w).filter(|_| !w.on_board()) {
            leads.push((p.part, c.parts[p.part as usize].pins[p.pin as usize].num, w));
        } else if let Some(k) = below(w) {
            under.push((k, length(c, w), wire_spot(c, w), w));
        } else if w.flex {
            flex.push((length(c, w), wire_spot(c, w), w));
        } else {
            flat.push((length(c, w), wire_spot(c, w), w));
        }
    }
    under.sort_by_key(|x| (x.0, x.1, x.2));
    flat.sort_by_key(|x| (x.0, x.1));
    flex.sort_by_key(|x| (x.0, x.1));

    let mut out: Vec<(String, String, String)> = under.iter().map(|x| wire(x.3, "under")).collect();
    out.extend(chips.iter().map(|p| part(p, "chips")));
    out.extend(small.iter().map(|p| part(p, "parts")));
    out.extend(flat.iter().map(|x| wire(x.2, "wires")));
    out.extend(flex.iter().map(|x| wire(x.2, "flex")));
    let mut off: Vec<(Spot, usize)> = c.parts.iter().enumerate().filter(|(_, p)| p.kind == Kind::Offboard)
        .map(|(i, p)| (spot(std::iter::empty(), p.pins.iter().filter_map(|q| q.xy)), i)).collect();
    off.sort();
    for (_, i) in off {
        out.push(part(&c.parts[i], "leads"));
        let mut mine: Vec<&(u16, i64, &Wire)> = leads.iter().filter(|l| l.0 as usize == i).collect();
        mine.sort_by_key(|l| (l.1, l.2.num));
        out.extend(mine.iter().map(|l| wire(l.2, "leads")));
    }
    out
}

/// The stock length of flexible jumper that reaches.
fn jumper_length(c: &Circuit, w: &Wire) -> String {
    let span_cm = pyf::dist(c.end_xy(w.a.as_ref().unwrap()), c.end_xy(w.b.as_ref().unwrap())) * 0.254;
    for n in [10, 20, 30] {
        if span_cm <= 0.6 * n as f64 || n == 30 {
            return format!("{} cm", n);
        }
    }
    unreachable!()
}

pub fn checklist(c: &Circuit, result: &Result) -> String {
    let (errors, _) = (result.errors(), result.warnings());
    let name = std::path::Path::new(&c.source).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out: Vec<String> = vec![format!("# {}", c.title), String::new(),
                                    format!("Generated by bbgen from `{}`. Edit that file, not this one.", name), String::new()];
    out.push(if errors == 0 { "**Wiring check: passed.**".into() } else {
        format!("**Wiring check: {} error(s). Fix these before building.**", errors)
    });
    if !result.issues.is_empty() {
        out.extend(["".into(), "## Check".into(), "".into()]);
        out.extend(result.issues.iter().map(|i| format!("- **{}** `{}`: {}", i.level, i.code, i.message)));
    }
    let guessed: Vec<String> = c.nets.values().filter(|n| n.guessed).map(|n| {
        let p = n.polarity.unwrap();
        format!("- ASSUMED: `{}` is {}, from its name; say `rail: {}` (or `rail: none`) in the file to be sure", n.name,
                if p == '+' { "a supply (+ rail)" } else { "a ground (- rail)" }, p)
    }).collect();
    let notes: Vec<String> = c.notes.iter().map(|n| format!("- {}", n)).collect();
    if !notes.is_empty() || !guessed.is_empty() {
        out.extend(["".into(), "## Notes".into(), "".into()]);
        out.extend(notes);
        out.extend(guessed);
    }
    out.extend(["".into(), "## Boards".into(), "".into()]);
    for b in &c.boards {
        let mut rails: Vec<String> = Vec::new();
        for rail in RAILS {
            let mut nets: Vec<String> = Vec::new();
            for h in b.holes().into_iter().filter(|h| h.row == rail) {
                for n in result.nets_at(Node::Strip(b.strip(h))) {
                    if !nets.contains(&n) {
                        nets.push(n);
                    }
                }
            }
            nets.sort();
            rails.push(format!("`{}` {}", rail.name(), if nets.is_empty() { "unused".into() } else { nets.join(" / ") }));
        }
        let split = if b.split_rails { ", rails split in the middle" } else { "" };
        out.push(format!("- board {}: {}-size{}. Rails: {}", b.id, b.kind, split, rails.join(", ")));
    }
    let steps = steps(c, result);
    for (section, head, intro) in SECTIONS {
        let mine: Vec<&(String, String, String)> = steps.iter().filter(|s| s.0 == section).collect();
        if mine.is_empty() {
            continue;
        }
        out.extend(["".into(), format!("## {} ({})", head, mine.len()), "".into(), intro.into(), "".into()]);
        for (_, item, text) in mine {
            out.push(format!("- [ ] **{}** {}", item.split_once(':').unwrap().1, text));
        }
    }
    out.join("\n") + "\n"
}
