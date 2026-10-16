//! The circuit view: a schematic of the circuit, drawn from its parts and nets (docs/circuit-view-plan.md).
//!
//! Each part's symbol (symbol.rs) becomes a node of a graph, one node per unit, with its pins on signal nets as
//! ports fixed where the symbol has them. ELK's layered layout (elk.rs) places the nodes and routes the nets
//! between their ports. What makes it read like a schematic is decided here, before ELK sees it:
//!
//! - Supplies and grounds aren't wires: every pin on one ends in a power symbol, pointing the way the pin does,
//!   inside the room its node keeps for it.
//! - A two-lead part between two signals lies in line, the signal driving it on its left. One with a leg on a
//!   supply or ground stands up and hangs from its signal: a pull-up up to its supply, an LED down to ground.
//! - Signals flow left to right. Each net is drawn from the pin that drives it: an output (by KiCad's pin
//!   types), else the controller's pin, else a connector's, ... else the pin of the part nearest the controller.
//!   ELK merges a net's edges from that pin into one wire, with a dot where it branches.
//!
//! A port sits on its node's edge, where the pin points out of the node's box (the box takes in the symbol's
//! text and power symbols); the pin is drawn on to it.

use crate::circuit::{Circuit, PinId};
use crate::parts::Kind;
use crate::symbol::{self, Orient, PartSymbol, Prim, Xy, PX};
use serde_json::{json, Value};

/// A unit of a part's symbol, as a node of the graph.
struct Node {
    part: usize,
    /// U1, or U1A for a unit
    id: String,
    /// what it covers (mm, about the symbol's origin, turned): power symbols, text and caps beside it included
    bounds: (f64, f64, f64, f64),
    /// its pins on signal nets
    ports: Vec<Port>,
    /// what's drawn, about the symbol's origin: (the group's attributes, its prims)
    groups: Vec<(String, Vec<Prim>)>,
    /// where the symbol of a supply in `hang` would have gone (build)
    anchors: Vec<(usize, Xy)>,
    /// the symbol's body (mm, about its origin)
    body: (f64, f64, f64, f64),
    /// whether any of its pins is a net's label
    labels: bool,
    /// a supply's power symbol (the rails style), not a part
    power: bool,
}

struct Port {
    net: usize,
    at: Xy,
    out: (i32, i32),
    /// how strongly this pin drives its net
    drive: f64,
    id: String,
    /// the part's index and its pin's (usize::MAX for a power symbol or a label)
    pin: (usize, usize),
}

/// The nets drawn as labels: whole, or only at some pins (`apart`), the rest wired together with a label of their own.
#[derive(Clone)]
struct Labels {
    whole: Vec<bool>,
    /// pins (the part's index and its pin's) of a wired net drawn as its label instead
    apart: std::collections::HashSet<(usize, usize)>,
}

impl Labels {
    /// a net wired at some of its pins and labelled at the others
    fn split(&self, n: usize, ctx: &Ctx) -> bool {
        !self.whole[n] && self.apart.iter().any(|&(k, i)| ctx.net_of[k][i] == Some(n))
    }
}

/// The schematic: its SVG (in the drawing's colour, `currentColor`), its size in pixels, and the graph ELK laid out.
pub struct Schematic {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub graph: Value,
    pub laid_out: Value,
    /// what the drawing, read back, gets wrong (none, unless bbgen has a bug)
    pub problems: Vec<String>,
    /// the nets drawn as labels rather than wires
    pub labelled: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Signal,
    Supply,
    Ground,
}

/// How strongly a pin of this kind drives its net: outputs most, then a controller's pins, connectors' and
/// bidirectional ones, plain ones, a two-lead part's, inputs least.
fn drive(kind: Kind, source: &str, pin_kind: &str) -> f64 {
    match pin_kind {
        "output" | "power_out" | "tri_state" | "open_collector" | "open_emitter" => 5.0,
        _ if kind == Kind::Pico || source.starts_with("MCU_") => 4.0,
        _ if kind == Kind::Offboard => 3.0,
        "bidirectional" => 3.0,
        "input" => 0.0,
        _ if kind.is_lead() => 1.0,
        _ => 2.0,
    }
}

fn add(a: Xy, b: Xy) -> Xy {
    (a.0 + b.0, a.1 + b.1)
}

fn along(d: (i32, i32), k: f64) -> Xy {
    (d.0 as f64 * k, d.1 as f64 * k)
}

/// A power symbol pointing `dir` from `at`: a short line, then a bar with the supply's name, or the ground sign
/// (named unless it's GND).
fn flag(at: Xy, dir: (i32, i32), name: &str, ground: bool) -> Vec<Prim> {
    let thin = 0.1524;
    let stem = add(at, along(dir, 1.27));
    // narrower pointing sideways, so the symbols of neighbouring pins (a pitch apart) don't touch
    let half = if dir.0 != 0 { 0.95 } else { 1.27 };
    let side = |k: f64| (dir.1.abs() as f64 * k * half / 1.27, dir.0.abs() as f64 * k * half / 1.27);
    let mut prims = vec![Prim::Line { pts: vec![at, stem], width: thin, fill: symbol::Fill::None }];
    if ground {
        let tip = add(stem, along(dir, 1.27));
        prims.push(Prim::Line { pts: vec![add(stem, side(-1.27)), add(stem, side(1.27)), tip, add(stem, side(-1.27))], width: thin,
                                fill: symbol::Fill::None });
        if name != "GND" {
            prims.push(text_beyond(add(tip, along(dir, 0.5)), dir, name));
        }
    } else {
        prims.push(Prim::Line { pts: vec![add(stem, side(-1.27)), add(stem, side(1.27))], width: 0.254, fill: symbol::Fill::None });
        prims.push(text_beyond(add(stem, along(dir, 0.6)), dir, name));
    }
    prims
}

/// A net's label at the end of a pin: a short wire out, then its name in a tag pointing back at the pin, as KiCad
/// draws a global label.
fn label(at: Xy, out: (i32, i32), name: &str) -> Vec<Prim> {
    let thin = 0.1524;
    let tip = add(at, along(out, 1.27));
    let w = symbol::text_len(name, 1.27) + 1.0;
    let across = (out.1.abs() as f64, out.0.abs() as f64);
    let at_ = |k: f64, h: f64| add(tip, add(along(out, k), (across.0 * h, across.1 * h)));
    let tag = vec![tip, at_(0.8, -0.9), at_(0.8 + w, -0.9), at_(0.8 + w, 0.9), at_(0.8, 0.9), tip];
    let text_at = at_(1.2, 0.0);
    let text = match out {
        (1, _) => Prim::Text { at: (text_at.0, text_at.1 + 0.45), up: false, text: name.into(), size: 1.27, anchor: "start" },
        (-1, _) => Prim::Text { at: (text_at.0, text_at.1 + 0.45), up: false, text: name.into(), size: 1.27, anchor: "end" },
        (_, -1) => Prim::Text { at: (text_at.0 + 0.45, text_at.1), up: true, text: name.into(), size: 1.27, anchor: "start" },
        _ => Prim::Text { at: (text_at.0 + 0.45, text_at.1), up: true, text: name.into(), size: 1.27, anchor: "end" },
    };
    vec![Prim::Line { pts: vec![at, tip], width: thin, fill: symbol::Fill::None },
         Prim::Line { pts: tag, width: thin, fill: symbol::Fill::None }, text]
}

/// Text past a point, going on the way `out` points.
fn text_beyond(at: Xy, out: (i32, i32), text: &str) -> Prim {
    let size = 1.27;
    match out {
        (0, -1) => Prim::Text { at, up: false, text: text.into(), size, anchor: "middle" },
        (0, _) => Prim::Text { at: (at.0, at.1 + size * 0.75), up: false, text: text.into(), size, anchor: "middle" },
        (-1, _) => Prim::Text { at: (at.0, at.1 + size * 0.36), up: false, text: text.into(), size, anchor: "end" },
        _ => Prim::Text { at: (at.0, at.1 + size * 0.36), up: false, text: text.into(), size, anchor: "start" },
    }
}

