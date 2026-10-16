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
}

struct Port {
    net: usize,
    at: Xy,
    out: (i32, i32),
    /// how strongly this pin drives its net
    drive: f64,
    id: String,
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

/// The schematic of a circuit.
pub fn draw(c: &Circuit) -> Result<Schematic, String> {
    let library = c.library.as_deref();
    let syms: Vec<PartSymbol> = c.parts.iter().map(|p| symbol::of_part(p, library)).collect();
    let classes: Vec<Class> = c.nets.values().map(|n| match n.polarity {
        Some('+') => Class::Supply,
        Some('-') => Class::Ground,
        _ => Class::Signal,
    }).collect();
    let names: Vec<&str> = c.nets.keys().map(String::as_str).collect();
    // each part's pins' nets
    let mut net_of: Vec<Vec<Option<usize>>> = c.parts.iter().map(|p| vec![None; p.pins.len()]).collect();
    for (n, net) in c.nets.values().enumerate() {
        for &PinId { part, pin } in &net.pins {
            net_of[part as usize][pin as usize] = Some(n);
        }
    }
    // a symbol pin is on the net of any of the part's pins it carries
    let pin_net = |part: usize, sp: &symbol::SymPin| sp.pins.iter().find_map(|&i| net_of[part][i]);

    // how far (in parts, along signal nets) each part is from what drives the circuit
    let strength: Vec<f64> = c.parts.iter().enumerate().map(|(k, p)| {
        syms[k].drawing.units.iter().flat_map(|u| &u.pins).filter(|sp| pin_net(k, sp).map(|n| classes[n] == Class::Signal).unwrap_or(false))
            .map(|sp| drive(p.kind, &p.source, &sp.kind)).fold(0.0, f64::max)
    }).collect();
    let top = strength.iter().cloned().fold(0.0, f64::max).min(4.0);
    let mut dist: Vec<usize> = strength.iter().map(|&s| if s >= top { 0 } else { usize::MAX }).collect();
    loop {
        let mut changed = false;
        for (n, net) in c.nets.values().enumerate() {
            if classes[n] != Class::Signal {
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

    // each net's driving pin: (drive, nearness, part order); the part's index and the symbol pin's number
    let mut driver: Vec<Option<(f64, usize, usize)>> = vec![None; c.nets.len()];
    let mut driver_key: Vec<(f64, i64, i64)> = vec![(-1.0, 0, 0); c.nets.len()];
    for (k, p) in c.parts.iter().enumerate() {
        for sp in syms[k].drawing.units.iter().flat_map(|u| &u.pins) {
            if let Some(n) = pin_net(k, sp) {
                let key = (drive(p.kind, &p.source, &sp.kind), -(dist[k].min(1 << 20) as i64), -(k as i64));
                if driver[n].is_none() || key > driver_key[n] {
                    driver[n] = Some((key.0, k, sp.pins[0]));
                    driver_key[n] = key;
                }
            }
        }
    }
    let drives = |n: usize, part: usize, sp: &symbol::SymPin| driver[n].map(|(_, k, i)| k == part && sp.pins.contains(&i)).unwrap_or(false);

    // nets drawn as labels from the start: one with many pins (a supply without a rail, like 16's VDDIO), and
    // those of a connector with many pins (its wires would run all over the page)
    let mut labelled: Vec<bool> = (0..names.len()).map(|n| {
        let pins = &c.nets[n].pins;
        let big_connector = pins.iter().any(|p| {
            let part = &c.parts[p.part as usize];
            part.kind == Kind::Offboard
                && net_of[p.part as usize].iter().flatten().filter(|&&m| classes[m] == Class::Signal).count() >= BIG_CONNECTOR
        });
        classes[n] == Class::Signal && (pins.len() >= MANY_PINS || big_connector)
    }).collect();
    let mut round = 0;
    loop {
    // decoupling caps (both legs on supplies) go beside a chip: the one they're `near`, else the first part with a
    // pin on their supply that has fewest so far
    let lead_nets = |k: usize| -> Vec<Option<usize>> { (0..c.parts[k].pins.len()).map(|i| net_of[k][i]).collect() };
    let mut beside: Vec<Option<usize>> = vec![None; c.parts.len()];
    let mut count = vec![0usize; c.parts.len()];
    for (k, p) in c.parts.iter().enumerate() {
        let legs = lead_nets(k);
        let rails = legs.iter().all(|n| n.map(|n| classes[n] != Class::Signal).unwrap_or(false));
        if !(p.kind.is_lead() && legs.len() == 2 && rails) {
            continue;
        }
        let near = p.near.as_ref().and_then(|r| c.parts.iter().position(|q| &q.r#ref == r));
        let supply = legs.iter().flatten().find(|&&n| classes[n] == Class::Supply).copied();
        let host = near.or_else(|| {
            let module = |j: usize| c.parts[j].kind == Kind::Pico || c.parts[j].source.starts_with("MCU_Module");
            (0..c.parts.len()).filter(|&j| !c.parts[j].kind.is_lead() && c.parts[j].kind != Kind::Offboard)
                .filter(|&j| net_of[j].iter().any(|n| n.is_some() && *n == supply)).min_by_key(|&j| (module(j), count[j], j))
        });
        if let Some(h) = host {
            beside[k] = Some(h);
            count[h] += 1;
        }
    }

    // a unit that only takes power (an op-amp's supply pins), when the part has others
    let power_unit = |k: usize, u: usize| {
        let pins = &syms[k].drawing.units[u].pins;
        syms[k].drawing.units.len() > 1 && pins.iter().any(|sp| pin_net(k, sp).is_some())
            && pins.iter().all(|sp| pin_net(k, sp).map(|n| classes[n] != Class::Signal).unwrap_or(true))
    };

    // the nodes, each unit turned as it reads best
    let mut nodes: Vec<Node> = Vec::new();
    for (k, p) in c.parts.iter().enumerate() {
        if beside[k].is_some() {
            continue;
        }
        let sym = &syms[k];
        for (u, unit) in sym.drawing.units.iter().enumerate() {
            let connected = unit.pins.iter().any(|sp| pin_net(k, sp).is_some());
            if (!connected || power_unit(k, u)) && sym.drawing.units.len() > 1 {
                continue;
            }
            let lead = p.kind.is_lead() && unit.pins.len() == 2;
            let mut orient = if lead { lead_orient(unit, &|sp| pin_net(k, sp), &classes, &|n, sp| drives(n, k, sp)) } else { Orient::default() };
            if sym.source == "potentiometer" {
                // a pot's ends: the supply on top, the ground below
                let end = |num: &str| unit.pins.iter().find(|sp| sp.number == num).and_then(|sp| pin_net(k, sp)).map(|n| classes[n]);
                if end("1") == Some(Class::Ground) || end("3") == Some(Class::Supply) {
                    orient.rot = 180;
                }
            }
            let mut node = build(c, &syms, k, u, orient, &|sp| pin_net(k, sp), &classes, &labelled, &names, &|n, sp| drives(n, k, sp));
            if !lead && sym.source != "potentiometer" {
                // a symbol whose driving pins point left, or whose driven ones point right, is mirrored
                let lean: f64 = node.ports.iter().map(|p| {
                    let side = p.out.0 as f64;
                    if p.drive > 0.0 { side } else { -side }
                }).sum();
                if lean < 0.0 {
                    orient.mirror = true;
                    node = build(c, &syms, k, u, orient, &|sp| pin_net(k, sp), &classes, &labelled, &names, &|n, sp| drives(n, k, sp));
                }
            }
            // above it, at its right: its power unit (an op-amp's supply pins) and its decoupling caps
            let mut extras: Vec<(usize, usize)> = (0..sym.drawing.units.len()).filter(|&v| v != u && power_unit(k, v)).map(|v| (k, v)).collect();
            extras.extend((0..c.parts.len()).filter(|&j| beside[j] == Some(k)).map(|j| (j, 0)));
            if u == 0 && !extras.is_empty() {
                let mut x = node.bounds.2;
                let mut placed = Vec::new();
                for &(j, v) in extras.iter().rev() {
                    let unit = &syms[j].drawing.units[v];
                    let o = if j == k { Orient::default() } else { lead_orient(unit, &|sp| pin_net(j, sp), &classes, &|n, sp| drives(n, j, sp)) };
                    let cap = build(c, &syms, j, v, o, &|sp| pin_net(j, sp), &classes, &labelled, &names, &|n, sp| drives(n, j, sp));
                    let b = cap.bounds;
                    x -= b.2 - b.0;
                    placed.push((cap, (x - b.0, node.bounds.1 - 1.27 - b.3)));
                    x -= 2.54;
                }
                for (cap, d) in placed {
                    for (attrs, prims) in cap.groups {
                        node.groups.push((attrs, shift(&prims, d)));
                    }
                }
                node.bounds = bounds_of(&node.groups);
            }
            nodes.push(node);
        }
    }

    // the graph
    let mut children = Vec::new();
    for node in &nodes {
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
        if c.parts[node.part].kind == Kind::Offboard && !node.ports.is_empty() {
            let driving = node.ports.iter().filter(|p| p.drive > 0.0).count();
            let place = if 2 * driving >= node.ports.len() { "FIRST" } else { "LAST" };
            options["elk.layered.layering.layerConstraint"] = json!(place);
        }
        children.push(json!({
            "id": node.id, "width": (b.2 - b.0) * PX, "height": (b.3 - b.1) * PX, "ports": ports, "layoutOptions": options,
        }));
    }
    let mut edges = Vec::new();
    for (n, name) in names.iter().enumerate() {
        if classes[n] != Class::Signal || labelled[n] {
            continue;
        }
        let ports: Vec<&Port> = nodes.iter().flat_map(|node| &node.ports).filter(|p| p.net == n).collect();
        if ports.len() < 2 {
            continue;
        }
        let from = ports.iter().position(|p| p.drive > 0.0).unwrap_or(0);
        for (j, p) in ports.iter().enumerate() {
            if j != from {
                edges.push(json!({"id": format!("{}#{}", name, j), "sources": [ports[from].id], "targets": [p.id]}));
            }
        }
    }
    let graph = json!({
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
    });
        let laid_out = crate::elk::layout(&graph)?;
        let drawn = route(&nodes, &laid_out);
        let problems = check(&drawn, &names);
        let (width, height) = (laid_out["width"].as_f64().unwrap_or(0.0), laid_out["height"].as_f64().unwrap_or(0.0));
        // what to label next: a net the check finds wrong, or one whose wires run across much of the page
        let long = |n: usize| drawn.length(n) > LONG * (width + height);
        let more: Vec<usize> = (0..names.len()).filter(|&n| !labelled[n] && (problems.iter().any(|(m, _)| *m == n) || long(n))).collect();
        round += 1;
        // BBGEN_SCHEMATIC_DEBUG: what each round found, to see why a net became a label
        if std::env::var("BBGEN_SCHEMATIC_DEBUG").is_ok() {
            for (_, p) in &problems {
                eprintln!("round {}: {}", round, p);
            }
            for n in (0..names.len()).filter(|&n| long(n) && !labelled[n]) {
                eprintln!("round {}: {} is {:.0} px long, the page {:.0} px across and down", round, names[n], drawn.length(n), width + height);
            }
        }
        if more.is_empty() || round == ROUNDS {
            let svg = render(&nodes, &drawn, &names);
            let problems = problems.into_iter().map(|(_, p)| p).collect();
            let labels = (0..names.len()).filter(|&n| labelled[n]).map(|n| names[n].to_string()).collect();
            return Ok(Schematic { svg, width, height, graph, laid_out, problems, labelled: labels });
        }
        for n in more {
            labelled[n] = true;
        }
        // a bus (BUS0, BUS1, ...) is labelled all together or not at all
        for n in 0..names.len() {
            if !labelled[n] && classes[n] == Class::Signal {
                labelled[n] = (0..names.len()).any(|m| labelled[m] && same_bus(names[m], names[n]));
            }
        }
    }
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
/// Lay out at most this many times.
const ROUNDS: usize = 3;

/// What's drawn of the wires, in pixels: each net's lines (pins drawn on to their ports included), its branch dots,
/// and where its pins are.
struct Drawn {
    lines: Vec<(usize, Vec<Xy>)>,
    dots: Vec<(usize, Xy)>,
    pins: Vec<(usize, Xy)>,
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
fn route(nodes: &[Node], laid_out: &Value) -> Drawn {
    let at: std::collections::HashMap<String, Xy> = laid_out["children"].as_array().unwrap().iter()
        .map(|n| (n["id"].as_str().unwrap().to_string(), (n["x"].as_f64().unwrap(), n["y"].as_f64().unwrap()))).collect();
    let xy = |v: &Value| (v["x"].as_f64().unwrap(), v["y"].as_f64().unwrap());
    let mut d = Drawn { lines: vec![], dots: vec![], pins: vec![], at };
    let net_of_port: std::collections::HashMap<&str, usize> = nodes.iter().flat_map(|n| &n.ports).map(|p| (p.id.as_str(), p.net)).collect();
    for e in laid_out["edges"].as_array().unwrap() {
        let net = net_of_port[e["sources"][0].as_str().unwrap()];
        for s in e["sections"].as_array().into_iter().flatten() {
            let mut pts = vec![xy(&s["startPoint"])];
            pts.extend(s["bendPoints"].as_array().into_iter().flatten().map(xy));
            pts.push(xy(&s["endPoint"]));
            d.lines.push((net, pts));
        }
        for j in e["junctionPoints"].as_array().into_iter().flatten() {
            d.dots.push((net, xy(j)));
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
    d
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
fn lead_orient(unit: &symbol::Unit, pin_net: &dyn Fn(&symbol::SymPin) -> Option<usize>, classes: &[Class],
               drives: &dyn Fn(usize, &symbol::SymPin) -> bool) -> Orient {
    let legs: Vec<Option<usize>> = unit.pins.iter().map(pin_net).collect();
    let class = |i: usize| legs[i].map(|n| classes[n]);
    let signal = |i: usize| class(i) == Some(Class::Signal);
    let rot = if signal(0) && signal(1) {
        let fed = |i: usize| !drives(legs[i].unwrap(), &unit.pins[i]);
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

/// A unit of a part's symbol as a node: drawn about its origin, its power symbols added, its ports found.
#[allow(clippy::too_many_arguments)]
fn build(c: &Circuit, syms: &[PartSymbol], k: usize, u: usize, orient: Orient, pin_net: &dyn Fn(&symbol::SymPin) -> Option<usize>,
         classes: &[Class], labelled: &[bool], names: &[&str], drives: &dyn Fn(usize, &symbol::SymPin) -> bool) -> Node {
    let p = &c.parts[k];
    let sym = &syms[k];
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
    for sp in unit.pins.iter().filter(|sp| !sp.hidden) {
        let n = match pin_net(sp) {
            Some(n) => n,
            None => continue,
        };
        let at = orient.point(sp.at);
        let d = orient.dir(sp.dir);
        let out = (-d.0, -d.1);
        if classes[n] == Class::Signal && labelled[n] {
            labels.push((format!("data-net=\"{}\"", symbol::escape(names[n])), label(at, out, names[n])));
        } else if classes[n] == Class::Signal {
            if ports.iter().any(|q: &Port| q.at == at) {
                continue;
            }
            let id = format!("{}.{}.{}", p.r#ref, u, sp.number);
            ports.push(Port { net: n, at, out, drive: if drives(n, sp) { 1.0 } else { 0.0 }, id });
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
        if out.0 == 0 {
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
    let mut groups = vec![(format!("data-part=\"{}\"{}", symbol::escape(&sym.r#ref), unit_attr), prims)];
    groups.extend(flags);
    groups.extend(labels);
    let bounds = bounds_of(&groups);
    Node { part: k, id: sym.unit_ref(u), bounds, ports, groups }
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
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" font-family=\"{}\" \
             color=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>{}</svg>\n",
            crate::render::FONT, crate::render::INK, s.svg, w = s.width, h = s.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(lines: Vec<(usize, Vec<Xy>)>, pins: Vec<(usize, Xy)>) -> Drawn {
        Drawn { lines, dots: vec![], pins, at: Default::default() }
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
