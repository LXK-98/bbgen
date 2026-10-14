//! Wires for every net the file leaves unconnected (bbgen/autowire.py; its docstring says how).
//!
//! A faithful port: the same pieces joined in the same order, by the same costs, so the wires come out the
//! same. Where Python leans on object identity (a wire taken out and put back, a lead list holding wires that
//! are out), wires carry a stable `id`.

use crate::board::{Hole, Row, Strip, StripKind, Xy, RAILS, ROWS};
use crate::check::Dsu;
use crate::circuit::{aligned, pymax, pymin, Circuit, End, Issue, Net, Node, Opt, PinId, Wire, K};
use crate::costs::*;
use crate::dump;
use crate::measure::cross;
use crate::parts::Kind;
use crate::pyf;
use crate::IndexMap;
use serde_json::{json, Value};
use std::collections::BinaryHeap;
use crate::{HashMap, HashSet};

const LEAD_PASSES: usize = 3;
const END_GROUP: usize = 5;
const RELAY_HOPS: usize = 3;
const RELAY_LIMIT: f64 = 80.0;

/// What a wire end is written as in the checklist: a pin, a rail, or (a strip a route passed through) a hole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spec {
    Pin(PinId),
    Kind(StripKind),
    Hole(Hole),
}

/// (spec, option, node): one place a wire could join a node.
#[derive(Clone, Copy, Debug)]
pub struct Cand {
    pub spec: Spec,
    pub o: Opt,
    pub node: Node,
}

/// A cost key as _cost returns it: (cost, nearness, p.xy, q.xy), compared as a tuple.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key(pub f64, pub f64, pub Xy, pub Xy);

impl Key {
    fn lt(&self, other: &Key) -> bool {
        (self.0, self.1, self.2 .0, self.2 .1, self.3 .0, self.3 .1)
            .partial_cmp(&(other.0, other.1, other.2 .0, other.2 .1, other.3 .0, other.3 .1)) == Some(std::cmp::Ordering::Less)
    }

    fn json(&self) -> Value {
        json!([dump::num(self.0), dump::num(self.1), dump::xy(self.2), dump::xy(self.3)])
    }
}

fn node(c: &Circuit, hole: Hole) -> Node {
    Node::Strip(c.boards[hole.board as usize].strip(hole))
}

fn pin_node(c: &Circuit, p: PinId) -> Option<Node> {
    let pin = c.pin(p);
    if let Some(h) = pin.hole {
        return Some(node(c, h));
    }
    if c.parts[p.part as usize].kind == Kind::Offboard {
        return Some(Node::Off(p.part, pin.num));
    }
    None
}

fn end_node(c: &Circuit, e: &End) -> Node {
    match e.pin {
        Some(p) => Node::Off(p.part, c.pin(p).num),
        None => node(c, e.hole.unwrap()),
    }
}

