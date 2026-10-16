//! The scene: what a build produced, as JSON (`bbgen build circuit.yaml --json`), for programs that show or work
//! on a board (bbgen studio). The format is written up in docs/scene-format.md; `SCHEMA` goes up when a change
//! would break a reader.
//!
//! Positions are in the drawing's pixels, so whatever is laid over the SVG lines up with it.

use crate::board::RAILS;
use crate::check::Result;
use crate::circuit::{color_hex, Circuit, End, Node, PinId};
use crate::parts::{body_rect, placed_box, Kind};
use crate::render::Canvas;
use crate::val::Val;
use serde_json::{json, Map, Value};

pub const SCHEMA: i64 = 1;

fn round1(x: f64) -> Value {
    json!((x * 10.0).round() / 10.0)
}

fn pt(cv: &Canvas, p: (f64, f64)) -> Value {
    let [x, y] = cv.px(p);
    json!([round1(x), round1(y)])
}

fn rect(cv: &Canvas, r: (f64, f64, f64, f64)) -> Value {
    let ([x0, y0], [x1, y1]) = (cv.px((r.0, r.1)), cv.px((r.2, r.3)));
    json!([round1(x0), round1(y0), round1(x1), round1(y1)])
}

/// Whether the file gives a part's place itself (`at:` for a chip or module, `pins:` for a two-lead part).
fn placed_by_you(data: Option<&Val>, r: &str, kind: Kind) -> Option<bool> {
    let spec = data?.get("parts")?.get(r)?;
    let given = |key: &str| spec.get(key).map(|v| v.truthy()).unwrap_or(false);
    Some(if kind.is_lead() { given("pins") } else { kind == Kind::Offboard || given("at") })
}

/// The scene for a built circuit. `data` is the circuit file as read, to tell what it placed from what bbgen did.
pub fn scene(c: &Circuit, result: &Result, data: Option<&Val>, ms: f64) -> Value {
    let (svg, cv) = crate::render::draw_annotated(c, result);
    let (width, height) = svg_size(&svg);

    let mut net_of: crate::HashMap<PinId, String> = Default::default();
    for n in c.nets.values() {
        for &p in &n.pins {
            net_of.insert(p, n.name.clone());
        }
    }

    let boards: Vec<Value> = c.boards.iter().map(|b| {
        let mut rails = Map::new();
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
            rails.insert(rail.name().into(), json!(nets));
        }
        let mut holes = Map::new();
        for h in b.holes() {
            holes.insert(c.hole_name(h), pt(&cv, b.xy(h)));
        }
        json!({"id": b.id, "kind": b.kind, "columns": b.columns, "split_rails": b.split_rails,
               "rect": rect(&cv, (b.left(), b.top(), b.right(), b.bottom())), "rails": rails, "holes": holes})
    }).collect();

    let parts: Vec<Value> = c.parts.iter().enumerate().map(|(pi, p)| {
        let pins: Vec<Value> = p.pins.iter().enumerate().map(|(k, pin)| {
            let id = PinId { part: pi as u16, pin: k as u16 };
            json!({"num": pin.num, "name": pin.name, "hole": pin.hole.map(|h| c.hole_name(h)),
                   "xy": pin.xy.map(|xy| pt(&cv, xy)), "net": net_of.get(&id)})
        }).collect();
        let body = if p.kind == Kind::Offboard {
            p.r#box.map(|r| rect(&cv, r))
        } else if p.kind.is_footprint() && p.pins[0].xy.is_some() {
            body_rect(p).map(|r| rect(&cv, placed_box(p, r)))
        } else {
            None
        };
        let placed_by = match placed_by_you(data, &p.r#ref, p.kind) {
            Some(true) => json!("you"),
            Some(false) => json!("bbgen"),
            None => Value::Null,
        };
        json!({"ref": p.r#ref, "kind": p.kind.name(), "title": p.title, "value": p.value, "color": p.color,
               "source": p.source, "adapter": p.adapter, "at": p.pins.first().and_then(|x| x.hole).map(|h| c.hole_name(h)),
               "rot": p.rot, "raised": p.raised, "placed_by": placed_by, "legs": p.legs, "slant": p.slant,
               "keepouts": p.keepouts.iter().map(|(w, _)| w.clone()).collect::<Vec<_>>(),
               "body": body, "pins": pins, "text": crate::checklist::part_text(c, p)})
    }).collect();

    let end = |e: &Option<End>| match e {
        Some(e) => json!({"hole": e.hole.map(|h| c.hole_name(h)), "pin": e.pin.map(|p| c.pin_str(p)), "spec": e.spec}),
        None => Value::Null,
    };
    let wires: Vec<Value> = c.wires.iter().map(|w| {
        let placed = w.a.is_some() && w.b.is_some();
        let points: Vec<Value> = if placed { c.wire_points(w).into_iter().map(|p| pt(&cv, p)).collect() } else { vec![] };
        json!({"id": format!("W{}", w.num), "num": w.num, "group": w.group, "nets": result.wire_nets(c, w),
               "color": color_hex(&result.wire_color(c, w)), "a": end(&w.a), "b": end(&w.b),
               "spec": [w.spec.0, w.spec.1], "via": w.via, "points": points, "flex": w.flex,
               "flying": placed && !w.on_board(), "laid_by": if w.auto { "bbgen" } else { "you" },
               "slant": w.slant, "under": if placed { c.under_of(w) } else { vec![] },
               "text": crate::checklist::wire_text(c, result, w)})
    }).collect();

    let nets: Vec<Value> = c.nets.values().map(|n| {
        json!({"name": n.name, "color": color_hex(&n.color), "pins": n.pins.iter().map(|&p| c.pin_str(p)).collect::<Vec<_>>(),
               "rails": n.rails, "polarity": n.polarity.map(|p| p.to_string()), "guessed": n.guessed})
    }).collect();

    let issues: Vec<Value> = result.issues.iter().map(|i| {
        let mut refs: Vec<String> = Vec::new();
        for &p in &i.pins {
            let r = &c.parts[p.part as usize].r#ref;
            if !refs.contains(r) {
                refs.push(r.clone());
            }
        }
        json!({"level": i.level, "code": i.code, "message": i.message, "parts": refs,
               "pins": i.pins.iter().map(|&p| c.pin_str(p)).collect::<Vec<_>>(),
               "holes": i.holes.iter().flatten().map(|&h| c.hole_name(h)).collect::<Vec<_>>()})
    }).collect();

    let checklist: Vec<Value> = crate::checklist::steps(c, result).into_iter()
        .map(|(section, item, text)| json!({"section": section, "item": item, "text": text})).collect();

    json!({
        "schema": SCHEMA, "bbgen": env!("CARGO_PKG_VERSION"), "source": c.source, "title": c.title, "notes": c.notes,
        "groups": c.groups, "boards": boards, "parts": parts, "wires": wires, "nets": nets, "issues": issues,
        "checklist": checklist,
        "stats": {"errors": result.errors(), "warnings": result.warnings(), "parts": c.parts.len(), "wires": c.wires.len(),
                  "nets": c.nets.len(), "ms": (ms * 10.0).round() / 10.0},
        "width": width, "height": height, "svg": svg,
    })
}