/// How supplies and grounds are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// a power symbol at every pin, as in most schematics; decoupling caps hang from their chip's supply, and
    /// parts that only take power sit in a row of their own
    #[default]
    Symbols,
    /// as wires, from one power symbol per supply and ground: a textbook drawing, for small circuits
    Rails,
}

/// What every node is made from: the circuit, its symbols, its nets and what drives them.
struct Ctx<'a> {
    c: &'a Circuit,
    syms: Vec<PartSymbol>,
    classes: Vec<Class>,
    names: Vec<&'a str>,
    /// each part's pins' nets
    net_of: Vec<Vec<Option<usize>>>,
    /// each net's driving pin: the part's index and its pin's
    driver: Vec<Option<(usize, usize)>>,
    style: Style,
}

impl Ctx<'_> {
    /// a symbol pin is on the net of any of the part's pins it carries
    fn pin_net(&self, part: usize, sp: &symbol::SymPin) -> Option<usize> {
        sp.pins.iter().find_map(|&i| self.net_of[part][i])
    }

    fn drives(&self, n: usize, part: usize, sp: &symbol::SymPin) -> bool {
        self.driver[n].map(|(k, i)| k == part && sp.pins.contains(&i)).unwrap_or(false)
    }

    /// a net drawn as wires: a signal, or in the rails style any net
    fn wired(&self, n: usize) -> bool {
        self.classes[n] == Class::Signal || self.style == Style::Rails
    }
}

/// The schematic of a circuit, its supplies as power symbols.
pub fn draw(c: &Circuit) -> Result<Schematic, String> {
    draw_with(c, Style::Symbols)
}