fn node_json(c: &Circuit, n: &Node) -> Value {
    match n {
        Node::Strip(s) => json!(["strip", c.boards[s.board as usize].id, s.kind.name(), s.n]),
        Node::Off(p, num) => json!(["off", c.parts[*p as usize].r#ref, num]),
    }
}

fn spec_str(c: &Circuit, s: Spec) -> String {
    match s {
        Spec::Pin(p) => format!("{}.{}", c.parts[p.part as usize].r#ref, c.pin(p).num),
        Spec::Kind(k) => k.name().to_string(),
        Spec::Hole(h) => c.hole_name(h),
    }
}

fn is_rail_strip(n: &Node) -> bool {
    matches!(n, Node::Strip(Strip { kind: StripKind::Rail(_), .. }))
}

fn has_rail(group: &[Node]) -> bool {
    group.iter().any(is_rail_strip)
}

fn rail_of(n: &Node) -> Option<Row> {
    match n {
        Node::Strip(s) => s.kind.rail(),
        _ => None,
    }
}

type NodePins = IndexMap<Node, Vec<PinId>>;

pub fn autowire(c: &mut Circuit) {
    let (mut dsu, node_pins) = joined(c);
    c.ends = c.wires.iter().flat_map(|w| [&w.a, &w.b]).flatten().filter(|e| e.hole.is_some()).map(|e| pyf::key(c.end_xy(e))).collect();
    let mut lain = HashSet::default();
    for w in &c.wires {
        lain.extend(c.lies_over(w));
    }
    c.lain = lain;
    let in_nets: HashSet<PinId> = c.nets.values().flat_map(|n| n.pins.iter().copied()).collect();
    c.pin_strips = node_pins.iter().filter(|(n, pins)| n.strip().is_some() && pins.iter().any(|p| in_nets.contains(p)))
        .map(|(n, _)| n.strip().unwrap()).collect();
    let mut strip_at = HashMap::default();
    for b in &c.boards {
        for h in b.holes() {
            strip_at.insert(pyf::key(b.xy(h)), b.strip(h));
        }
    }
    c.strip_at = strip_at;
    let rails = rails(c, &mut dsu, &node_pins);
    let first = c.wires.len();
    let mut order: Vec<String> = c.nets.keys().cloned().collect();
    let spans: HashMap<String, f64> = order.iter().map(|n| (n.clone(), span(c, &c.nets[n]))).collect();
    let has = |n: &String| rails.get(n).map(|r| !r.is_empty()).unwrap_or(false);
    order.sort_by(|a, b| (has(a), spans[a]).partial_cmp(&(has(b), spans[b])).unwrap());
    if dump::active() {
        let r: serde_json::Map<String, Value> = rails.iter().map(|(n, s)| (n.clone(), Value::Array(s.iter().map(|x| node_json(c, x)).collect()))).collect();
        dump::event("rails", vec![("rails", Value::Object(r)), ("order", json!(order))]);
    }
    for flying in [false, true] {
        for name in &order {
            let net = c.nets[name].clone();
            let rs = rails.get(name).cloned().unwrap_or_default();
            wire_net(c, &mut dsu, &net, &rs, &node_pins, flying);
        }
    }
    settle_leads(c, &node_pins, first);
    let net_pos: HashMap<&String, usize> = c.nets.keys().enumerate().map(|(i, n)| (n, i)).collect();
    let mut built: Vec<Wire> = c.wires[first..].to_vec();
    built.sort_by_key(|w| (w.flex, !has(&w.group), net_pos[&w.group], w.num));
    renumber(c, built, first);
    if dump::active() {
        let ws: Vec<Value> = c.wires[first..].iter().map(|w| dumped(c, w)).collect();
        dump::event("wires", vec![("wires", Value::Array(ws))]);
    }
    let mut groups: Vec<String> = Vec::new();
    for w in &c.wires[first..] {
        if !groups.contains(&w.group) {
            groups.push(w.group.clone());
        }
    }
    for g in groups {
        if !c.groups.contains(&g) {
            c.groups.push(g);
        }
    }
}

fn dumped(c: &Circuit, w: &Wire) -> Value {
    let ends: Vec<Value> = [&w.a, &w.b].iter().map(|e| {
        let e = e.as_ref().unwrap();
        match e.hole {
            Some(h) => json!(c.hole_name(h)),
            None => json!(c.pin_str(e.pin.unwrap())),
        }
    }).collect();
    json!({"num": w.num, "net": w.group, "flex": w.flex, "via": w.via, "ends": ends})
}

fn dumped_fields(c: &Circuit, w: &Wire) -> Vec<(&'static str, Value)> {
    match dumped(c, w) {
        Value::Object(m) => {
            let keys = ["num", "net", "flex", "via", "ends"];
            keys.iter().map(|k| (*k, m[*k].clone())).collect()
        }
        _ => unreachable!(),
    }
}

/// How far a net's pins on the board are spread (half its bounding box's perimeter).
fn span(c: &Circuit, net: &Net) -> f64 {
    let pts: Vec<Xy> = net.pins.iter().filter_map(|&p| {
        let pin = c.pin(p);
        if pin.hole.is_some() { pin.xy } else { None }
    }).collect();
    if pts.len() < 2 {
        return 0.0;
    }
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    (crate::parts::max(&xs) - crate::parts::min(&xs)) + (crate::parts::max(&ys) - crate::parts::min(&ys))
}

/// What's connected so far (the strips, the parts' own buses and the wires in), and the pins in each node.
fn joined(c: &Circuit) -> (Dsu<Node>, NodePins) {
    let mut dsu = Dsu::new();
    let mut node_pins: NodePins = IndexMap::default();
    for (pi, part) in c.parts.iter().enumerate() {
        for k in 0..part.pins.len() {
            let id = PinId { part: pi as u16, pin: k as u16 };
            if let Some(n) = pin_node(c, id) {
                dsu.find(n);
                node_pins.entry(n).or_default().push(id);
            }
        }
        for bus in &part.buses {
            let nodes: Vec<Node> = bus.iter().filter_map(|&k| pin_node(c, PinId { part: pi as u16, pin: k as u16 })).collect();
            for &n in nodes.iter().skip(1) {
                dsu.union(nodes[0], n);
            }
        }
    }
    for w in &c.wires {
        if let (Some(a), Some(b)) = (&w.a, &w.b) {
            dsu.union(end_node(c, a), end_node(c, b));
        }
    }
    (dsu, node_pins)
}

fn wire_index(c: &Circuit, id: u64) -> Option<usize> {
    c.wires.iter().position(|w| w.id == id)
}

/// Lay each flying lead that crosses another again, now that every lead is in, keeping the new one only if it
/// costs less; then let two leads of one net that cross swap holes.
fn settle_leads(c: &mut Circuit, node_pins: &NodePins, first: usize) {
    // wires taken out and not (yet) put back, which the leads list still holds
    let mut out: HashMap<u64, Wire> = HashMap::default();
    let get = |c: &Circuit, out: &HashMap<u64, Wire>, id: u64| -> Wire {
        wire_index(c, id).map(|i| c.wires[i].clone()).unwrap_or_else(|| out[&id].clone())
    };
    for _ in 0..LEAD_PASSES {
        let mut moved = false;
        let mut leads: Vec<u64> = c.wires[first..].iter().filter(|w| {
            w.auto && w.a.is_some() && w.b.is_some() && (w.a.as_ref().unwrap().pin.is_some() != w.b.as_ref().unwrap().pin.is_some())
        }).map(|w| w.id).collect();
        let line_of = |c: &Circuit, w: &Wire| (c.end_xy(w.a.as_ref().unwrap()), c.end_xy(w.b.as_ref().unwrap()));
        let crossed = |c: &Circuit, out: &HashMap<u64, Wire>, leads: &[u64], id: u64| -> usize {
            let w = get(c, out, id);
            let s = line_of(c, &w);
            leads.iter().filter(|&&x| x != id).map(|&x| cross(s, line_of(c, &get(c, out, x))) as usize).sum()
        };
        let mut order: Vec<(i64, i64, u64)> = leads.iter().map(|&id| {
            (-(crossed(c, &out, &leads, id) as i64), get(c, &out, id).num, id)
        }).collect();
        order.sort_by_key(|t| (t.0, t.1));
        for (_, _, wid) in order {
            if crossed(c, &out, &leads, wid) == 0 {
                continue;
            }
            let w = get(c, &out, wid);
            let net = c.nets[&w.group].clone();
            let (pin_end, hole_end) = if w.a.as_ref().unwrap().pin.is_some() {
                (w.a.clone().unwrap(), w.b.clone().unwrap())
            } else {
                (w.b.clone().unwrap(), w.a.clone().unwrap())
            };
            unwire(c, wid);
            out.insert(wid, w.clone());
            let hxy = c.end_xy(&hole_end);
            let pp = pin_end.pin.unwrap();
            let mut po = Opt::at(None, c.pin(pp).xy.unwrap());
            po.pin = Some(pp);
            let mut ho = Opt::at(hole_end.hole, hxy);
            ho.covered = c.covered.contains_key(&pyf::key(hxy));
            let old = cost(c, &po, &ho, false).0;
            let (mut dsu, _) = joined(c);
            let mut mine: HashSet<Node> = HashSet::default();
            for &p in &net.pins {
                if let Some(n) = pin_node(c, p) {
                    if n.strip().is_some() {
                        mine.insert(dsu.find(n));
                    }
                }
            }
            let keys: Vec<Node> = dsu.parent.keys().copied().collect();
            let mut board_rails: Vec<Node> = Vec::new();
            for n in keys {
                if is_rail_strip(&n) && mine.contains(&dsu.find(n)) {
                    board_rails.push(n);
                }
            }
            join(c, &mut dsu, &net, &board_rails, node_pins, true);
            let new: Vec<u64> = c.wires.iter().filter(|x| {
                !leads.contains(&x.id) && x.group == net.name && x.auto && x.a.is_some() && x.b.is_some()
                    && (x.a.as_ref().unwrap().pin == Some(pp) || x.b.as_ref().unwrap().pin == Some(pp))
            }).map(|x| x.id).collect();
            if let Some(&xid) = new.first() {
                let x = c.wires[wire_index(c, xid).unwrap()].clone();
                let (xp, xh) = if x.a.as_ref().unwrap().pin.is_some() { (x.a.clone().unwrap(), x.b.clone().unwrap()) } else { (x.b.clone().unwrap(), x.a.clone().unwrap()) };
                let xxy = c.end_xy(&xh);
                let mut po = Opt::at(None, c.pin(xp.pin.unwrap()).xy.unwrap());
                po.pin = xp.pin;
                let mut ho = Opt::at(xh.hole, xxy);
                ho.covered = c.covered.contains_key(&pyf::key(xxy));
                let k = cost(c, &po, &ho, false).0;
                if k < old - 1e-9 {
                    let i = leads.iter().position(|&l| l == wid).unwrap();
                    leads[i] = xid;
                    moved = true;
                    continue;
                }
                unwire(c, xid);
            }
            let w = out.remove(&wid).unwrap();
            put_back(c, w);
        }
        for i in 0..leads.len() {
            for j in i + 1..leads.len() {
                let (w, x) = (get(c, &out, leads[i]), get(c, &out, leads[j]));
                if w.group != x.group || !cross(line_of(c, &w), line_of(c, &x)) {
                    continue;
                }
                let before: usize = leads.iter().map(|&y| crossed(c, &out, &leads, y)).sum();
                swap_holes(c, leads[i], leads[j]);
                let after: usize = leads.iter().map(|&y| crossed(c, &out, &leads, y)).sum();
                if after < before {
                    moved = true;
                } else {
                    swap_holes(c, leads[i], leads[j]);
                }
            }
        }
        if !moved {
            return;
        }
    }
}

/// Two flying leads trade the holes their board ends are in.
fn swap_holes(c: &mut Circuit, wid: u64, xid: u64) {
    let (wi, xi) = (wire_index(c, wid).unwrap(), wire_index(c, xid).unwrap());
    if dump::active() {
        dump::event("swap", vec![("nums", json!([c.wires[wi].num, c.wires[xi].num]))]);
    }
    let board_end = |w: &Wire| if w.a.as_ref().unwrap().pin.is_some() { 1 } else { 0 };
    let (bw, bx) = (board_end(&c.wires[wi]), board_end(&c.wires[xi]));
    let ew = if bw == 1 { c.wires[wi].b.clone().unwrap() } else { c.wires[wi].a.clone().unwrap() };
    let ex = if bx == 1 { c.wires[xi].b.clone().unwrap() } else { c.wires[xi].a.clone().unwrap() };
    let nw = End { spec: ex.spec.clone(), hole: ex.hole, pin: ew.pin };
    let nx = End { spec: ew.spec.clone(), hole: ew.hole, pin: ex.pin };
    if bw == 1 { c.wires[wi].b = Some(nw.clone()) } else { c.wires[wi].a = Some(nw.clone()) }
    if bx == 1 { c.wires[xi].b = Some(nx.clone()) } else { c.wires[xi].a = Some(nx.clone()) }
    for (end, num) in [(nw, c.wires[wi].num), (nx, c.wires[xi].num)] {
        let h = end.hole.unwrap();
        let who = format!("W{}", num);
        c.occupied.insert(h, who.clone());
        c.used.insert(pyf::key(c.xy(h)), who);
    }
}

/// Put a wire taken out back exactly as it was, under a number of its own.
fn put_back(c: &mut Circuit, mut w: Wire) {
    w.num = c.wires.iter().map(|x| x.num).max().unwrap_or(0) + 1;
    if dump::active() {
        dump::event("put back", dumped_fields(c, &w));
    }
    let who = format!("W{}", w.num);
    c.wired += 1;
    for e in [&w.a, &w.b].into_iter().flatten() {
        if let Some(h) = e.hole {
            c.occupy(h, &who);
        }
    }
    for e in [&w.a, &w.b].into_iter().flatten() {
        if e.hole.is_some() {
            c.ends.insert(pyf::key(c.end_xy(e)));
        }
    }
    if !w.flex && w.on_board() {
        let pts = c.wire_points(&w);
        for s in pts.windows(2) {
            c.add_segment(s[0], s[1], &who);
        }
        let over = c.lies_over(&w);
        c.lain.extend(over);
    }
    c.wires.push(w);
}

fn renumber(c: &mut Circuit, wires: Vec<Wire>, first: usize) {
    let names: HashMap<String, String> = wires.iter().enumerate().map(|(i, w)| (format!("W{}", w.num), format!("W{}", first + i + 1))).collect();
    let mut wires = wires;
    for (i, w) in wires.iter_mut().enumerate() {
        w.num = (first + i + 1) as i64;
    }
    c.wires.truncate(first);
    c.wires.extend(wires);
    for who in c.occupied.values_mut() {
        if let Some(n) = names.get(who) {
            *who = n.clone();
        }
    }
    for who in c.used.values_mut() {
        if let Some(n) = names.get(who) {
            *who = n.clone();
        }
    }
    for s in c.segments.iter_mut() {
        if let Some(n) = names.get(&s.2) {
            s.2 = n.clone();
        }
    }
    c.reindex();
}

fn rail_strips(c: &Circuit, spec: &str) -> Option<Vec<Node>> {
    let (bid, row, col) = crate::board::parse_hole(spec)?;
    if !row.is_rail() || col.is_some() {
        return None;
    }
    let bi = match bid {
        None => c.default_board,
        Some(id) => c.board_by_id(&id)?,
    };
    let board = &c.boards[bi as usize];
    let mut out: Vec<Node> = Vec::new();
    for h in board.holes() {
        if h.row == row {
            let n = node(c, h);
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.sort_by_key(|n| n.strip().unwrap().n);
    Some(out)
}

/// The rail strips each net owns.
fn rails(c: &mut Circuit, dsu: &mut Dsu<Node>, node_pins: &NodePins) -> IndexMap<String, Vec<Node>> {
    let mut every: Vec<Node> = Vec::new();
    for b in &c.boards {
        for h in b.holes() {
            if h.row.is_rail() {
                let n = Node::Strip(b.strip(h));
                if !every.contains(&n) {
                    every.push(n);
                }
            }
        }
    }
    let mut owner: IndexMap<Node, String> = IndexMap::default();
    let nets: Vec<Net> = c.nets.values().cloned().collect();
    for net in &nets {
        let roots: HashSet<Node> = net.pins.iter().filter_map(|&p| pin_node(c, p)).map(|n| dsu.find(n)).collect();
        for &s in &every {
            if roots.contains(&dsu.find(s)) {
                owner.insert(s, net.name.clone());
            }
        }
    }
    for net in &nets {
        for spec in &net.rails {
            let strips = match rail_strips(c, spec) {
                Some(s) => s,
                None => {
                    c.issues.push(Issue::new("error", "BAD_RAIL", format!(
                        "nets.{}: rail {} is not a rail (top+, top-, bot+ or bot-, optionally 2:top+)", net.name, crate::val::repr_str(spec))));
                    continue;
                }
            };
            for s in strips {
                if let Some(o) = owner.get(&s) {
                    if *o != net.name {
                        c.issues.push(Issue::new("error", "BAD_RAIL", format!("nets.{}: {} already carries {}", net.name, spec, o)));
                        break;
                    }
                }
                owner.insert(s, net.name.clone());
            }
        }
    }
    for (s, name) in &owner {
        if let Some(sign) = c.nets[name].polarity {
            let row = rail_of(s).unwrap();
            if row.sign() != sign {
                c.issues.push(Issue::new("error", "BAD_RAIL", format!("nets.{} is {} but is on the {} rail", name,
                                                                      if sign == '+' { "a supply" } else { "a ground" }, row.name())));
            }
        }
    }
    let claimed: HashSet<String> = owner.values().cloned().collect();
    for sign in ['-', '+'] {
        let mut ns: Vec<&Net> = nets.iter().filter(|n| n.polarity == Some(sign) && !claimed.contains(&n.name)).collect();
        let pos = |n: &&Net| c.rail_order.iter().position(|o| *o == n.name).unwrap_or(c.rail_order.len());
        ns.sort_by_key(pos);
        let free: Vec<Node> = every.iter().copied().filter(|s| rail_of(s).unwrap().sign() == sign && !owner.contains_key(s)).collect();
        let mut rows: Vec<Row> = Vec::new();
        for s in &free {
            let r = rail_of(s).unwrap();
            if !rows.contains(&r) {
                rows.push(r);
            }
        }
        let shares: Vec<Vec<Node>> = if ns.len() == 1 && sign == '-' {
            vec![free.clone()]
        } else if ns.len() <= rows.len() {
            rows.iter().map(|r| free.iter().copied().filter(|s| rail_of(s) == Some(*r)).collect()).collect()
        } else {
            free.iter().map(|s| vec![*s]).collect()
        };
        for (net, share) in ns.iter().zip(&shares) {
            for s in share {
                owner.insert(*s, net.name.clone());
            }
        }
        for net in ns.iter().skip(shares.len()) {
            c.issues.push(Issue::new("error", "BAD_RAIL", format!("nets.{}: no {} rail is left for it", net.name, sign)));
        }
    }
    let mut lone: Vec<(Net, char)> = Vec::new();
    for sign in ['-', '+'] {
        let ns: Vec<&Net> = nets.iter().filter(|n| n.polarity == Some(sign) && n.rails.is_empty()).collect();
        if ns.len() == 1 && nets.iter().filter(|n| n.polarity == Some(sign)).count() == 1 {
            lone.push((ns[0].clone(), sign));
        }
    }
    for (net, sign) in lone {
        let mine: HashSet<Row> = owner.iter().filter(|(_, n)| **n == net.name).map(|(s, _)| rail_of(s).unwrap()).collect();
        if mine.is_empty() {
            continue;
        }
        let mut pieces: IndexMap<Node, Vec<Node>> = IndexMap::default();
        for &p in &net.pins {
            if let Some(n) = pin_node(c, p) {
                let r = dsu.find(n);
                pieces.entry(r).or_default().push(n);
            }
        }
        for (side, half) in [("top", StripKind::AE), ("bot", StripKind::FJ)] {
            let rail = Row::parse(&format!("{}{}", side, sign)).unwrap();
            let strips: Vec<Node> = every.iter().copied().filter(|s| rail_of(s) == Some(rail)).collect();
            if mine.contains(&rail) || strips.is_empty() || strips.iter().any(|s| owner.contains_key(s)) {
                continue;
            }
            let own: Vec<Node> = owner.iter().filter(|(_, n)| **n == net.name).map(|(s, _)| *s).collect();
            let mut stranded: Vec<usize> = Vec::new();
            for (gi, g) in pieces.values().enumerate() {
                let all_half = g.iter().all(|n| matches!(n, Node::Strip(s) if s.kind == half));
                if !all_half {
                    continue;
                }
                let g0 = dsu.find(g[0]);
                let mut joined_own = false;
                for &s in &own {
                    if dsu.find(s) == g0 {
                        joined_own = true;
                        break;
                    }
                }
                if !joined_own {
                    stranded.push(gi);
                }
            }
            let mut gain = 0.0f64;
            if !stranded.is_empty() {
                let groups: Vec<&Vec<Node>> = pieces.values().collect();
                let group: Vec<Node> = stranded.iter().flat_map(|&i| groups[i].iter().copied()).collect();
                let mut rest: Vec<Node> = (0..groups.len()).filter(|i| !stranded.contains(i)).flat_map(|i| groups[i].iter().copied()).collect();
                rest.extend(own.iter().copied());
                gain = pymax(0.0, cheapest(c, &net, node_pins, &group, &rest) - cheapest(c, &net, node_pins, &group, &strips));
            }
            for &p in &net.pins {
                if c.parts[p.part as usize].kind == Kind::Offboard && c.pin(p).xy.is_some() {
                    gain += pymax(0.0, lead_estimate(c, p, &own) - lead_estimate(c, p, &strips));
                }
            }
            let link = if gain != 0.0 {
                let a: Vec<Node> = own.iter().copied().filter(|s| rail_of(s).unwrap().sign() == sign).collect();
                link_cost(c, &net, node_pins, &a, &strips)
            } else {
                0.0
            };
            if gain > link {
                for s in strips {
                    owner.insert(s, net.name.clone());
                }
            }
        }
    }
    let mut out: IndexMap<String, Vec<Node>> = IndexMap::default();
    for (s, name) in owner {
        out.entry(name).or_default().push(s);
    }
    out
}

fn wire_net(c: &mut Circuit, dsu: &mut Dsu<Node>, net: &Net, rails: &[Node], node_pins: &NodePins, flying: bool) {
    join(c, dsu, net, rails, node_pins, flying);
    if flying {
        let named: Vec<String> = net.rails.iter().map(|s| s.rsplit(':').next().unwrap().to_string()).collect();
        let rs: Vec<Node> = rails.iter().copied().filter(|s| !named.iter().any(|n| Some(n.as_str()) == rail_of(s).map(|r| r.name()))).collect();
        prune(c, &net.name, &rs, node_pins);
    }
}

/// Items in order of a float key, ties in the order they came (Python's stable sort), sorted lazily: a heap
/// hands them out one at a time, and only as many as the search reads get sorted.
struct LazySorted<T: Copy> {
    heap: BinaryHeap<std::cmp::Reverse<(OrdF, u32)>>,
    items: Vec<T>,
    sorted: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct OrdF(f64);
impl Eq for OrdF {}
impl Ord for OrdF {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&o.0).unwrap()
    }
}

impl<T: Copy> LazySorted<T> {
    fn new(items: Vec<T>, key: impl Fn(&T) -> f64) -> Self {
        let keyed: Vec<std::cmp::Reverse<(OrdF, u32)>> = items.iter().enumerate().map(|(i, t)| std::cmp::Reverse((OrdF(key(t)), i as u32))).collect();
        LazySorted { heap: BinaryHeap::from(keyed), items, sorted: Vec::new() }
    }

    /// The i-th item in order, if there are that many.
    fn get(&mut self, i: usize) -> Option<T> {
        while self.sorted.len() <= i {
            let std::cmp::Reverse((_, k)) = self.heap.pop()?;
            self.sorted.push(k);
        }
        Some(self.items[self.sorted[i] as usize])
    }
}

/// A pair of ends to weigh: options[gi][pi] and options[gj][qj] (indices, so sorting moves little).
#[derive(Clone, Copy)]
struct Pair {
    bound: f64,
    gi: u32,
    pi: u32,
    gj: u32,
    qj: u32,
    unavoidable: bool,
}

fn join(c: &mut Circuit, dsu: &mut Dsu<Node>, net: &Net, rails: &[Node], node_pins: &NodePins, flying: bool) {
    let mut nodes: Vec<Node> = Vec::new();
    for n in net.pins.iter().filter_map(|&p| pin_node(c, p)).chain(rails.iter().copied()) {
        if !nodes.contains(&n) {
            nodes.push(n);
        }
    }
    let offboard = net.pins.iter().any(|p| c.parts[p.part as usize].kind == Kind::Offboard);
    if !flying {
        nodes.retain(|n| !matches!(n, Node::Off(..)));
    }
    loop {
        let mut pieces: IndexMap<Node, Vec<Node>> = IndexMap::default();
        for &n in &nodes {
            let r = dsu.find(n);
            pieces.entry(r).or_default().push(n);
        }
        if pieces.len() < 2 {
            if !flying && !offboard {
                for n in &nodes {
                    if let Some(s) = n.strip() {
                        c.pin_strips.remove(&s);
                    }
                }
            }
            return;
        }
        let groups: Vec<Vec<Node>> = pieces.into_values().collect();
        let options: Vec<Vec<Cand>> = groups.iter().map(|g| g.iter().flat_map(|&n| options(c, n, net, node_pins)).collect()).collect();
        let lone: Vec<bool> = groups.iter().map(|g| g.len() == 1 && matches!(g[0], Node::Off(..))).collect();
        let mut pairs: Vec<Pair> = Vec::new();
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                if flying && lone[i] == lone[j] {
                    continue;
                }
                let (all_i, all_j) = (&options[i], &options[j]);
                let mut oi: Vec<u32> = (0..all_i.len() as u32).collect();
                let mut oj: Vec<u32> = (0..all_j.len() as u32).collect();
                if flying {
                    oi.retain(|&k| !all_i[k as usize].o.under);
                    oj.retain(|&k| !all_j[k as usize].o.under);
                    let on_rail = |o: &Cand| o.o.hole.map(|h| h.is_rail()).unwrap_or(false);
                    let ri: Vec<u32> = oi.iter().copied().filter(|&k| on_rail(&all_i[k as usize])).collect();
                    if !ri.is_empty() {
                        oi = ri;
                    }
                    let rj: Vec<u32> = oj.iter().copied().filter(|&k| on_rail(&all_j[k as usize])).collect();
                    if !rj.is_empty() {
                        oj = rj;
                    }
                }
                if !flying && has_rail(&groups[i]) && has_rail(&groups[j]) {
                    let (ei, ej) = (rail_end_indices(c, all_i, &oi), rail_end_indices(c, all_j, &oj));
                    if !ei.is_empty() && !ej.is_empty() {
                        oi = ei.clone();
                        oj = ej.clone();
                        let mut col = None;
                        'outer: for &p in &ei {
                            for &q in &ej {
                                let (p, q) = (&all_i[p as usize].o, &all_j[q as usize].o);
                                if p.hole.unwrap().col == q.hole.unwrap().col && lies_clear(c, p.xy, q.xy) {
                                    col = Some(p.hole.unwrap().col);
                                    break 'outer;
                                }
                            }
                        }
                        if let Some(col) = col {
                            oi = ei.iter().copied().filter(|&p| all_i[p as usize].o.hole.unwrap().col == col).collect();
                            oj = ej.iter().copied().filter(|&q| all_j[q as usize].o.hole.unwrap().col == col).collect();
                        }
                    }
                }
                let si = strip_holes_of(c, oi.iter().map(|&k| &all_i[k as usize]));
                let sj = strip_holes_of(c, oj.iter().map(|&k| &all_j[k as usize]));
                let unavoidable = !si.iter().any(|&a| sj.iter().any(|&b| could_be_straight(c, a, b)));
                let short = !flying && (groups.len() > 2 || offboard) && !has_rail(&groups[i]) && !has_rail(&groups[j])
                    && options[i].len() + options[j].len() <= 2;
                if short {
                    continue;
                }
                for &p in &oi {
                    let po = &all_i[p as usize].o;
                    for &q in &oj {
                        let qo = &all_j[q as usize].o;
                        if po.pin.is_none() || qo.pin.is_none() {
                            pairs.push(Pair { bound: bound(po, qo), gi: i as u32, pi: p, gj: j as u32, qj: q, unavoidable });
                        }
                    }
                }
            }
        }
        let mut pairs = LazySorted::new(pairs, |p| p.bound);
        let mut best: Option<(Key, Cand, Cand)> = None;
        let mut flex = false;
        if c.flex {
            let mut found: Option<(Key, Cand, Cand)> = None;
            let mut n = 0;
            while let Some(pr) = pairs.get(n) {
                n += 1;
                if let Some(f) = &found {
                    if pr.bound + FLEX_COST > f.0 .0 + 1e-9 {
                        break;
                    }
                }
                let (pc, qc) = (&options[pr.gi as usize][pr.pi as usize], &options[pr.gj as usize][pr.qj as usize]);
                let (p, q) = (&pc.o, &qc.o);
                if p.hole.is_some() && q.hole.is_some() && !over_keepout(c, p.xy, q.xy) && !p.under && !q.under {
                    let key = Key(pyf::round(pr.bound + FLEX_COST + flex_over(c, p.xy, q.xy), 6), pyf::round(p.near + q.near, 6), p.xy, q.xy);
                    if found.as_ref().map(|f| key.lt(&f.0)).unwrap_or(true) {
                        found = Some((key, *pc, *qc));
                    }
                }
            }
            if found.is_some() {
                best = found;
                flex = true;
            }
        }
        let mut n = 0;
        while let Some(pr) = pairs.get(n) {
            n += 1;
            if let Some(b) = &best {
                if pr.bound > b.0 .0 + 1e-9 {
                    break;
                }
            }
            let (pc, qc) = (&options[pr.gi as usize][pr.pi as usize], &options[pr.gj as usize][pr.qj as usize]);
            let key = cost(c, &pc.o, &qc.o, pr.unavoidable);
            if key.0 < f64::INFINITY && best.as_ref().map(|b| key.lt(&b.0)).unwrap_or(true) {
                best = Some((key, *pc, *qc));
                flex = false;
            }
        }
        if !flying {
            let try_relay = match &best {
                None => true,
                Some(b) => flex || b.0 .0 > bound(&b.1.o, &b.2.o) + RELAY_COST,
            };
            if try_relay {
                let limit = best.as_ref().map(|b| b.0 .0).unwrap_or(RELAY_LIMIT);
                let froms: Option<Vec<usize>> = best.as_ref().map(|b| (0..groups.len()).filter(|&i| groups[i].contains(&b.1.node)).collect());
                if let Some(route) = relay(c, &options, limit, froms.as_deref()) {
                    if lay_route(c, dsu, net, &route, &mut nodes) {
                        continue;
                    }
                }
            }
        }
        let (key, end_a, end_b) = match best {
            None => return,
            Some(b) => b,
        };
        if dump::active() {
            dump::event("cost", vec![("net", json!(net.name)), ("key", key.json()), ("flex", json!(flex))]);
        }
        lay(c, Some(dsu), net, &end_a, &end_b, flex, flying, false);
    }
}

/// Put in one wire between two ends (joining their nodes, unless dsu is None).
#[allow(clippy::too_many_arguments)]
fn lay(c: &mut Circuit, dsu: Option<&mut Dsu<Node>>, net: &Net, end_a: &Cand, end_b: &Cand, flex: bool, _flying: bool, _quiet: bool) -> u64 {
    let num = c.wires.iter().map(|x| x.num).max().unwrap_or(0) + 1;
    let who = format!("W{}", num);
    let (sa, sb) = (spec_str(c, end_a.spec), spec_str(c, end_b.spec));
    let a = c.take(&end_a.o, &who, &sa);
    let b = c.take(&end_b.o, &who, &sb);
    c.next_id += 1;
    let mut w = Wire { id: c.next_id, num, group: net.name.clone(), spec: (sa, sb), color: None, via: vec![], slant: None,
                       auto: true, flex, a: Some(a), b: Some(b), via_xy: vec![] };
    c.wired += 1;
    for e in [&w.a, &w.b].into_iter().flatten() {
        if e.hole.is_some() {
            c.ends.insert(pyf::key(c.end_xy(e)));
        }
    }
    let id = w.id;
    if flex {
        if dump::active() {
            dump::event("wire", dumped_fields(c, &w));
        }
        c.wires.push(w);
        if let Some(d) = dsu {
            d.union(end_a.node, end_b.node);
        }
        return id;
    }
    if w.on_board() {
        let (pa, pb) = (c.end_xy(w.a.as_ref().unwrap()), c.end_xy(w.b.as_ref().unwrap()));
        if c.crosses_any_body(pa, pb) {
            w.via = c.detour(w.a.as_ref().unwrap().hole.unwrap(), w.b.as_ref().unwrap().hole.unwrap(), &who).unwrap_or_default();
            w.via_xy = w.via.iter().map(|v| {
                let h = c.hole_of(v).unwrap();
                c.xy(h)
            }).collect();
        }
    }
    let pts = c.wire_points(&w);
    for s in pts.windows(2) {
        c.add_segment(s[0], s[1], &who);
    }
    if dump::active() {
        dump::event("wire", dumped_fields(c, &w));
    }
    let over = c.lies_over(&w);
    c.wires.push(w);
    c.lain.extend(over);
    if let Some(d) = dsu {
        d.union(end_a.node, end_b.node);
    }
    id
}

/// The cheapest route of straight, clear wires from one piece of the net to another through strips nothing
/// uses yet, costing less than limit.
fn relay(c: &mut Circuit, options: &[Vec<Cand>], limit: f64, froms: Option<&[usize]>) -> Option<Vec<(Cand, Cand)>> {
    let mut target: IndexMap<Hole, (usize, Cand)> = IndexMap::default();
    for (gi, opts) in options.iter().enumerate() {
        for o in opts {
            if let Some(h) = o.o.hole {
                target.entry(h).or_insert((gi, *o));
            }
        }
    }
    if target.len() < 2 {
        return None;
    }
    let empty = empty_strips(c);
    let mut best: Option<Vec<(Cand, Cand)>> = None;
    let mut best_cost = limit - 1e-9;
    let mut cache = ReachCache::default();
    for (gi, opts) in options.iter().enumerate() {
        if let Some(f) = froms {
            if !f.contains(&gi) {
                continue;
            }
        }
        let starts: Vec<Cand> = opts.iter().copied().filter(|o| o.o.hole.is_some()).collect();
        let mut xs: Vec<f64> = target.iter().filter(|(_, (gj, _))| *gj != gi).map(|(h, _)| c.xy(*h).0).collect();
        if starts.is_empty() || xs.is_empty() {
            continue;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ahead = |xy: Xy| -> f64 {
            let i = xs.partition_point(|&x| x < xy.0);
            let mut m = f64::INFINITY;
            let mut first = true;
            for k in [i as isize - 1, i as isize] {
                if k >= 0 && (k as usize) < xs.len() {
                    let d = (xs[k as usize] - xy.0).abs();
                    if first || d < m {
                        m = d;
                        first = false;
                    }
                }
            }
            m
        };
        let mut floor = f64::INFINITY;
        for (k, s) in starts.iter().enumerate() {
            let a = ahead(s.o.xy);
            if k == 0 || a < floor {
                floor = a;
            }
        }
        floor += RELAY_COST;
        let mut heap: BinaryHeap<State> = BinaryHeap::new();
        heap.push(State { f: floor, tie: 0, cost: 0.0, hops: 0, ends: starts, path: vec![] });
        let mut seen: HashMap<Strip, f64> = HashMap::default();
        let mut tie = 1u64;
        while let Some(st) = heap.pop() {
            if st.f >= best_cost {
                break;
            }
            for end in &st.ends {
                let o = end.o;
                let reach = reach(c, o.hole.unwrap(), &mut cache);
                for (t, length) in reach.iter().copied() {
                    let step = st.cost + length + COVERED_COST * (o.covered as i32 as f64);
                    if step >= best_cost {
                        continue;
                    }
                    if let Some(&(hg, hc)) = target.get(&t) {
                        if hg == gi || st.path.is_empty() {
                            continue;
                        }
                        let mut total = step + COVERED_COST * (hc.o.covered as i32 as f64);
                        if total < best_cost {
                            let mut route = st.path.clone();
                            route.push((*end, hc));
                            let crossed: usize = route.iter().map(|(x, y)| wires_crossed(c, x.o.xy, y.o.xy)).sum();
                            total += CROSS_WIRE_COST * crossed as f64;
                            if total < best_cost {
                                best = Some(route);
                                best_cost = total;
                            }
                        }
                        continue;
                    }
                    if st.hops + 1 >= RELAY_HOPS {
                        continue;
                    }
                    let board = &c.boards[t.board as usize];
                    let strip = board.strip(t);
                    if !empty.contains(&strip) {
                        continue;
                    }
                    let nxt = step + RELAY_COST;
                    let txy = board.xy(t);
                    if nxt + ahead(txy) >= best_cost || seen.get(&strip).copied().unwrap_or(f64::INFINITY) <= nxt {
                        continue;
                    }
                    seen.insert(strip, nxt);
                    let nd = Node::Strip(strip);
                    let here = Cand { spec: Spec::Hole(t), o: Opt::at(Some(t), txy), node: nd };
                    let out: Vec<Cand> = board.strip_holes(strip).into_iter()
                        .filter(|&h| h != t && !c.occupied.contains_key(&h) && !c.lain.contains(&pyf::key(board.xy(h))))
                        .map(|h| Cand { spec: Spec::Hole(h), o: Opt::at(Some(h), board.xy(h)), node: nd }).collect();
                    if !out.is_empty() {
                        let mut path = st.path.clone();
                        path.push((*end, here));
                        heap.push(State { f: nxt + ahead(txy), tie, cost: nxt, hops: st.hops + 1, ends: out, path });
                        tie += 1;
                    }
                }
            }
        }
    }
    best
}

struct State {
    f: f64,
    tie: u64,
    cost: f64,
    hops: usize,
    ends: Vec<Cand>,
    path: Vec<(Cand, Cand)>,
}

impl PartialEq for State {
    fn eq(&self, o: &Self) -> bool {
        self.f == o.f && self.tie == o.tie
    }
}
impl Eq for State {}
impl PartialOrd for State {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for State {
    // a min-heap on (f, tie), as heapq
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.f.partial_cmp(&self.f).unwrap().then(o.tie.cmp(&self.tie))
    }
}

#[derive(Default)]
struct ReachCache {
    holes: HashMap<Hole, std::rc::Rc<Vec<(Hole, f64)>>>,
    spare: HashMap<Strip, usize>,
}

/// (hole, cost) for each free hole a straight wire from this one could end in, lying clear: along its row,
/// and up and down its column, each line walked out a step at a time to the first thing in the way.
fn reach(c: &mut Circuit, hole: Hole, cache: &mut ReachCache) -> std::rc::Rc<Vec<(Hole, f64)>> {
    if let Some(r) = cache.holes.get(&hole) {
        return r.clone();
    }
    let bi = hole.board as usize;
    let a = c.boards[bi].xy(hole);
    let columns = c.boards[bi].columns;
    // levels: (y, row name) sorted
    let mut levels: Vec<(f64, Row)> = ROWS.iter().map(|&r| (c.boards[bi].y(r), r)).collect();
    levels.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap().then(x.1.name().cmp(y.1.name())));
    let lines: [Vec<Hole>; 4] = [
        (1..hole.col).rev().map(|col| Hole::new(hole.board, hole.row, col)).collect(),
        (hole.col + 1..=columns).map(|col| Hole::new(hole.board, hole.row, col)).collect(),
        levels.iter().rev().filter(|(y, _)| *y < a.1).map(|(_, r)| Hole::new(hole.board, *r, hole.col)).collect(),
        levels.iter().filter(|(y, _)| *y > a.1).map(|(_, r)| Hole::new(hole.board, *r, hole.col)).collect(),
    ];
    let mut out = Vec::new();
    for line in lines.iter() {
        let mut prev = a;
        let mut first = true;
        let mut over = false;
        let mut covered: HashMap<Strip, usize> = HashMap::default();
        for &t in line {
            if !c.boards[bi].has(t.row, t.col) {
                continue;
            }
            let b = c.boards[bi].xy(t);
            let step = Circuit::step_key(prev, b);
            let blocked = match c.step_blocked.get(&step) {
                Some(&v) => v,
                None => {
                    let v = c.crosses_any_body(prev, b);
                    c.step_blocked.insert(step, v);
                    v
                }
            };
            if blocked || c.along_any(prev, b, "") {
                break;
            }
            if !first {
                let k = pyf::key(prev);
                if c.ends.contains(&k) {
                    break;
                }
                over = over || c.used.contains_key(&k);
                if let Some(&strip) = c.strip_at.get(&k) {
                    if c.pin_strips.contains(&strip) && !c.lain.contains(&k) && !c.used.contains_key(&k) {
                        let n = covered.entry(strip).or_insert(0);
                        *n += 1;
                        let n = *n;
                        let s = *cache.spare.entry(strip).or_insert_with(|| spare(c, strip));
                        if s <= n {
                            break;
                        }
                    }
                }
            }
            prev = b;
            first = false;
            let kb = pyf::key(b);
            if c.occupied.contains_key(&t) || c.lain.contains(&kb) || c.covered.contains_key(&kb) {
                continue;
            }
            out.push((t, pyf::dist(a, b) + SHAPE_COST[if over { 1 } else { 0 }]));
        }
    }
    let r = std::rc::Rc::new(out);
    cache.holes.insert(hole, r.clone());
    r
}

/// Strips of the terminal area with nothing in them and nothing lying over any of their holes.
fn empty_strips(c: &mut Circuit) -> HashSet<Strip> {
    if let Some((w, s)) = &c.empty {
        if *w == c.wired {
            return s.clone();
        }
    }
    let mut out = HashSet::default();
    for b in &c.boards {
        for col in 1..=b.columns {
            for kind in [StripKind::AE, StripKind::FJ] {
                let strip = Strip { board: b.index, kind, n: col };
                if b.strip_holes(strip).iter().all(|&h| {
                    let k = pyf::key(b.xy(h));
                    !c.occupied.contains_key(&h) && !c.lain.contains(&k) && !c.covered.contains_key(&k)
                }) {
                    out.insert(strip);
                }
            }
        }
    }
    c.empty = Some((c.wired, out.clone()));
    out
}

/// Put in a route's wires one by one, each checked against the ones before; if one no longer lies clear, take
/// them all out again (false).
fn lay_route(c: &mut Circuit, dsu: &mut Dsu<Node>, net: &Net, route: &[(Cand, Cand)], nodes: &mut Vec<Node>) -> bool {
    let mut laid: Vec<u64> = Vec::new();
    for (ea, eb) in route {
        let (pa, pb) = (ea.o, eb.o);
        if c.occupied.contains_key(&pa.hole.unwrap()) || c.occupied.contains_key(&pb.hole.unwrap()) || !lies_clear(c, pa.xy, pb.xy)
            || c.lain.contains(&pyf::key(pa.xy)) || c.lain.contains(&pyf::key(pb.xy)) {
            for &w in &laid {
                unwire(c, w);
            }
            return false;
        }
        laid.push(lay(c, None, net, ea, eb, false, false, true));
    }
    for (ea, eb) in route {
        dsu.union(ea.node, eb.node);
        for n in [ea.node, eb.node] {
            if !nodes.contains(&n) {
                nodes.push(n);
                if let Some(s) = n.strip() {
                    c.pin_strips.insert(s);
                }
            }
        }
    }
    if dump::active() {
        let nums: Vec<i64> = laid.iter().map(|&id| c.wires[wire_index(c, id).unwrap()].num).collect();
        dump::event("relay", vec![("net", json!(net.name)), ("nums", json!(nums))]);
    }
    true
}

/// The options in a rail's end holes (its first or last five), the outermost first and the right end first.
fn rail_ends(c: &Circuit, options: &[Cand]) -> Vec<Cand> {
    let all: Vec<u32> = (0..options.len() as u32).collect();
    rail_end_indices(c, options, &all).into_iter().map(|k| options[k as usize]).collect()
}

/// rail_ends over some of the options, by index.
fn rail_end_indices(c: &Circuit, options: &[Cand], which: &[u32]) -> Vec<u32> {
    let mut out: Vec<(i32, i32, u32)> = Vec::new();
    for &k in which {
        let o = &options[k as usize];
        if let Some(h) = o.o.hole {
            if h.is_rail() {
                let b = &c.boards[h.board as usize];
                let mut cols: Vec<i32> = b.strip_holes(b.strip(h)).iter().map(|x| x.col).collect();
                cols.sort();
                let n = cols.len();
                let head = &cols[..END_GROUP.min(n)];
                let tail = &cols[n.saturating_sub(END_GROUP)..];
                if head.contains(&h.col) || tail.contains(&h.col) {
                    out.push(((h.col - cols[0]).min(cols[n - 1] - h.col), -h.col, k));
                }
            }
        }
    }
    out.sort_by_key(|t| (t.0, t.1));
    out.into_iter().map(|t| t.2).collect()
}

/// Take out a net's wires whose only job is to reach a rail no pin uses.
fn prune(c: &mut Circuit, net: &str, rails: &[Node], node_pins: &NodePins) {
    let mut changed = true;
    while changed {
        changed = false;
        for s in rails {
            if node_pins.get(s).map(|v| !v.is_empty()).unwrap_or(false) {
                continue;
            }
            let strip = s.strip().unwrap();
            let holes: HashSet<Hole> = c.boards[strip.board as usize].strip_holes(strip).into_iter().collect();
            let here: Vec<usize> = (0..c.wires.len()).filter(|&i| {
                [&c.wires[i].a, &c.wires[i].b].into_iter().flatten().any(|e| e.hole.map(|h| holes.contains(&h)).unwrap_or(false))
            }).collect();
            if here.iter().any(|&i| !c.wires[i].auto) {
                continue;
            }
            if here.len() == 1 {
                let id = c.wires[here[0]].id;
                unwire(c, id);
                changed = true;
            }
        }
        let mine: Vec<u64> = c.wires.iter().filter(|w| w.auto && w.group == net).map(|w| w.id).collect();
        for wid in mine {
            let wi = match wire_index(c, wid) {
                Some(i) => i,
                None => continue,
            };
            let ends: Vec<End> = [&c.wires[wi].a, &c.wires[wi].b].into_iter().flatten().cloned().collect();
            for e in ends {
                let h = match e.hole {
                    Some(h) if !h.is_rail() => h,
                    _ => continue,
                };
                let b = &c.boards[h.board as usize];
                let strip = b.strip(h);
                let holes: HashSet<Hole> = b.strip_holes(strip).into_iter().collect();
                let others = c.wires.iter().any(|x| x.id != wid && [&x.a, &x.b].into_iter().flatten()
                    .any(|e| e.hole.map(|h| holes.contains(&h)).unwrap_or(false)));
                let has_pins = node_pins.get(&Node::Strip(strip)).map(|v| !v.is_empty()).unwrap_or(false);
                if !has_pins && !others {
                    unwire(c, wid);
                    changed = true;
                    break;
                }
            }
        }
    }
}

fn unwire(c: &mut Circuit, wid: u64) {
    let wi = wire_index(c, wid).unwrap();
    if dump::active() {
        dump::event("unwire", vec![("num", json!(c.wires[wi].num))]);
    }
    let w = c.wires.remove(wi);
    c.wired += 1;
    let who = format!("W{}", w.num);
    for e in [&w.a, &w.b].into_iter().flatten() {
        if let Some(h) = e.hole {
            if c.occupied.get(&h) == Some(&who) {
                c.occupied.remove(&h);
                c.used.remove(&pyf::key(c.xy(h)));
            }
        }
    }
    c.segments.retain(|s| s.2 != who);
    c.reindex();
    let ends: HashSet<K> = c.wires.iter().flat_map(|x| [&x.a, &x.b]).flatten().filter(|e| e.hole.is_some()).map(|e| pyf::key(c.end_xy(e))).collect();
    c.ends = ends;
    let mut lain = HashSet::default();
    for x in &c.wires {
        lain.extend(c.lies_over(x));
    }
    c.lain = lain;
}

/// (spec, option, node) for each place a wire could join this node.
fn options(c: &Circuit, n: Node, net: &Net, node_pins: &NodePins) -> Vec<Cand> {
    if let Node::Off(..) = n {
        let p = node_pins[&n][0];
        let mut o = Opt::at(None, c.pin(p).xy.unwrap());
        o.pin = Some(p);
        return vec![Cand { spec: Spec::Pin(p), o, node: n }];
    }
    let strip = n.strip().unwrap();
    let board = &c.boards[strip.board as usize];
    let mine: Vec<PinId> = node_pins.get(&n).map(|v| v.iter().copied().filter(|p| net.pins.contains(p)).collect()).unwrap_or_default();
    let is_rail = strip.kind.rail().is_some();
    let mut out = Vec::new();
    for hole in board.strip_holes(strip) {
        let spec = if let Some(&m) = mine.first() { Spec::Pin(m) } else if is_rail { Spec::Kind(strip.kind) } else { Spec::Hole(hole) };
        if c.occupied.contains_key(&hole) {
            continue;
        }
        let xy = board.xy(hole);
        let k = pyf::key(xy);
        if c.lain.contains(&k) {
            continue;
        }
        let mut near = 0.0;
        let mut first = true;
        for &p in &mine {
            if let Some(pxy) = c.pin(p).xy {
                let d = pyf::dist(xy, pxy);
                if first || d < near {
                    near = d;
                    first = false;
                }
            }
        }
        let mut o = Opt::at(Some(hole), xy);
        o.near = near;
        o.covered = c.covered.contains_key(&k);
        o.kept = c.lead_room.contains(&hole);
        o.under = c.under.contains_key(&k);
        out.push(Cand { spec, o, node: n });
    }
    out
}

/// A floor under cost's first term: the straight length and the penalties that don't depend on the route.
fn bound(p: &Opt, q: &Opt) -> f64 {
    let mut length = pyf::dist(p.xy, q.xy);
    if p.pin.is_some() || q.pin.is_some() {
        let board_end = if p.pin.is_some() { q } else { p };
        length += if board_end.hole.unwrap().is_rail() { 0.0 } else { OFF_RAIL_COST };
    } else {
        length += KEPT_COST * (p.kept as i32 + q.kept as i32) as f64;
    }
    length + COVERED_COST * (p.covered as i32 + q.covered as i32) as f64
}

fn cost(c: &Circuit, p: &Opt, q: &Opt, unavoidable: bool) -> Key {
    let mut length = pyf::dist(p.xy, q.xy);
    if p.pin.is_none() && q.pin.is_none() && c.flex {
        if !lies_clear(c, p.xy, q.xy) {
            return Key(f64::INFINITY, 0.0, p.xy, q.xy);
        }
        length += SHAPE_COST[c.shape_level(p.xy, q.xy) as usize];
    } else if p.pin.is_none() && q.pin.is_none() {
        let detour = if c.crosses_any_body(p.xy, q.xy) { c.detour_cost(p.hole.unwrap(), q.hole.unwrap(), "") } else { None };
        if let Some(((trouble, l), _)) = detour {
            length = l;
            length += BEND_COST + 2.0 * trouble as f64;
        } else {
            let mut level = c.shape_level(p.xy, q.xy);
            if level == 2 && unavoidable {
                level = 1;
            }
            length += SHAPE_COST[level as usize] + CROSS_COST * c.bodies_crossed(p.xy, q.xy).len() as f64;
        }
        if p.hole.is_some() && q.hole.is_some() {
            length += CROSS_WIRE_COST * wires_crossed(c, p.xy, q.xy) as f64;
        }
    } else {
        let (board_end, pin_end) = if p.pin.is_some() { (q, p) } else { (p, q) };
        length += if board_end.hole.unwrap().is_rail() { 0.0 } else { OFF_RAIL_COST };
        length += RAIL_END_COST * from_end(c, board_end.hole.unwrap(), pin_end.xy) as f64;
        let over: Vec<usize> = c.bodies_crossed(p.xy, q.xy).into_iter().filter(|&i| !c.keepouts.contains(c.body_name(i))).collect();
        length += LEAD_OVER_COST * (over.len() + wires_crossed(c, p.xy, q.xy)) as f64;
        length += LEAD_TALL_COST * over.iter().filter(|&&i| c.tall.contains(c.body_name(i))).count() as f64;
    }
    if p.pin.is_none() && q.pin.is_none() {
        length += KEPT_COST * (p.kept as i32 + q.kept as i32) as f64;
    }
    length += COVERED_COST * (p.covered as i32 + q.covered as i32) as f64;
    Key(pyf::round(length, 6), pyf::round(p.near + q.near, 6), p.xy, q.xy)
}

/// For a rail hole: how many of its rail's holes lie between it and the rail's end nearer a point.
fn from_end(c: &Circuit, hole: Hole, toward: Xy) -> usize {
    if !hole.is_rail() {
        return 0;
    }
    let b = &c.boards[hole.board as usize];
    let mut cols: Vec<i32> = b.strip_holes(b.strip(hole)).iter().map(|h| h.col).collect();
    cols.sort();
    let left = b.xy(Hole::new(b.index, hole.row, cols[0])).0;
    let right = b.xy(Hole::new(b.index, hole.row, cols[cols.len() - 1])).0;
    let i = cols.iter().position(|&x| x == hole.col).unwrap();
    if (toward.0 - left).abs() < (toward.0 - right).abs() { i } else { cols.len() - 1 - i }
}

/// A wire lying on the board from a to b would be straight, clear of every part, along no other wire, and over
/// no wire's end; never from one board to another.
fn lies_clear(c: &Circuit, a: Xy, b: Xy) -> bool {
    if !aligned(a, b) || c.crosses_any_body(a, b) || c.along_any(a, b, "") {
        return false;
    }
    if let (Some(sa), Some(sb)) = (c.strip_at.get(&pyf::key(a)), c.strip_at.get(&pyf::key(b))) {
        if sa.board != sb.board {
            return false;
        }
    }
    let between = c.between(a, b);
    if between.iter().any(|k| c.ends.contains(k)) {
        return false;
    }
    let mut covered: Vec<(Strip, usize)> = Vec::new();
    for k in &between {
        if let Some(&s) = c.strip_at.get(k) {
            if c.pin_strips.contains(&s) && !c.lain.contains(k) && !c.used.contains_key(k) {
                match covered.iter_mut().find(|(x, _)| *x == s) {
                    Some(slot) => slot.1 += 1,
                    None => covered.push((s, 1)),
                }
            }
        }
    }
    covered.iter().all(|&(s, n)| spare(c, s) > n)
}

/// Free holes of a strip that no wire lies over.
fn spare(c: &Circuit, s: Strip) -> usize {
    let b = &c.boards[s.board as usize];
    b.strip_holes(s).iter().filter(|&&h| !c.occupied.contains_key(&h) && !c.lain.contains(&pyf::key(b.xy(h)))).count()
}

fn flex_over(c: &Circuit, a: Xy, b: Xy) -> f64 {
    let mut over: Vec<&str> = c.bodies_crossed(a, b).into_iter().map(|i| c.body_name(i)).filter(|r| !c.keepouts.contains(*r)).collect();
    over.extend(c.raised_crossed(a, b));
    FLEX_OVER_COST * (over.len() + over.iter().filter(|r| c.tall.contains(**r)).count()) as f64
}

fn over_keepout(c: &Circuit, a: Xy, b: Xy) -> bool {
    !c.keepouts.is_empty() && c.bodies_crossed(a, b).into_iter().any(|i| c.keepouts.contains(c.body_name(i)))
}

fn wires_crossed(c: &Circuit, a: Xy, b: Xy) -> usize {
    let mut n = 0;
    for w in &c.wires {
        if w.a.is_some() && w.b.is_some() {
            let pts = c.wire_points(w);
            for s in pts.windows(2) {
                n += cross((a, b), (s[0], s[1])) as usize;
            }
        }
    }
    n
}

fn lead_estimate(c: &Circuit, p: PinId, strips: &[Node]) -> f64 {
    let pxy = c.pin(p).xy.unwrap();
    let mut best = f64::INFINITY;
    for s in strips {
        let strip = s.strip().unwrap();
        let b = &c.boards[strip.board as usize];
        for h in b.strip_holes(strip) {
            if c.occupied.contains_key(&h) {
                continue;
            }
            let xy = b.xy(h);
            let over: Vec<usize> = c.bodies_crossed(pxy, xy).into_iter().filter(|&i| !c.keepouts.contains(c.body_name(i))).collect();
            let k = pyf::dist(pxy, xy) + LEAD_OVER_COST * over.len() as f64
                + LEAD_TALL_COST * over.iter().filter(|&&i| c.tall.contains(c.body_name(i))).count() as f64;
            best = pymin(best, k);
        }
    }
    best
}

fn link_cost(c: &Circuit, net: &Net, node_pins: &NodePins, a: &[Node], b: &[Node]) -> f64 {
    let pa = rail_ends(c, &a.iter().flat_map(|&n| options(c, n, net, node_pins)).collect::<Vec<_>>());
    let pb = rail_ends(c, &b.iter().flat_map(|&n| options(c, n, net, node_pins)).collect::<Vec<_>>());
    if pa.iter().any(|p| pb.iter().any(|q| p.o.hole.unwrap().col == q.o.hole.unwrap().col && lies_clear(c, p.o.xy, q.o.xy))) {
        return RAIL_LINK_COST;
    }
    cheapest(c, net, node_pins, a, b)
}

/// Roughly what the cheapest wire between two sets of nodes would cost now (inf if there's none).
fn cheapest(c: &Circuit, net: &Net, node_pins: &NodePins, a: &[Node], b: &[Node]) -> f64 {
    let pa: Vec<Opt> = a.iter().flat_map(|&n| options(c, n, net, node_pins)).map(|x| x.o).filter(|o| o.hole.is_some()).collect();
    let pb: Vec<Opt> = b.iter().flat_map(|&n| options(c, n, net, node_pins)).map(|x| x.o).filter(|o| o.hole.is_some()).collect();
    let mut pairs: Vec<(f64, u32, u32)> = Vec::with_capacity(pa.len() * pb.len());
    for (i, p) in pa.iter().enumerate() {
        for (j, q) in pb.iter().enumerate() {
            pairs.push((bound(p, q), i as u32, j as u32));
        }
    }
    let mut pairs = LazySorted::new(pairs, |t| t.0);
    let mut best = f64::INFINITY;
    let mut n = 0;
    while let Some((bd, i, j)) = pairs.get(n) {
        n += 1;
        let (p, q) = (pa[i as usize], pb[j as usize]);
        if bd > best {
            break;
        }
        if c.flex && !over_keepout(c, p.xy, q.xy) {
            best = pymin(best, bd + FLEX_COST + flex_over(c, p.xy, q.xy));
        }
        if c.flex {
            if lies_clear(c, p.xy, q.xy) {
                best = pymin(best, bd + SHAPE_COST[c.shape_level(p.xy, q.xy) as usize]);
            }
            continue;
        }
        let rough = bd + SHAPE_COST[c.shape_level(p.xy, q.xy) as usize] + CROSS_COST * c.bodies_crossed(p.xy, q.xy).len() as f64;
        best = pymin(best, rough);
    }
    best
}

/// One hole per strip the options are in.
fn strip_holes_of<'a>(c: &Circuit, options: impl Iterator<Item = &'a Cand>) -> Vec<Hole> {
    let mut out: Vec<(Strip, Hole)> = Vec::new();
    for o in options {
        if let Some(h) = o.o.hole {
            let s = c.boards[h.board as usize].strip(h);
            if !out.iter().any(|(x, _)| *x == s) {
                out.push((s, h));
            }
        }
    }
    out.into_iter().map(|(_, h)| h).collect()
}

/// Some hole of a's strip lines up with some hole of b's, so a wire between them needn't slant.
fn could_be_straight(c: &Circuit, a: Hole, b: Hole) -> bool {
    if a.board != b.board {
        return false;
    }
    let board = &c.boards[a.board as usize];
    let (sa, sb) = (board.strip(a), board.strip(b));
    let pa: Vec<Xy> = board.strip_holes(sa).iter().map(|&h| board.xy(h)).collect();
    let pb: Vec<Xy> = board.strip_holes(sb).iter().map(|&h| board.xy(h)).collect();
    pa.iter().any(|&p| pb.iter().any(|&q| p != q && aligned(p, q)))
}

#[allow(dead_code)]
fn _unused(_: [Row; 4]) -> [Row; 4] {
    RAILS
}
