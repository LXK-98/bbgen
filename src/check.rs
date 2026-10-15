//! Wiring check: what the breadboard actually connects, compared with the nets.

use crate::circuit::{Circuit, End, Issue, Node, PinId};
use crate::IndexMap;
use std::hash::Hash;

/// Union-find as check.py's _DSU: a dict of parents (in insertion order, which wiring iterates), halving paths.
#[derive(Clone, Default)]
pub struct Dsu<T: Copy + Eq + Hash> {
    pub parent: IndexMap<T, T>,
}

impl<T: Copy + Eq + Hash> Dsu<T> {
    pub fn new() -> Self {
        Dsu { parent: IndexMap::default() }
    }

    pub fn find(&mut self, x: T) -> T {
        let mut x = x;
        self.parent.entry(x).or_insert(x);
        loop {
            let p = self.parent[&x];
            if p == x {
                return x;
            }
            let gp = self.parent[&p];
            self.parent.insert(x, gp);
            x = gp;
        }
    }

    pub fn union(&mut self, a: T, b: T) {
        let ra = self.find(a);
        let rb = self.find(b);
        self.parent.insert(ra, rb);
    }
}

pub fn end_node(c: &Circuit, end: &End) -> Node {
    match end.pin {
        Some(p) => Node::Off(p.part, c.pin(p).num),
        None => {
            let h = end.hole.unwrap();
            Node::Strip(c.boards[h.board as usize].strip(h))
        }
    }
}

pub struct Result {
    pub issues: Vec<Issue>,
    /// board strip -> pins sitting in it
    pub strip_pins: IndexMap<crate::board::Strip, Vec<PinId>>,
    /// connection root -> net names found on it (sorted)
    pub root_nets: IndexMap<Node, Vec<String>>,
    pub dsu: std::cell::RefCell<Dsu<Node>>,
}

impl Result {
    /// The nets on whatever a node is connected to.
    pub fn nets_at(&self, node: Node) -> Vec<String> {
        let root = self.dsu.borrow_mut().find(node);
        self.root_nets.get(&root).cloned().unwrap_or_default()
    }

    pub fn wire_nets(&self, c: &Circuit, w: &crate::circuit::Wire) -> Vec<String> {
        match &w.a {
            Some(a) => self.nets_at(end_node(c, a)),
            None => vec![],
        }
    }

    pub fn wire_color(&self, c: &Circuit, w: &crate::circuit::Wire) -> String {
        if let Some(col) = &w.color {
            return col.clone();
        }
        let nets = self.wire_nets(c, w);
        if nets.len() == 1 { c.nets[&nets[0]].color.clone() } else { "gray".into() }
    }

    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.level == "error").count()
    }

    pub fn warnings(&self) -> usize {
        self.issues.iter().filter(|i| i.level == "warning").count()
    }
}

fn short_list(c: &Circuit, pins: &[PinId], limit: usize) -> String {
    let names: Vec<String> = pins.iter().map(|&p| c.pin_str(p)).collect();
    let mut s = names.iter().take(limit).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > limit {
        s += &format!(" and {} more", names.len() - limit);
    }
    s
}