/// The schematic of a circuit, its supplies drawn in this style.
pub fn draw_with(c: &Circuit, style: Style) -> Result<Schematic, String> {
    let library = c.library.as_deref();
    let syms: Vec<PartSymbol> = c.parts.iter().map(|p| symbol::of_part(p, library)).collect();
    let classes: Vec<Class> = c.nets.values().map(|n| match n.polarity {
        Some('+') => Class::Supply,
        Some('-') => Class::Ground,
        _ => Class::Signal,
    }).collect();
    let names: Vec<&str> = c.nets.keys().map(String::as_str).collect();
    let mut net_of: Vec<Vec<Option<usize>>> = c.parts.iter().map(|p| vec![None; p.pins.len()]).collect();
    for (n, net) in c.nets.values().enumerate() {
        for &PinId { part, pin } in &net.pins {
            net_of[part as usize][pin as usize] = Some(n);
        }
    }
    let mut ctx = Ctx { c, syms, classes, names, net_of, driver: vec![], style };

    // how far (in parts, along signal nets) each part is from what drives the circuit
    let strength: Vec<f64> = c.parts.iter().enumerate().map(|(k, p)| {
        ctx.syms[k].drawing.units.iter().flat_map(|u| &u.pins)
            .filter(|sp| ctx.pin_net(k, sp).map(|n| ctx.classes[n] == Class::Signal).unwrap_or(false))
            .map(|sp| drive(p.kind, &p.source, &sp.kind)).fold(0.0, f64::max)
    }).collect();
    let top = strength.iter().cloned().fold(0.0, f64::max).min(4.0);
    let mut dist: Vec<usize> = strength.iter().map(|&s| if s >= top { 0 } else { usize::MAX }).collect();
    loop {
        let mut changed = false;
        for (n, net) in c.nets.values().enumerate() {
            if ctx.classes[n] != Class::Signal {
                continue;
            }
            let best = net.pins.iter().map(|p| dist[p.part as usize]).min().unwrap_or(usize::MAX);
            if best == usize::MAX {
                continue;
            }
            for p in &net.pins {
                if dist[p.part as usize] > best + 1 {
                    dist[p.part as usize] = best + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    // each net's driving pin: the strongest (drive), then the nearest the controller, then the first in the file
    let mut driver: Vec<Option<(usize, usize)>> = vec![None; c.nets.len()];
    let mut best: Vec<(f64, i64, i64)> = vec![(-1.0, 0, 0); c.nets.len()];
    for (k, p) in c.parts.iter().enumerate() {
        for sp in ctx.syms[k].drawing.units.iter().flat_map(|u| &u.pins) {
            if let Some(n) = ctx.pin_net(k, sp) {
                let key = (drive(p.kind, &p.source, &sp.kind), -(dist[k].min(1 << 20) as i64), -(k as i64));
                if driver[n].is_none() || key > best[n] {
                    driver[n] = Some((k, sp.pins[0]));
                    best[n] = key;
                }
            }
        }
    }
    ctx.driver = driver;
    let ctx = ctx;
    let (names, classes) = (&ctx.names, &ctx.classes);

    // nets drawn as labels from the start: one with many pins (a supply without a rail, like 16's VDDIO), and
    // those of a connector with many pins (its wires would run all over the page)
    let whole: Vec<bool> = (0..names.len()).map(|n| {
        let pins = &c.nets[n].pins;
        let big_connector = pins.iter().any(|p| {
            let part = &c.parts[p.part as usize];
            part.kind == Kind::Offboard
                && ctx.net_of[p.part as usize].iter().flatten().filter(|&&m| classes[m] == Class::Signal).count() >= BIG_CONNECTOR
        });
        classes[n] == Class::Signal && (pins.len() >= MANY_PINS || big_connector)
    }).collect();
    let mut labelled = Labels { whole, apart: Default::default() };
    let mut round = 0;
    loop {
        let mut nodes = make_nodes(&ctx, &labelled);
        // parts that only take power (a supply's connector, a cap with no chip to stand by) go in a row of their
        // own above the drawing, rather than wherever the layout packs them
        let corner: Vec<Node> = if style == Style::Symbols {
            let (alone, rest): (Vec<Node>, Vec<Node>) = nodes.into_iter().partition(|n| n.ports.is_empty() && !n.labels && !n.power);
            nodes = rest;
            alone
        } else {
            vec![]
        };
        let graph = graph(&ctx, &nodes, &labelled);
        let laid_out = crate::elk::layout(&graph)?;
        // the power row: its nodes side by side under a caption, the laid-out drawing below it
        let pad = 20.0;
        let mut at: std::collections::HashMap<String, Xy> = std::collections::HashMap::new();
        let (mut row_w, mut row_h) = (0.0f64, 0.0f64);
        let mut caption = None;
        if !corner.is_empty() {
            let top = pad + 1.27 * PX * 1.6;
            let mut x = pad;
            for n in &corner {
                at.insert(n.id.clone(), (x, top));
                x += (n.bounds.2 - n.bounds.0) * PX + 30.0;
                row_h = row_h.max((n.bounds.3 - n.bounds.1) * PX);
            }
            row_w = x - 30.0 + pad;
            row_h += top + 10.0;
            caption = Some((pad, pad + 1.27 * PX));
        }
        for n in laid_out["children"].as_array().unwrap() {
            at.insert(n["id"].as_str().unwrap().to_string(), (n["x"].as_f64().unwrap(), n["y"].as_f64().unwrap() + row_h));
        }
        nodes.extend(corner);
        let mut drawn = route(&nodes, &laid_out, at, row_h);
        // a net wired at some pins and labelled at others has its name on its wire, where there's room
        let mut nameless = Vec::new();
        for n in (0..names.len()).filter(|&n| labelled.split(n, &ctx)) {
            match name_spot(&nodes, &drawn, n, names[n]) {
                Some(p) => drawn.names.push((n, p)),
                None => nameless.push(n),
            }
        }
        let problems = check(&drawn, names);
        let width = laid_out["width"].as_f64().unwrap_or(0.0).max(row_w);
        let height = laid_out["height"].as_f64().unwrap_or(0.0) + row_h;
        // what to label next: a net the check finds wrong, or one whose wires run across much of the page
        let long = |n: usize| drawn.length(n) > LONG * (width + height);
        // a long net stays wired if labelling it would leave a part with no wire at all: a part cut loose from the
        // circuit (the 555's timing resistors, with all their nets labels) reads worse than a long wire
        let mut more: Vec<usize> = (0..names.len()).filter(|&n| !labelled.whole[n] && (problems.iter().any(|(m, _)| *m == n) || nameless.contains(&n)))
            .collect();
        let mut would = labelled.whole.clone();
        for &n in &more {
            would[n] = true;
        }
        // (in the rails style, supplies stay wires however long: that's the style)
        // (a net wired at some pins only is long past SHORT for each wire joining two of them: it was split to keep
        // those close together)
        let joins = |n: usize| c.nets[n].pins.iter().filter(|p| !labelled.apart.contains(&(p.part as usize, p.pin as usize))).count()
            .saturating_sub(1).max(1) as f64;
        let long_ones: Vec<usize> = (0..names.len()).filter(|&n| !would[n] && classes[n] == Class::Signal)
            .filter(|&n| long(n) || (labelled.split(n, &ctx) && drawn.length(n) / joins(n) > SHORT * (width + height))).collect();
        let mut apart = Vec::new();
        for n in long_ones {
            let keeps = c.nets[n].pins.iter().all(|p| {
                let k = p.part as usize;
                ctx.net_of[k].iter().enumerate().any(|(i, m)| m.map(|m| m != n && ctx.wired(m) && !would[m]
                    && !labelled.apart.contains(&(k, i)) && !apart.contains(&(k, i))).unwrap_or(false))
            });
            if !keeps {
                continue;
            }
            // rather than all of it, a label at only the pins far from the rest, which stay wired (as one draws a long
            // net by hand): a pin wired to its neighbour reads better than both labelled
            let bus = (0..names.len()).any(|m| m != n && same_bus(names[m], names[n]));
            if !bus && !labelled.split(n, &ctx) {
                let cut: std::collections::HashSet<(usize, usize)> = labelled.apart.iter().chain(&apart).copied().collect();
                let far = far_pins(&ctx, &nodes, &drawn.at, n, width + height, &would, &cut);
                if !far.is_empty() {
                    apart.extend(far);
                    continue;
                }
            }
            would[n] = true;
            more.push(n);
        }
        round += 1;
        // BBGEN_SCHEMATIC_DEBUG: what each round found, to see why a net became a label
        if std::env::var("BBGEN_SCHEMATIC_DEBUG").is_ok() {
            for (_, p) in &problems {
                eprintln!("round {}: {}", round, p);
            }
            for n in (0..names.len()).filter(|&n| long(n) && !labelled.whole[n]) {
                eprintln!("round {}: {} is {:.0} px long, the page {:.0} px across and down", round, names[n], drawn.length(n), width + height);
            }
            for n in (0..names.len()).filter(|&n| labelled.split(n, &ctx)) {
                eprintln!("round {}: {}, wired at some pins, is {:.0} px long, the page {:.0} px across and down", round, names[n], drawn.length(n),
                          width + height);
            }
            for &(k, i) in &apart {
                eprintln!("round {}: {} labelled at {}.{} only", round, names[ctx.net_of[k][i].unwrap()], c.parts[k].r#ref, c.parts[k].pins[i].num);
            }
        }
        if (more.is_empty() && apart.is_empty()) || round == ROUNDS {
            let mut svg = render(&nodes, &drawn, names);
            if let Some((x, y)) = caption {
                svg.insert_str(0, &format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{:.1}\" fill=\"currentColor\" fill-opacity=\"0.6\">Power</text>",
                                           x, y, 1.27 * PX));
            }
            let problems = problems.into_iter().map(|(_, p)| p).collect();
            let labels = (0..names.len()).filter(|&n| labelled.whole[n] || labelled.split(n, &ctx)).map(|n| names[n].to_string()).collect();
            return Ok(Schematic { svg, width, height, graph, laid_out, problems, labelled: labels });
        }
        for n in more {
            labelled.whole[n] = true;
        }
        labelled.apart.extend(apart);
        // a bus (BUS0, BUS1, ...) is labelled all together or not at all
        for n in 0..names.len() {
            if !labelled.whole[n] && classes[n] == Class::Signal {
                labelled.whole[n] = (0..names.len()).any(|m| labelled.whole[m] && same_bus(names[m], names[n]));
            }
        }
    }
}

/// Where to write a net's name on its wire: above a level stretch with room for it, clear of other wires, symbols and
/// names, the longest such stretch; where the text starts (its baseline), or None.
fn name_spot(nodes: &[Node], d: &Drawn, n: usize, name: &str) -> Option<Xy> {
    let (w, h) = ((symbol::text_len(name, 1.27) + 1.0) * PX, 1.27 * PX);
    let boxes: Vec<(f64, f64, f64, f64)> = nodes.iter().map(|node| {
        let (x, y) = d.at[&node.id];
        (x, y, x + (node.bounds.2 - node.bounds.0) * PX, y + (node.bounds.3 - node.bounds.1) * PX)
    }).collect();
    let hits = |b: (f64, f64, f64, f64)| {
        let meets = |o: (f64, f64, f64, f64)| b.0 < o.2 && o.0 < b.2 && b.1 < o.3 && o.1 < b.3;
        boxes.iter().any(|&o| meets(o))
            || d.lines.iter().flat_map(|(_, pts)| pts.windows(2)).any(|s| meets((s[0].0.min(s[1].0) - 1.0, s[0].1.min(s[1].1) - 1.0,
                                                                                  s[0].0.max(s[1].0) + 1.0, s[0].1.max(s[1].1) + 1.0)))
            || d.names.iter().any(|(_, p)| meets((p.0, p.1 - h, p.0 + w, p.1 + 2.0)))
    };
    let mut best: Option<(f64, Xy)> = None;
    for s in d.lines.iter().filter(|(m, _)| *m == n).flat_map(|(_, pts)| pts.windows(2)) {
        let (a, b) = (s[0], s[1]);
        let len = (b.0 - a.0).abs();
        if (a.1 - b.1).abs() > 0.5 || len < w + 10.0 || best.map(|(l, _)| l >= len).unwrap_or(false) {
            continue;
        }
        let x = a.0.min(b.0) + (len - w) / 2.0;
        let at = (x, a.1 - 3.0);
        if !hits((at.0, at.1 - h, at.0 + w, at.1 - 1.0)) {
            best = Some((len, at));
        }
    }
    best.map(|(_, p)| p)
}

/// The pins of a long net to label rather than wire: the fewest, farthest from the rest, that leave the others close
/// together (joined, as the crow flies along x and y, in under SHORT of the page's width and height together). None if
/// that would leave fewer than two pins wired. The driving pin stays, and so does any pin whose part would have no
/// wire left.
fn far_pins(ctx: &Ctx, nodes: &[Node], at: &std::collections::HashMap<String, Xy>, n: usize, page: f64, would: &[bool],
            cut: &std::collections::HashSet<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut pins: Vec<((usize, usize), Xy, bool)> = Vec::new();
    for node in nodes {
        let (x, y) = at[&node.id];
        let o = (x / PX - node.bounds.0, y / PX - node.bounds.1);
        for p in node.ports.iter().filter(|p| p.net == n && p.pin.0 != usize::MAX) {
            let a = add(o, p.at);
            pins.push((p.pin, (a.0 * PX, a.1 * PX), p.drive > 0.0));
        }
    }
    if pins.len() < 3 {
        return vec![];
    }
    let driver = pins.iter().position(|p| p.2).unwrap_or(0);
    // (a pin already labelled at its end, `cut`, is no wire)
    let keeps = |k: usize| ctx.net_of[k].iter().enumerate()
        .any(|(i, m)| m.map(|m| m != n && ctx.wired(m) && !would[m] && !cut.contains(&(k, i))).unwrap_or(false));
    // the shortest tree joining them, along x and y
    let tree = |ps: &[usize]| {
        let mut joined = vec![ps[0]];
        let mut total = 0.0;
        let mut rest: Vec<usize> = ps[1..].to_vec();
        while !rest.is_empty() {
            let (j, d) = rest.iter().enumerate().map(|(j, &r)| {
                let d = joined.iter().map(|&q| (pins[q].1 .0 - pins[r].1 .0).abs() + (pins[q].1 .1 - pins[r].1 .1).abs()).fold(f64::INFINITY, f64::min);
                (j, d)
            }).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            total += d;
            joined.push(rest.remove(j));
        }
        total
    };
    let mut kept: Vec<usize> = (0..pins.len()).collect();
    let mut far = Vec::new();
    while tree(&kept) > SHORT * page {
        let best = kept.iter().copied().filter(|&i| i != driver && keeps(pins[i].0 .0))
            .map(|i| (i, tree(&kept.iter().copied().filter(|&j| j != i).collect::<Vec<_>>())))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) if kept.len() > 2 => {
                kept.retain(|&j| j != i);
                far.push(pins[i].0);
            }
            _ => return vec![],
        }
    }
    if kept.len() < 2 { vec![] } else { far }
}

/// Each part's units as nodes, turned and mirrored as they read best, with decoupling caps hanging from their
/// chips' supplies; and in the rails style, a node for each supply's power symbol.
fn make_nodes(ctx: &Ctx, labelled: &Labels) -> Vec<Node> {
    let (c, classes) = (ctx.c, &ctx.classes);
    let rails_style = ctx.style == Style::Rails;
    // decoupling caps (both legs on supplies) go by a chip: the one they're `near`, else the chip with a pin on their
    // supply that has fewest so far, chips before modules
    let mut beside: Vec<Option<usize>> = vec![None; c.parts.len()];
    let mut count = vec![0usize; c.parts.len()];
    for (k, p) in c.parts.iter().enumerate() {
        let legs: Vec<Option<usize>> = ctx.net_of[k].clone();
        let rails = legs.iter().all(|n| n.map(|n| classes[n] != Class::Signal).unwrap_or(false));
        if rails_style || !(p.kind.is_lead() && legs.len() == 2 && rails) {
            continue;
        }
        let near = p.near.as_ref().and_then(|r| c.parts.iter().position(|q| &q.r#ref == r));
        let supply = legs.iter().flatten().find(|&&n| classes[n] == Class::Supply).copied();
        let host = near.or_else(|| {
            let module = |j: usize| c.parts[j].kind == Kind::Pico || c.parts[j].source.starts_with("MCU_Module");
            (0..c.parts.len()).filter(|&j| !c.parts[j].kind.is_lead() && c.parts[j].kind != Kind::Offboard)
                .filter(|&j| ctx.net_of[j].iter().any(|n| n.is_some() && *n == supply)).min_by_key(|&j| (module(j), count[j], j))
        });
        if let Some(h) = host {
            beside[k] = Some(h);
            count[h] += 1;
        }
    }
    // a unit that only takes power (an op-amp's supply pins), when the part has others
    let power_unit = |k: usize, u: usize| {
        let pins = &ctx.syms[k].drawing.units[u].pins;
        !rails_style && ctx.syms[k].drawing.units.len() > 1 && pins.iter().any(|sp| ctx.pin_net(k, sp).is_some())
            && pins.iter().all(|sp| ctx.pin_net(k, sp).map(|n| classes[n] != Class::Signal).unwrap_or(true))
    };
    let supply_of = |j: usize| ctx.net_of[j].iter().flatten().find(|&&n| classes[n] == Class::Supply).copied();

    let mut nodes: Vec<Node> = Vec::new();
    for (k, p) in c.parts.iter().enumerate() {
        if beside[k].is_some() {
            continue;
        }
        let sym = &ctx.syms[k];
        for (u, unit) in sym.drawing.units.iter().enumerate() {
            let connected = unit.pins.iter().any(|sp| ctx.pin_net(k, sp).is_some());
            if (!connected || power_unit(k, u)) && sym.drawing.units.len() > 1 {
                continue;
            }
            let lead = p.kind.is_lead() && unit.pins.len() == 2;
            let mut orient = if lead { lead_orient(ctx, k, unit) } else { Orient::default() };
            if sym.source == "potentiometer" {
                // a pot's ends: the supply on top, the ground below
                let end = |num: &str| unit.pins.iter().find(|sp| sp.number == num).and_then(|sp| ctx.pin_net(k, sp)).map(|n| classes[n]);
                if end("1") == Some(Class::Ground) || end("3") == Some(Class::Supply) {
                    orient.rot = 180;
                }
            }
            // the caps that hang from this unit's supply pins
            let caps: Vec<usize> = if !nodes.iter().any(|n| n.part == k) {
                (0..c.parts.len()).filter(|&j| beside[j] == Some(k)).collect()
            } else {
                vec![]
            };
            let hang: Vec<usize> = caps.iter().filter_map(|&j| supply_of(j)).collect();
            let mut node = build(ctx, labelled, k, u, orient, &hang, None);
            if !lead && sym.source != "potentiometer" {
                // a symbol whose driving pins point left, or whose driven ones point right, is mirrored
                let lean: f64 = node.ports.iter().map(|p| if p.drive > 0.0 { p.out.0 as f64 } else { -p.out.0 as f64 }).sum();
                if lean < 0.0 {
                    orient.mirror = true;
                    node = build(ctx, labelled, k, u, orient, &hang, None);
                }
            }
            // decoupling caps on a supply line from the chip's pin, each to its own ground; the part's power unit
            // above its first unit drawn
            let first = !nodes.iter().any(|n| n.part == k);
            let mut above: Vec<(usize, usize)> = (0..sym.drawing.units.len()).filter(|&v| first && v != u && power_unit(k, v))
                .map(|v| (k, v)).collect();
            let anchors = node.anchors.clone();
            for &j in &caps {
                let s = supply_of(j);
                if !anchors.iter().any(|(n, _)| Some(*n) == s) {
                    above.push((j, 0));
                }
            }
            for (n, a) in anchors {
                let mine: Vec<usize> = caps.iter().copied().filter(|&j| supply_of(j) == Some(n)).collect();
                hang_caps(ctx, labelled, &mut node, n, a, &mine);
            }
            // anything else above it, at its right: its power unit, caps it has no supply pin on top for
            if !above.is_empty() {
                let mut x = node.bounds.2;
                let mut placed = Vec::new();
                for &(j, v) in above.iter().rev() {
                    let unit = &ctx.syms[j].drawing.units[v];
                    let o = if j == k { Orient::default() } else { lead_orient(ctx, j, unit) };
                    let extra = build(ctx, labelled, j, v, o, &[], None);
                    let b = extra.bounds;
                    x -= b.2 - b.0;
                    placed.push((extra, (x - b.0, node.bounds.1 - 1.27 - b.3)));
                    x -= 2.54;
                }
                for (extra, d) in placed {
                    for (attrs, prims) in extra.groups {
                        node.groups.push((attrs, shift(&prims, d)));
                    }
                }
                node.bounds = bounds_of(&node.groups);
            }
            nodes.push(node);
        }
    }
    if rails_style {
        // one power symbol for each supply and ground, the source of its wires
        for n in (0..ctx.names.len()).filter(|&n| classes[n] != Class::Signal && !labelled.whole[n]) {
            if !nodes.iter().any(|node| node.ports.iter().any(|p| p.net == n)) {
                continue;
            }
            let ground = classes[n] == Class::Ground;
            let way = if ground { (0, 1) } else { (0, -1) };
            let groups = vec![(format!("data-net=\"{}\"", symbol::escape(ctx.names[n])), flag((0.0, 0.0), way, ctx.names[n], ground))];
            let port = Port { net: n, at: (0.0, 0.0), out: (-way.0, -way.1), drive: 2.0, id: format!("power.{}", ctx.names[n]),
                              pin: (usize::MAX, 0) };
            nodes.push(Node { part: usize::MAX, id: format!("power:{}", ctx.names[n]), bounds: bounds_of(&groups), ports: vec![port],
                              groups, anchors: vec![], body: (0.0, 0.0, 0.0, 0.0), labels: false, power: true });
        }
    }
    nodes
}

/// Decoupling caps on the supply line from a chip's top pin(s) at `a`: the line goes up past the top of the caps,
/// the supply's symbol on its end, and across to the caps, which stand to the right of everything above the chip,
/// each down to its own ground.
fn hang_caps(ctx: &Ctx, labelled: &Labels, node: &mut Node, n: usize, a: Xy, caps: &[usize]) {
    let thin = 0.1524;
    let mut built = Vec::new();
    for &j in caps {
        let unit = &ctx.syms[j].drawing.units[0];
        let o = lead_orient(ctx, j, unit);
        let cap = build(ctx, labelled, j, 0, o, &[], Some(n));
        let top = unit.pins.iter().find(|sp| ctx.pin_net(j, sp) == Some(n)).map(|sp| o.point(sp.at)).unwrap_or((0.0, -3.81));
        built.push((cap, top));
    }
    // right of everything above the chip's body
    let body_top = node.body.1;
    let mut right = a.0;
    for (_, prims) in &node.groups {
        for p in prims {
            let b = symbol::bounds(std::slice::from_ref(p));
            if b.1 < body_top - 0.01 && b.2.is_finite() {
                right = right.max(b.2);
            }
        }
    }
    let drop = built.iter().map(|(cap, top)| cap.bounds.3 - top.1).fold(0.0, f64::max);
    let line_y = (a.1 - 1.27).min(body_top - 1.27 - drop);
    let t = (a.0, line_y);
    let attrs = format!("data-net=\"{}\"", symbol::escape(ctx.names[n]));
    let mut prims = vec![Prim::Line { pts: vec![a, t], width: thin, fill: symbol::Fill::None }];
    prims.extend(flag(t, (0, -1), ctx.names[n], false));
    let mut x = right + 2.54;
    let mut tops = Vec::new();
    for (cap, top) in built {
        let b = cap.bounds;
        let at_x = x + (top.0 - b.0);
        let d = (at_x - top.0, line_y - top.1);
        for (attrs, ps) in cap.groups {
            node.groups.push((attrs, shift(&ps, d)));
        }
        tops.push((at_x, line_y));
        x += b.2 - b.0 + 2.54;
    }
    if let Some(&last) = tops.last() {
        prims.push(Prim::Line { pts: vec![t, last], width: thin, fill: symbol::Fill::None });
        for p in std::iter::once(&t).chain(&tops[..tops.len() - 1]) {
            prims.push(Prim::Circle { c: *p, r: 0.38, width: thin, fill: symbol::Fill::Outline });
        }
    }
    node.groups.push((attrs, prims));
    node.bounds = bounds_of(&node.groups);
}

/// The graph ELK lays out: the nodes, their ports fixed where their pins are, and each wired net's edges from its
/// driving pin.
fn graph(ctx: &Ctx, nodes: &[Node], labelled: &Labels) -> Value {
    let mut children = Vec::new();
    for node in nodes {
        let b = node.bounds;
        let ports: Vec<Value> = node.ports.iter().map(|p| {
            let edge = edge_point(p.at, p.out, b);
            let side = match p.out {
                (-1, _) => "WEST",
                (1, _) => "EAST",
                (_, -1) => "NORTH",
                _ => "SOUTH",
            };
            json!({"id": p.id, "x": (edge.0 - b.0) * PX, "y": (edge.1 - b.1) * PX, "width": 0, "height": 0,
                   "layoutOptions": {"elk.port.side": side}})
        }).collect();
        let mut options = json!({"elk.portConstraints": "FIXED_POS"});
        // a connector at the edge its signals come in from or go out to
        let offboard = node.part != usize::MAX && ctx.c.parts[node.part].kind == Kind::Offboard;
        // (not in the rails style: ELK allows no edge into its first layer, and there supplies' symbols wire to
        // connectors' pins)
        if ctx.style == Style::Symbols && offboard && !node.ports.is_empty() {
            let driving = node.ports.iter().filter(|p| p.drive > 0.0).count();
            let place = if 2 * driving >= node.ports.len() { "FIRST" } else { "LAST" };
            options["elk.layered.layering.layerConstraint"] = json!(place);
        }
        children.push(json!({
            "id": node.id, "width": (b.2 - b.0) * PX, "height": (b.3 - b.1) * PX, "ports": ports, "layoutOptions": options,
        }));
    }
    let mut edges = Vec::new();
    for (n, name) in ctx.names.iter().enumerate() {
        if !ctx.wired(n) || labelled.whole[n] {
            continue;
        }
        let ports: Vec<&Port> = nodes.iter().flat_map(|node| &node.ports).filter(|p| p.net == n).collect();
        if ports.len() < 2 {
            continue;
        }
        // from the supply's symbol, else the pin that drives the net
        let from = ports.iter().position(|p| p.drive > 1.0).or_else(|| ports.iter().position(|p| p.drive > 0.0)).unwrap_or(0);
        for (j, p) in ports.iter().enumerate() {
            if j != from {
                edges.push(json!({"id": format!("{}#{}", name, j), "sources": [ports[from].id], "targets": [p.id]}));
            }
        }
    }
    json!({
        "id": "root",
        "layoutOptions": {
            "elk.algorithm": "layered",
            "elk.direction": "RIGHT",
            "elk.edgeRouting": "ORTHOGONAL",
            "elk.layered.mergeEdges": "true",
            "elk.spacing.nodeNode": "30",
            "elk.layered.spacing.nodeNodeBetweenLayers": "40",
            "elk.spacing.edgeNode": "15",
            "elk.spacing.edgeEdge": "10",
            "elk.layered.spacing.edgeNodeBetweenLayers": "15",
            "elk.spacing.componentComponent": "40",
            "elk.padding": "[top=20,left=20,bottom=20,right=20]",
        },
        "children": children,
        "edges": edges,
    })
}

/// Whether two nets are lines of one bus: the same name but for a number at the end (BUS0 and BUS7, D0 and D17).
fn same_bus(a: &str, b: &str) -> bool {
    fn stem(s: &str) -> Option<&str> {
        let t = s.trim_end_matches(|c: char| c.is_ascii_digit());
        (t.len() < s.len() && !t.is_empty()).then_some(t)
    }
    matches!((stem(a), stem(b)), (Some(x), Some(y)) if x == y)
}

/// A net with this many pins is a label from the start.
const MANY_PINS: usize = 6;
/// So are the nets of an off-board part with this many pins on signals.
const BIG_CONNECTOR: usize = 12;
/// A net whose wires are longer than this share of the page's width and height together becomes a label.
const LONG: f64 = 0.75;
/// A long net's pins that stay wired are joined in under this share of the page's width and height together (far_pins),
/// and once drawn, its wires are no longer than this for each two pins they join.
const SHORT: f64 = 0.4;
/// Lay out at most this many times.
const ROUNDS: usize = 4;

/// What's drawn of the wires, in pixels: each net's lines (pins drawn on to their ports included), its branch dots,
/// and where its pins are.
struct Drawn {
    lines: Vec<(usize, Vec<Xy>)>,
    dots: Vec<(usize, Xy)>,
    pins: Vec<(usize, Xy)>,
    /// a net's name written on its wire (a net wired at some pins, labelled at others): where its text starts
    names: Vec<(usize, Xy)>,
    /// each node's place: (x, y) in pixels of its box's corner
    at: std::collections::HashMap<String, Xy>,
}

impl Drawn {
    fn length(&self, n: usize) -> f64 {
        self.lines.iter().filter(|(m, _)| *m == n).flat_map(|(_, pts)| pts.windows(2))
            .map(|s| (s[1].0 - s[0].0).abs() + (s[1].1 - s[0].1).abs()).sum()
    }
}

/// The wires ELK routed, and each pin drawn on from its symbol to its port.
fn route(nodes: &[Node], laid_out: &Value, at: std::collections::HashMap<String, Xy>, down: f64) -> Drawn {
    // ELK's points, moved down by `down` (the power row above the layout)
    let xy = |v: &Value| (v["x"].as_f64().unwrap(), v["y"].as_f64().unwrap() + down);
    let mut d = Drawn { lines: vec![], dots: vec![], pins: vec![], names: vec![], at };
    let net_of_port: std::collections::HashMap<&str, usize> = nodes.iter().flat_map(|n| &n.ports).map(|p| (p.id.as_str(), p.net)).collect();
    for e in laid_out["edges"].as_array().unwrap() {
        let net = net_of_port[e["sources"][0].as_str().unwrap()];
        for s in e["sections"].as_array().into_iter().flatten() {
            let mut pts = vec![xy(&s["startPoint"])];
            pts.extend(s["bendPoints"].as_array().into_iter().flatten().map(xy));
            pts.push(xy(&s["endPoint"]));
            d.lines.push((net, pts));
        }
    }
    for node in nodes {
        let (x, y) = d.at[&node.id];
        let b = node.bounds;
        let o = (x / PX - b.0, y / PX - b.1);
        for p in &node.ports {
            let (a, e) = (add(o, p.at), add(o, edge_point(p.at, p.out, b)));
            let (a, e) = ((a.0 * PX, a.1 * PX), (e.0 * PX, e.1 * PX));
            d.pins.push((p.net, a));
            if a != e {
                d.lines.push((p.net, vec![a, e]));
            }
        }
    }
    d.dots = junctions(&d.lines);
    d
}

/// Where a net's wires branch, as the drawing shows them: each net's lines cut into pieces at every point where one
/// meets another, pieces drawn twice counted once (ELK's merged edges share their first stretch), and a dot wherever
/// three pieces or more meet. (ELK's own junction points only cover the edges it merged.)
fn junctions(lines: &[(usize, Vec<Xy>)]) -> Vec<(usize, Xy)> {
    // points to a tenth of a pixel, so ends that meet are equal
    let key = |p: Xy| ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64);
    let mut nets: Vec<usize> = lines.iter().map(|(n, _)| *n).collect();
    nets.sort();
    nets.dedup();
    let mut dots = Vec::new();
    for n in nets {
        let segs: Vec<(Xy, Xy)> = lines.iter().filter(|(m, _)| *m == n).flat_map(|(_, pts)| pts.windows(2).map(|w| (w[0], w[1])))
            .filter(|(a, b)| key(*a) != key(*b)).collect();
        let mut points: Vec<Xy> = segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
        points.sort_by_key(|p| key(*p));
        points.dedup_by_key(|p| key(*p));
        // each segment cut at the points on it, as pieces between neighbouring points
        let mut pieces: std::collections::HashSet<((i64, i64), (i64, i64))> = std::collections::HashSet::new();
        for (a, b) in &segs {
            let mut on_it: Vec<Xy> = points.iter().copied().filter(|p| on(*p, *a, *b)).collect();
            let along = |p: &Xy| (p.0 - a.0).abs() + (p.1 - a.1).abs();
            on_it.sort_by(|p, q| along(p).total_cmp(&along(q)));
            for w in on_it.windows(2) {
                let (x, y) = (key(w[0]), key(w[1]));
                if x != y {
                    pieces.insert(if x < y { (x, y) } else { (y, x) });
                }
            }
        }
        let mut degree: std::collections::HashMap<(i64, i64), usize> = std::collections::HashMap::new();
        for (x, y) in &pieces {
            *degree.entry(*x).or_default() += 1;
            *degree.entry(*y).or_default() += 1;
        }
        let mut here: Vec<(i64, i64)> = degree.into_iter().filter(|(_, d)| *d >= 3).map(|(p, _)| p).collect();
        here.sort();
        dots.extend(here.into_iter().map(|(x, y)| (n, (x as f64 / 10.0, y as f64 / 10.0))));
    }
    dots
}

/// Whether p lies on the segment from a to b (they're horizontal or vertical), within half a pixel.
fn on(p: Xy, a: Xy, b: Xy) -> bool {
    let e = 0.5;
    let within = |v: f64, x: f64, y: f64| v >= x.min(y) - e && v <= x.max(y) + e;
    if (a.1 - b.1).abs() < e {
        (p.1 - a.1).abs() < e && within(p.0, a.0, b.0)
    } else if (a.0 - b.0).abs() < e {
        (p.0 - a.0).abs() < e && within(p.1, a.1, b.1)
    } else {
        false
    }
}

/// How long two segments run along each other (both horizontal on one line, or both vertical), in pixels.
fn overlap(a: (Xy, Xy), b: (Xy, Xy)) -> f64 {
    let e = 0.5;
    let span = |x0: f64, x1: f64, y0: f64, y1: f64| (x0.max(x1).min(y0.max(y1)) - x0.min(x1).max(y0.min(y1))).max(0.0);
    let flat = |s: (Xy, Xy)| (s.0 .1 - s.1 .1).abs() < e;
    let upright = |s: (Xy, Xy)| (s.0 .0 - s.1 .0).abs() < e;
    if flat(a) && flat(b) && (a.0 .1 - b.0 .1).abs() < e {
        span(a.0 .0, a.1 .0, b.0 .0, b.1 .0)
    } else if upright(a) && upright(b) && (a.0 .0 - b.0 .0).abs() < e {
        span(a.0 .1, a.1 .1, b.0 .1, b.1 .1)
    } else {
        0.0
    }
}

/// The drawing read back, as the check reads a board: each wired net in one piece, and no wire of one net ending
/// on another's or running along it (either would look like a connection). (net, what's wrong) for each problem.
fn check(d: &Drawn, names: &[&str]) -> Vec<(usize, String)> {
    let mut problems = Vec::new();
    let segs = |i: usize| d.lines[i].1.windows(2).map(|s| (s[0], s[1])).collect::<Vec<_>>();
    let ends = |i: usize| [d.lines[i].1[0], d.lines[i].1[d.lines[i].1.len() - 1]];
    // one piece: lines of a net joined where one's end lies on the other
    for n in 0..names.len() {
        let mine: Vec<usize> = (0..d.lines.len()).filter(|&i| d.lines[i].0 == n).collect();
        if mine.is_empty() {
            continue;
        }
        let mut group: Vec<usize> = (0..mine.len()).collect();
        fn root(g: &mut Vec<usize>, i: usize) -> usize {
            let mut r = i;
            while g[r] != r {
                r = g[r];
            }
            g[i] = r;
            r
        }
        for x in 0..mine.len() {
            for y in 0..mine.len() {
                let touch = ends(mine[x]).iter().any(|p| segs(mine[y]).iter().any(|s| on(*p, s.0, s.1)));
                if x != y && touch {
                    let (rx, ry) = (root(&mut group, x), root(&mut group, y));
                    group[rx] = ry;
                }
            }
        }
        let pieces: std::collections::HashSet<usize> = (0..mine.len()).map(|x| root(&mut group, x)).collect();
        let near = |a: &Xy, b: &Xy| (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5;
        let loose = d.pins.iter().filter(|(m, _)| *m == n).filter(|(_, p)| !mine.iter().any(|&i| ends(i).iter().any(|e| near(e, p)))).count();
        if pieces.len() > 1 || loose > 0 {
            problems.push((n, format!("{} is drawn in {} pieces", names[n], pieces.len() + loose)));
        }
    }
    // nothing of one net on another
    for i in 0..d.lines.len() {
        for j in 0..d.lines.len() {
            let (m, n) = (d.lines[i].0, d.lines[j].0);
            if m == n {
                continue;
            }
            if ends(i).iter().any(|p| segs(j).iter().any(|s| on(*p, s.0, s.1))) {
                problems.push((m, format!("a wire of {} ends on {}", names[m], names[n])));
            }
            if i < j && segs(i).iter().any(|a| segs(j).iter().any(|b| overlap(*a, *b) > 1.0)) {
                problems.push((m, format!("{} runs along {}", names[m], names[n])));
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

/// The SVG: wires under the symbols, each net's in a group of its own.
fn render(nodes: &[Node], d: &Drawn, names: &[&str]) -> String {
    let px = |p: &Xy| format!("{:.1},{:.1}", p.0, p.1);
    let mut wires: Vec<String> = vec![String::new(); names.len()];
    for (n, pts) in &d.lines {
        wires[*n].push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linejoin=\"round\"/>",
                                    pts.iter().map(px).collect::<Vec<_>>().join(" ")));
    }
    for (n, p) in &d.names {
        wires[*n].push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{:.1}\" fill=\"currentColor\">{}</text>", p.0, p.1,
                                    1.27 * PX, symbol::escape(names[*n])));
    }
    for (n, p) in &d.dots {
        wires[*n].push_str(&format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"currentColor\"/>", p.0, p.1));
    }
    let mut body = String::new();
    for (n, w) in wires.iter().enumerate() {
        if !w.is_empty() {
            body.push_str(&format!("<g data-net=\"{}\">{}</g>", symbol::escape(names[n]), w));
        }
    }
    for node in nodes {
        let (x, y) = d.at[&node.id];
        let o = (x / PX - node.bounds.0, y / PX - node.bounds.1);
        for (attrs, prims) in &node.groups {
            body.push_str(&format!("<g {}>{}</g>", attrs, symbol::svg(&shift(prims, o))));
        }
    }
    body
}

/// How a two-lead part stands: in line (pin 1 left, unless its other leg is the one fed), hanging from its signal
/// (down to a ground, up to a supply), or upright between two supplies (the supply on top).
fn lead_orient(ctx: &Ctx, k: usize, unit: &symbol::Unit) -> Orient {
    let legs: Vec<Option<usize>> = unit.pins.iter().map(|sp| ctx.pin_net(k, sp)).collect();
    let class = |i: usize| legs[i].map(|n| ctx.classes[n]);
    let signal = |i: usize| class(i) == Some(Class::Signal);
    let rot = if signal(0) && signal(1) {
        let fed = |i: usize| !ctx.drives(legs[i].unwrap(), k, &unit.pins[i]);
        if fed(0) || !fed(1) { 270 } else { 90 }
    } else if signal(0) || signal(1) {
        let s = if signal(0) { 0 } else { 1 };
        let to_ground = class(1 - s) != Some(Class::Supply);
        if (s == 0) == to_ground { 0 } else { 180 }
    } else if class(1) == Some(Class::Supply) && class(0) != Some(Class::Supply) {
        180
    } else {
        0
    };
    Orient { rot, mirror: false }
}

/// How far apart (mm) two side pins on one supply may be and still share a power symbol: two pins' pitch.
const GAP: f64 = 5.08;

/// A unit of a part's symbol as a node: drawn about its origin, its power symbols added, its ports found. For a
/// supply in `hang`, top pins get no symbol: where it would go is left in `anchors`, for decoupling caps to hang
/// from. A pin on `quiet` gets nothing (a cap's supply leg, wired to its chip's).
fn build(ctx: &Ctx, labelled: &Labels, k: usize, u: usize, orient: Orient, hang: &[usize], quiet: Option<usize>) -> Node {
    let (classes, names) = (&ctx.classes, &ctx.names);
    let p = &ctx.c.parts[k];
    let sym = &ctx.syms[k];
    let unit = &sym.drawing.units[u];
    let in_line = p.kind.is_lead() && orient.rot % 180 != 0;
    let prims = if in_line {
        // a part lying in line: its ref and value above it, across
        let mut quiet = sym.clone();
        for f in [&mut quiet.drawing.reference, &mut quiet.drawing.value].into_iter().flatten() {
            f.hidden = true;
        }
        let mut prims = symbol::prims(&quiet, u, orient, (0.0, 0.0));
        let b = symbol::bounds(&prims);
        let mid = (b.0 + b.2) / 2.0;
        let mut y = b.1 - 0.5;
        for text in [sym.value.clone(), sym.unit_ref(u)] {
            if !text.is_empty() {
                prims.push(Prim::Text { at: (mid, y), up: false, text, size: 1.27, anchor: "middle" });
                y -= 1.5;
            }
        }
        prims
    } else {
        symbol::prims(sym, u, orient, (0.0, 0.0))
    };
    let mut ports = Vec::new();
    let mut rails: Vec<(Xy, (i32, i32), usize)> = Vec::new();
    let mut flags: Vec<(String, Vec<Prim>)> = Vec::new();
    let mut labels: Vec<(String, Vec<Prim>)> = Vec::new();
    let mut anchors: Vec<(usize, Xy)> = Vec::new();
    for sp in unit.pins.iter().filter(|sp| !sp.hidden) {
        let n = match ctx.pin_net(k, sp) {
            Some(n) if Some(n) != quiet => n,
            _ => continue,
        };
        let at = orient.point(sp.at);
        let d = orient.dir(sp.dir);
        let out = (-d.0, -d.1);
        if classes[n] == Class::Signal && (labelled.whole[n] || labelled.apart.contains(&(k, sp.pins[0]))) {
            labels.push((format!("data-net=\"{}\"", symbol::escape(names[n])), label(at, out, names[n])));
        } else if ctx.wired(n) && !labelled.whole[n] {
            if ports.iter().any(|q: &Port| q.at == at) {
                continue;
            }
            let id = format!("{}.{}.{}", p.r#ref, u, sp.number);
            ports.push(Port { net: n, at, out, drive: if ctx.drives(n, k, sp) { 1.0 } else { 0.0 }, id, pin: (k, sp.pins[0]) });
        } else {
            rails.push((at, out, n));
        }
    }
    // power symbols, supplies pointing up and grounds down. Neighbouring pins on one supply, on one side, share one,
    // their wires joined. A side pin's symbol turns up or down only where no other pin on that side is in its way.
    let mut done = vec![false; rails.len()];
    for i in 0..rails.len() {
        if done[i] {
            continue;
        }
        done[i] = true;
        let (at, out, n) = rails[i];
        let ground = classes[n] == Class::Ground;
        let way = if ground { (0, 1) } else { (0, -1) };
        let attrs = format!("data-net=\"{}\"", symbol::escape(names[n]));
        if out == (-way.0, -way.1) {
            flags.push((attrs, flag(at, out, names[n], ground)));
            continue;
        }
        // pins line up along `axis` (y for a side, x for the top or bottom); `across` is the other
        let axis = if out.0 != 0 { 1 } else { 0 };
        let get = |p: Xy, k: usize| if k == 0 { p.0 } else { p.1 };
        let same_line = |p: Xy| (get(p, 1 - axis) - get(at, 1 - axis)).abs() < 1e-6;
        let mut run: Vec<Xy> = vec![at];
        loop {
            let (lo, hi) = run.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(get(*p, axis)), h.max(get(*p, axis))));
            let gap = |p: Xy| (get(p, axis) - lo).abs().min((get(p, axis) - hi).abs());
            let blocked = |v: f64| ports.iter().any(|q: &Port| q.out == out && same_line(q.at) && {
                let w = get(q.at, axis);
                (w - v) * (w - lo) < 0.0 || (w - v) * (w - hi) < 0.0
            });
            let next = (0..rails.len()).filter(|&j| !done[j] && rails[j].2 == n && rails[j].1 == out && same_line(rails[j].0))
                .filter(|&j| gap(rails[j].0) <= GAP && !blocked(get(rails[j].0, axis)))
                .min_by(|&x, &y| gap(rails[x].0).total_cmp(&gap(rails[y].0)));
            match next {
                Some(j) => {
                    done[j] = true;
                    run.push(rails[j].0);
                }
                None => break,
            }
        }
        run.sort_by(|p, q| get(*p, axis).total_cmp(&get(*q, axis)));
        let reach = if out.0 != 0 { 2.54 } else { 1.27 };
        let corners: Vec<Xy> = run.iter().map(|p| add(*p, along(out, reach))).collect();
        let thin = 0.1524;
        let mut prims: Vec<Prim> = run.iter().zip(&corners).map(|(a, b)| Prim::Line { pts: vec![*a, *b], width: thin, fill: symbol::Fill::None })
            .collect();
        let (first, last) = (corners[0], corners[corners.len() - 1]);
        if first != last {
            prims.push(Prim::Line { pts: vec![first, last], width: thin, fill: symbol::Fill::None });
            for c in &corners[1..corners.len() - 1] {
                prims.push(Prim::Circle { c: *c, r: 0.38, width: thin, fill: symbol::Fill::Outline });
            }
        }
        if out.0 == 0 && out == way && hang.contains(&n) {
            // decoupling caps hang from here (hang_caps), the symbol above them
            anchors.push((n, corners[corners.len() / 2]));
        } else if out.0 == 0 {
            // along the top or bottom: one symbol from the middle
            prims.extend(flag(corners[corners.len() / 2], way, names[n], ground));
        } else {
            let end = if ground { last } else { first };
            // anything else on this side within reach of a symbol turned up or down?
            let reach_to = end.1 + way.1 as f64 * 3.81;
            let others = ports.iter().map(|q| (q.at, q.out)).chain(rails.iter().filter(|r| !run.contains(&r.0)).map(|r| (r.0, r.1)));
            let in_way = others.filter(|(p, o)| *o == out && same_line(*p)).any(|(p, _)| (p.1 - end.1) * (p.1 - reach_to) <= 0.0);
            if in_way {
                prims.extend(flag(corners[corners.len() / 2], out, names[n], ground));
            } else {
                prims.extend(flag(end, way, names[n], ground));
            }
        }
        flags.push((attrs, prims));
    }
    let unit_attr = if sym.drawing.units.len() > 1 {
        format!(" data-unit=\"{}\"", symbol::unit_letter(unit.unit))
    } else {
        String::new()
    };
    // the body: its shapes, else (a unit of pins only) where its pins end
    let mut pts: Vec<Xy> = Vec::new();
    for shape in &unit.shapes {
        match shape {
            symbol::Shape::Rect { a, b, .. } => pts.extend([*a, *b]),
            symbol::Shape::Poly { pts: ps, .. } => pts.extend(ps.iter().copied()),
            symbol::Shape::Circle { c, r, .. } => pts.extend([(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)]),
            _ => {}
        }
    }
    if pts.is_empty() {
        pts.extend(unit.pins.iter().map(|sp| add(sp.at, along(sp.dir, sp.len))));
    }
    let pts: Vec<Xy> = pts.iter().map(|q| orient.point(*q)).collect();
    let body = pts.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                               |b, q| (b.0.min(q.0), b.1.min(q.1), b.2.max(q.0), b.3.max(q.1)));
    let has_labels = !labels.is_empty();
    let mut groups = vec![(format!("data-part=\"{}\"{}", symbol::escape(&sym.r#ref), unit_attr), prims)];
    groups.extend(flags);
    groups.extend(labels);
    let bounds = bounds_of(&groups);
    Node { part: k, id: sym.unit_ref(u), bounds, ports, groups, anchors, body, labels: has_labels, power: false }
}

fn bounds_of(groups: &[(String, Vec<Prim>)]) -> (f64, f64, f64, f64) {
    let all: Vec<Prim> = groups.iter().flat_map(|(_, p)| p.iter().cloned()).collect();
    symbol::bounds(&all)
}

/// Prims moved by d (mm).
fn shift(prims: &[Prim], d: Xy) -> Vec<Prim> {
    let m = |p: &Xy| add(*p, d);
    prims.iter().map(|p| match p {
        Prim::Line { pts, width, fill } => Prim::Line { pts: pts.iter().map(m).collect(), width: *width, fill: *fill },
        Prim::Circle { c, r, width, fill } => Prim::Circle { c: m(c), r: *r, width: *width, fill: *fill },
        Prim::Arc { a, mid, b, width } => Prim::Arc { a: m(a), mid: m(mid), b: m(b), width: *width },
        Prim::Text { at, up, text, size, anchor } => Prim::Text { at: m(at), up: *up, text: text.clone(), size: *size, anchor },
        other => other.clone(),
    }).collect()
}

/// Where a pin pointing `out` from `at` meets the edge of the box `b`.
fn edge_point(at: Xy, out: (i32, i32), b: (f64, f64, f64, f64)) -> Xy {
    match out {
        (-1, _) => (b.0, at.1),
        (1, _) => (b.2, at.1),
        (_, -1) => (at.0, b.1),
        _ => (at.0, b.3),
    }
}

/// The schematic as a file: on white, in ink, with the board's font.
pub fn page(s: &Schematic) -> String {
    svg_page(s, true)
}

/// The schematic as an SVG: on white (`background`), or on nothing, in `currentColor`, for a page to style (the
/// scene's).
pub fn svg_page(s: &Schematic, background: bool) -> String {
    let (color, back) = if background {
        (format!(" color=\"{}\"", crate::render::INK), "<rect width=\"100%\" height=\"100%\" fill=\"white\"/>")
    } else {
        (String::new(), "")
    };
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" font-family=\"{}\"{}>{}{}</svg>\n",
            crate::render::FONT, color, back, s.svg, w = s.width, h = s.height)
}

/// The schematic of the circuit in this file, drawn on a thread of its own (it needs only the parts and nets, so a
/// build can place and wire meanwhile). The thread gives back the schematic and how long it took, or why not.
pub fn in_background(data: &crate::val::Val, source: &str, style: Style) -> std::thread::JoinHandle<(Result<Schematic, String>, f64)> {
    let (data, source) = (data.clone(), source.to_string());
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let s = Circuit::netlist(&data, &source).and_then(|c| draw_with(&c, style));
        (s, start.elapsed().as_secs_f64() * 1000.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(lines: Vec<(usize, Vec<Xy>)>, pins: Vec<(usize, Xy)>) -> Drawn {
        Drawn { lines, dots: vec![], pins, names: vec![], at: Default::default() }
    }

    #[test]
    fn a_dot_where_three_wires_meet_and_nowhere_else() {
        let line = |pts: &[Xy]| (0usize, pts.to_vec());
        // a T: a branch ending on a wire
        assert_eq!(junctions(&[line(&[(0.0, 0.0), (100.0, 0.0)]), line(&[(50.0, 40.0), (50.0, 0.0)])]), vec![(0, (50.0, 0.0))]);
        // a corner, and two wires crossing: no dot
        assert!(junctions(&[line(&[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0)])]).is_empty());
        assert!(junctions(&[line(&[(0.0, 0.0), (100.0, 0.0)]), line(&[(50.0, -40.0), (50.0, 40.0)])]).is_empty());
        // two edges from one pin sharing their first stretch (ELK's merged edges): a dot where they part only
        let merged = [line(&[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0)]), line(&[(0.0, 0.0), (60.0, 0.0), (100.0, 0.0)])];
        assert_eq!(junctions(&merged), vec![(0, (60.0, 0.0))]);
    }

    #[test]
    fn the_check_sees_a_net_in_pieces_and_nets_touching() {
        let names = ["A", "B"];
        // A in one piece: a branch ending on the trunk
        let ok = drawn(vec![(0, vec![(0.0, 0.0), (100.0, 0.0)]), (0, vec![(50.0, 0.0), (50.0, 40.0)])],
                       vec![(0, (0.0, 0.0)), (0, (100.0, 0.0)), (0, (50.0, 40.0))]);
        assert!(check(&ok, &names).is_empty());
        // A in two pieces
        let apart = drawn(vec![(0, vec![(0.0, 0.0), (40.0, 0.0)]), (0, vec![(60.0, 0.0), (100.0, 0.0)])],
                          vec![(0, (0.0, 0.0)), (0, (100.0, 0.0))]);
        assert_eq!(check(&apart, &names), vec![(0, "A is drawn in 2 pieces".to_string())]);
        // B ending on A, and B running along A: both look like connections
        let touching = drawn(vec![(0, vec![(0.0, 0.0), (100.0, 0.0)]), (1, vec![(50.0, 30.0), (50.0, 0.0)])],
                             vec![(0, (0.0, 0.0)), (1, (50.0, 30.0))]);
        assert!(check(&touching, &names).iter().any(|(_, p)| p == "a wire of B ends on A"));
        let along = drawn(vec![(0, vec![(0.0, 0.0), (100.0, 0.0)]), (1, vec![(20.0, 0.0), (80.0, 0.0)])],
                          vec![(0, (0.0, 0.0)), (1, (20.0, 0.0))]);
        assert!(check(&along, &names).iter().any(|(_, p)| p.contains("runs along")));
        // crossing isn't touching
        let crossing = drawn(vec![(0, vec![(0.0, 0.0), (100.0, 0.0)]), (1, vec![(50.0, -30.0), (50.0, 30.0)])],
                             vec![(0, (0.0, 0.0)), (1, (50.0, -30.0))]);
        assert!(check(&crossing, &names).is_empty());
    }
}
