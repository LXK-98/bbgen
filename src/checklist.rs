//! The build checklist: parts first, then the wires group by group, then any raised modules (over the wires
//! laid under them), then any flexible jumpers.

use crate::board::{Row, RAILS};
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

fn part_line(c: &Circuit, part: &Part) -> String {
    let name = [part.title.as_str(), part.value.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    let name = if name.is_empty() { part.kind.name().to_string() } else { name };
    let raised = if part.raised { "; it stands on its headers, over the wires that run under it" } else { "" };
    format!("- [ ] **{}** {}: {}{}", part.r#ref, name, where_(c, part), raised)
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
    let raised: Vec<&Part> = c.parts.iter().filter(|p| p.raised && p.kind != Kind::Offboard).collect();
    out.extend(["".into(), "## Parts".into(), "".into(), "Place these first.".into(), "".into()]);
    for part in &c.parts {
        if !raised.iter().any(|r| r.r#ref == part.r#ref) {
            out.push(part_line(c, part));
        }
    }
    out.extend(["".into(), "## Wires".into()]);
    let flex: Vec<&Wire> = c.wires.iter().filter(|w| w.flex).collect();
    let mut sections: Vec<(String, Vec<&Wire>)> = c.groups.iter()
        .map(|g| (format!("### {}", g), c.wires.iter().filter(|w| w.group == *g && !w.flex).collect())).collect();
    sections.push(("## Flexible jumpers\n\nPlug these in last: they arch over the rest.".into(), flex));
    for (head, wires) in sections {
        if head.starts_with("## Flexible") && !raised.is_empty() {
            out.extend(["".into(), "## Raised modules".into(), "".into(),
                        "Put these in now, standing on their headers over the wires laid under them.".into(), "".into()]);
            out.extend(raised.iter().map(|p| part_line(c, p)));
        }
        if wires.is_empty() {
            continue;
        }
        out.push(String::new());
        out.push(if head.starts_with("###") { format!("{} ({})", head, wires.len()) } else { head.clone() });
        out.push(String::new());
        for w in wires {
            let (a, b) = match (&w.a, &w.b) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    out.push(format!("- [ ] **W{}** `{}` → `{}`: could not be placed (see Check)", w.num, w.spec.0, w.spec.1));
                    continue;
                }
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
            out.push(format!("- [ ] **W{}** {}: `{}` → `{}`{} · {} ({} ↔ {}){}", w.num, result.wire_color(c, w), name(a), name(b), bends,
                             nets, end(c, result, a), end(c, result, b), beneath));
        }
    }
    out.join("\n") + "\n"
}
