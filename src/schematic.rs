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
    unit: usize,
    orient: Orient,
    /// the symbol's box (mm, with the symbol's origin at 0,0, turned), power symbols and text included
    bounds: (f64, f64, f64, f64),
    /// signal pins: (index of the symbol's pin in its unit, net, where it attaches, which way it points out)
    ports: Vec<Port>,
    /// pins on supplies and grounds: (where, which way out, net)
    flags: Vec<(Xy, (i32, i32), usize)>,
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

/// A power symbol at the end of a pin: a short line out, then a bar with the supply's name, or the ground sign.
fn flag(at: Xy, out: (i32, i32), name: &str, ground: bool) -> Vec<Prim> {
    let thin = 0.1524;
    let stem = add(at, along(out, 1.27));
    let across = (out.1.abs(), out.0.abs());
    let mut prims = vec![Prim::Line { pts: vec![at, stem], width: thin, fill: symbol::Fill::None }];
    let side = |k: f64| (across.0 as f64 * k, across.1 as f64 * k);
    if ground {
        let tip = add(stem, along(out, 1.27));
        prims.push(Prim::Line { pts: vec![add(stem, side(-1.27)), add(stem, side(1.27)), tip, add(stem, side(-1.27))], width: thin,
                                fill: symbol::Fill::None });
        if name != "GND" {
            prims.push(text_beyond(add(tip, along(out, 0.5)), out, name));
        }
    } else {
        prims.push(Prim::Line { pts: vec![add(stem, side(-1.27)), add(stem, side(1.27))], width: 0.254, fill: symbol::Fill::None });
        prims.push(text_beyond(add(stem, along(out, 0.6)), out, name));
    }
    prims
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

    // the nodes, each unit turned as it reads best
    let mut nodes: Vec<Node> = Vec::new();
    for (k, p) in c.parts.iter().enumerate() {
        let sym = &syms[k];
        for (u, unit) in sym.drawing.units.iter().enumerate() {
            let connected = unit.pins.iter().any(|sp| pin_net(k, sp).is_some());
            if !connected && sym.drawing.units.len() > 1 {
                continue;
            }
            let mut orient = Orient::default();
            if p.kind.is_lead() && unit.pins.len() == 2 {
                let legs: Vec<Option<usize>> = unit.pins.iter().map(|sp| pin_net(k, sp)).collect();
                let class = |i: usize| legs[i].map(|n| classes[n]);
                let signal = |i: usize| class(i) == Some(Class::Signal);
                orient.rot = if signal(0) && signal(1) {
                    // in line, the leg whose net something else drives on the left
                    let fed = |i: usize| !drives(legs[i].unwrap(), k, &unit.pins[i]);
                    if fed(0) || !fed(1) { 270 } else { 90 }
                } else if signal(0) || signal(1) {
                    // hanging from its signal: down to a ground, up to a supply
                    let s = if signal(0) { 0 } else { 1 };
                    let to_ground = class(1 - s) != Some(Class::Supply);
                    if (s == 0) == to_ground { 0 } else { 180 }
                } else if class(1) == Some(Class::Supply) && class(0) != Some(Class::Supply) {
                    180
                } else {
                    0
                };
            }
            let mut prims = symbol::prims(sym, u, orient, (0.0, 0.0));
            let mut ports = Vec::new();
            let mut flags = Vec::new();
            for sp in unit.pins.iter().filter(|sp| !sp.hidden) {
                let n = match pin_net(k, sp) {
                    Some(n) => n,
                    None => continue,
                };
                let at = orient.point(sp.at);
                let d = orient.dir(sp.dir);
                let out = (-d.0, -d.1);
                if classes[n] == Class::Signal {
                    if ports.iter().any(|q: &Port| q.at == at) {
                        continue;
                    }
                    let id = format!("{}.{}.{}", p.r#ref, u, sp.number);
                    ports.push(Port { net: n, at, out, drive: if drives(n, k, sp) { 1.0 } else { 0.0 }, id });
                } else {
                    prims.extend(flag(at, out, names[n], classes[n] == Class::Ground));
                    flags.push((at, out, n));
                }
            }
            let bounds = symbol::bounds(&prims);
            nodes.push(Node { part: k, unit: u, orient, bounds, ports, flags });
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
        let sym = &syms[node.part];
        children.push(json!({
            "id": sym.unit_ref(node.unit), "width": (b.2 - b.0) * PX, "height": (b.3 - b.1) * PX, "ports": ports,
            "layoutOptions": {"elk.portConstraints": "FIXED_POS"},
        }));
    }
    let mut edges = Vec::new();
    for (n, name) in names.iter().enumerate() {
        if classes[n] != Class::Signal {
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

    // the drawing: wires under the symbols
    let at_of: std::collections::HashMap<String, (f64, f64)> = laid_out["children"].as_array().unwrap().iter()
        .map(|n| (n["id"].as_str().unwrap().to_string(), (n["x"].as_f64().unwrap(), n["y"].as_f64().unwrap()))).collect();
    let mut wires: Vec<String> = vec![String::new(); names.len()];
    let px = |p: Xy| format!("{:.1},{:.1}", p.0, p.1);
    for e in laid_out["edges"].as_array().unwrap() {
        let net = names.iter().position(|n| e["id"].as_str().unwrap().rsplit_once('#').map(|(a, _)| a == *n).unwrap_or(false)).unwrap();
        for s in e["sections"].as_array().into_iter().flatten() {
            let mut pts = vec![(s["startPoint"]["x"].as_f64().unwrap(), s["startPoint"]["y"].as_f64().unwrap())];
            for b in s["bendPoints"].as_array().into_iter().flatten() {
                pts.push((b["x"].as_f64().unwrap(), b["y"].as_f64().unwrap()));
            }
            pts.push((s["endPoint"]["x"].as_f64().unwrap(), s["endPoint"]["y"].as_f64().unwrap()));
            wires[net].push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linejoin=\"round\"/>",
                                         pts.iter().map(|p| px(*p)).collect::<Vec<_>>().join(" ")));
        }
        for j in e["junctionPoints"].as_array().into_iter().flatten() {
            wires[net].push_str(&format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"currentColor\"/>",
                                         j["x"].as_f64().unwrap(), j["y"].as_f64().unwrap()));
        }
    }
    let mut parts_svg = String::new();
    for node in &nodes {
        let sym = &syms[node.part];
        let (x, y) = at_of[&sym.unit_ref(node.unit)];
        let b = node.bounds;
        let origin = (x / PX - b.0, y / PX - b.1);
        // a pin drawn on to its port, on the node's edge
        for p in &node.ports {
            let (a, e) = (add(origin, p.at), add(origin, edge_point(p.at, p.out, b)));
            if a != e {
                wires[p.net].push_str(&format!("<polyline points=\"{} {}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\"/>",
                                               px((a.0 * PX, a.1 * PX)), px((e.0 * PX, e.1 * PX))));
            }
        }
        let prims = symbol::prims(sym, node.unit, node.orient, origin);
        let mut flags = Vec::new();
        for &(at, out, n) in &node.flags {
            flags.push((n, symbol::svg(&flag(add(origin, at), out, names[n], classes[n] == Class::Ground))));
        }
        let unit = if sym.drawing.units.len() > 1 { format!(" data-unit=\"{}\"", symbol::unit_letter(sym.drawing.units[node.unit].unit)) } else { String::new() };
        parts_svg.push_str(&format!("<g data-part=\"{}\"{}>{}</g>", symbol::escape(&sym.r#ref), unit, symbol::svg(&prims)));
        for (n, svg) in flags {
            parts_svg.push_str(&format!("<g data-net=\"{}\">{}</g>", symbol::escape(names[n]), svg));
        }
    }
    let mut body = String::new();
    for (n, w) in wires.iter().enumerate() {
        if !w.is_empty() {
            body.push_str(&format!("<g data-net=\"{}\">{}</g>", symbol::escape(names[n]), w));
        }
    }
    body.push_str(&parts_svg);
    let (width, height) = (laid_out["width"].as_f64().unwrap_or(0.0), laid_out["height"].as_f64().unwrap_or(0.0));
    Ok(Schematic { svg: body, width, height, graph, laid_out })
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