/// A circuit file that couldn't be read, with where, when the message says (`... (line 3, column 1)`).
/// The scene's `schematic`: the drawing (on nothing, in `currentColor`, with `data-part`, `data-net` and `data-pin`
/// groups), its size, its style (`symbols` or `wires`), the nets drawn as labels, what the check found wrong in it (nothing, unless bbgen has a bug),
/// and how long it took.
pub fn schematic(s: &crate::schematic::Schematic, ms: f64, style: crate::schematic::Style) -> Value {
    let style = match style {
        crate::schematic::Style::Symbols => "symbols",
        crate::schematic::Style::Wires => "wires",
    };
    json!({"svg": crate::schematic::svg_page(s, false), "width": s.width, "height": s.height, "style": style,
           "labels": s.labelled, "problems": s.problems, "ms": (ms * 10.0).round() / 10.0})
}

pub fn error(message: &str) -> Value {
    let mut line = Value::Null;
    let mut column = Value::Null;
    if let Some(at) = message.rfind("(line ") {
        let nums: Vec<i64> = message[at..].split(|ch: char| !ch.is_ascii_digit()).filter_map(|s| s.parse().ok()).collect();
        if nums.len() >= 2 {
            line = json!(nums[0]);
            column = json!(nums[1]);
        }
    }
    json!({"schema": SCHEMA, "bbgen": env!("CARGO_PKG_VERSION"), "error": {"message": message, "line": line, "column": column}})
}

fn svg_size(svg: &str) -> (f64, f64) {
    let grab = |key: &str| -> f64 {
        svg.find(&format!(" {}=\"", key))
            .and_then(|at| svg[at + key.len() + 3..].split('"').next().and_then(|v| v.parse().ok()))
            .unwrap_or(0.0)
    };
    (grab("width"), grab("height"))
}