pub fn check(c: &Circuit) -> Result {
    let mut dsu: Dsu<Node> = Dsu::new();
    let mut issues = c.issues.clone();
    let mut pin_node: IndexMap<PinId, Node> = IndexMap::default();
    let mut strip_pins: IndexMap<crate::board::Strip, Vec<PinId>> = IndexMap::default();
    for (pi, part) in c.parts.iter().enumerate() {
        for (k, pin) in part.pins.iter().enumerate() {
            let id = PinId { part: pi as u16, pin: k as u16 };
            let node = if let Some(h) = pin.hole {
                let strip = c.boards[h.board as usize].strip(h);
                strip_pins.entry(strip).or_default().push(id);
                Node::Strip(strip)
            } else if part.kind == crate::parts::Kind::Offboard {
                Node::Off(pi as u16, pin.num)
            } else {
                continue;
            };
            pin_node.insert(id, node);
            dsu.find(node);
        }
        for bus in &part.buses {
            let nodes: Vec<Node> = bus.iter().filter_map(|&k| pin_node.get(&PinId { part: pi as u16, pin: k as u16 }).copied()).collect();
            for &n in nodes.iter().skip(1) {
                dsu.union(nodes[0], n);
            }
        }
    }
    let mut ends: IndexMap<Node, Vec<(usize, &End)>> = IndexMap::default();
    for (wi, w) in c.wires.iter().enumerate() {
        let (a, b) = match (&w.a, &w.b) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let (na, nb) = (end_node(c, a), end_node(c, b));
        dsu.union(na, nb);
        if na == nb {
            issues.push(Issue::new("warning", "NO_EFFECT", format!("W{} has both ends in the same strip", w.num)).holes(vec![a.hole]));
        }
        for (end, node) in [(a, na), (b, nb)] {
            if end.hole.is_some() {
                ends.entry(node).or_default().push((wi, end));
            }
        }
    }
    for (node, here) in &ends {
        let has_pins = node.strip().map(|s| strip_pins.get(&s).map(|v| !v.is_empty()).unwrap_or(false)).unwrap_or(false);
        if here.len() == 1 && !has_pins {
            let (wi, end) = here[0];
            issues.push(Issue::new("warning", "DANGLING", format!("W{} ends in {}, where nothing else is connected",
                                                                  c.wires[wi].num, c.hole_name(end.hole.unwrap()))).holes(vec![end.hole]));
        }
    }
    let mut net_of: IndexMap<PinId, &str> = IndexMap::default();
    for net in c.nets.values() {
        for &pin in &net.pins {
            if let Some(&other) = net_of.get(&pin) {
                if other != net.name {
                    issues.push(Issue::new("error", "TWO_NETS", format!("{} is in both {} and {}", c.pin_str(pin), other, net.name)).pins(vec![pin]));
                }
            }
            net_of.entry(pin).or_insert(&net.name);
        }
    }
    for net in c.nets.values() {
        let missing: Vec<PinId> = net.pins.iter().copied().filter(|p| !pin_node.contains_key(p)).collect();
        if !missing.is_empty() {
            issues.push(Issue::new("error", "NOT_PLACED", format!("net {}: {} not on the board", net.name, short_list(c, &missing, 6))).pins(missing.clone()));
        }
        let mut pieces: IndexMap<Node, Vec<PinId>> = IndexMap::default();
        for &pin in &net.pins {
            if let Some(&node) = pin_node.get(&pin) {
                pieces.entry(dsu.find(node)).or_default().push(pin);
            }
        }
        if pieces.len() > 1 {
            let text = pieces.values().map(|g| format!("{{{}}}", short_list(c, g, 4))).collect::<Vec<_>>().join(" | ");
            issues.push(Issue::new("error", "OPEN", format!("net {} is in {} unconnected pieces: {}", net.name, pieces.len(), text)).pins(pieces.values().flatten().copied().collect()));
        }
    }
    let mut groups: IndexMap<Node, Vec<PinId>> = IndexMap::default();
    let nodes: Vec<(PinId, Node)> = pin_node.iter().map(|(p, n)| (*p, *n)).collect();
    for (pin, node) in nodes {
        groups.entry(dsu.find(node)).or_default().push(pin);
    }
    let mut root_nets: IndexMap<Node, Vec<String>> = IndexMap::default();
    for (root, pins) in &groups {
        let mut nets: Vec<&str> = pins.iter().filter_map(|p| net_of.get(p).copied()).collect();
        nets.sort();
        nets.dedup();
        root_nets.insert(*root, nets.iter().map(|n| n.to_string()).collect());
        if nets.len() > 1 {
            issues.push(Issue::new("error", "SHORT", format!("nets {} are connected together ({})", nets.join(", "), short_list(c, pins, 6))).pins(pins.clone()));
        }
        // a pin, or the bus it's part of: one unit
        let unit = |p: PinId| -> (u16, Vec<u16>) {
            let part = &c.parts[p.part as usize];
            match part.bus_of(p.pin as usize) {
                Some(bus) => (p.part, bus.iter().map(|&k| k as u16).collect()),
                None => (p.part, vec![p.pin]),
            }
        };
        let mut units: Vec<(u16, Vec<u16>)> = Vec::new();
        for &p in pins {
            let u = unit(p);
            if !units.contains(&u) {
                units.push(u);
            }
        }
        if units.len() < 2 {
            continue;
        }
        let mut reported: Vec<(u16, Vec<u16>)> = Vec::new();
        for &pin in pins {
            let u = unit(pin);
            if net_of.contains_key(&pin) || reported.contains(&u) {
                continue;
            }
            reported.push(u);
            let what = if nets.len() == 1 {
                format!("net {}", nets[0])
            } else {
                let others: Vec<PinId> = pins.iter().copied().filter(|&q| q != pin).collect();
                short_list(c, &others, 4)
            };
            issues.push(Issue::new("error", "STRAY", format!("{} is connected to {} but isn't in any net", c.pin_str(pin), what)).pins(vec![pin]));
        }
    }
    for (pi, part) in c.parts.iter().enumerate() {
        let ids: Vec<PinId> = (0..part.pins.len()).map(|k| PinId { part: pi as u16, pin: k as u16 }).collect();
        if part.kind.is_lead() && ids.iter().all(|p| pin_node.contains_key(p)) {
            let a = dsu.find(pin_node[&ids[0]]);
            let b = dsu.find(pin_node[&ids[1]]);
            if a == b {
                issues.push(Issue::new("error", "SHORTED_PART", format!("both legs of {} are in the same connection", part.r#ref)).pins(ids.clone()));
            }
        }
    }
    // errors first, otherwise in order
    let (mut errors, warnings): (Vec<Issue>, Vec<Issue>) = issues.into_iter().partition(|i| i.level == "error");
    errors.extend(warnings);
    Result { issues: errors, strip_pins, root_nets, dsu: std::cell::RefCell::new(dsu) }
}
