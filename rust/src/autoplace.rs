//! The parts of bbgen/autoplace.py a build of a placed file still uses: which nets run on which rails, the rail
//! ends kept for a supply's flying leads, and a built layout's score.

use crate::board::{parse_hole, Hole, Row};
use crate::check;
use crate::circuit::{aligned, Circuit, PinId};
use crate::costs::*;
use crate::parts::Kind;
use crate::pyf;
use crate::IndexMap;
use crate::{HashMap, HashSet};

/// {net: [rail, ...]}, as autowire assigns them.
pub fn rail_plan(polarity: &IndexMap<String, Option<char>>, explicit: &IndexMap<String, Vec<String>>,
                 leg_rails: &IndexMap<String, Vec<String>>, order: &[String]) -> IndexMap<String, Vec<String>> {
    let mut plan: IndexMap<String, Vec<String>> = IndexMap::default();
    for name in polarity.keys() {
        let mut rails: Vec<String> = Vec::new();
        for r in explicit.get(name).into_iter().flatten().chain(leg_rails.get(name).into_iter().flatten()) {
            if !rails.contains(r) {
                rails.push(r.clone());
            }
        }
        plan.insert(name.clone(), rails);
    }
    let mut taken: HashSet<String> = plan.values().flatten().cloned().collect();
    let claimed: HashSet<String> = plan.iter().filter(|(_, r)| !r.is_empty()).map(|(n, _)| n.clone()).collect();
    for sign in ['-', '+'] {
        let mut nets: Vec<String> = polarity.iter().filter(|(n, p)| **p == Some(sign) && !claimed.contains(*n)).map(|(n, _)| n.clone()).collect();
        let pos = |n: &String| order.iter().position(|o| o == n).unwrap_or(order.len());
        nets.sort_by_key(pos);
        let rows: Vec<String> = [format!("top{}", sign), format!("bot{}", sign)].into_iter().filter(|r| !taken.contains(r)).collect();
        let shares: Vec<Vec<String>> = if nets.len() == 1 { vec![rows.clone()] } else { rows.iter().map(|r| vec![r.clone()]).collect() };
        for (net, share) in nets.iter().zip(shares) {
            plan.get_mut(net).unwrap().extend(share.iter().cloned());
            taken.extend(share);
        }
    }
    plan.into_iter().filter(|(_, r)| !r.is_empty()).collect()
}

fn net_of(c: &Circuit) -> HashMap<PinId, String> {
    let mut out = HashMap::default();
    for net in c.nets.values() {
        for &p in &net.pins {
            out.insert(p, net.name.clone());
        }
    }
    out
}

fn leg_rails(c: &Circuit, net_of: &HashMap<PinId, String>) -> IndexMap<String, Vec<String>> {
    let mut out: IndexMap<String, Vec<String>> = IndexMap::default();
    for (pi, part) in c.parts.iter().enumerate() {
        for (k, leg) in part.legs.iter().enumerate().take(part.pins.len()) {
            if let Some((_, row, _)) = parse_hole(leg) {
                if let Some(net) = net_of.get(&PinId { part: pi as u16, pin: k as u16 }) {
                    if row.is_rail() {
                        out.entry(net.clone()).or_default().push(row.name().to_string());
                    }
                }
            }
        }
    }
    out
}

fn plan_of(c: &Circuit, net_of: &HashMap<PinId, String>) -> IndexMap<String, Vec<String>> {
    let polarity: IndexMap<String, Option<char>> = c.nets.iter().map(|(n, net)| (n.clone(), net.polarity)).collect();
    let explicit: IndexMap<String, Vec<String>> = c.nets.iter()
        .map(|(n, net)| (n.clone(), net.rails.iter().map(|r| r.rsplit(':').next().unwrap().to_string()).collect())).collect();
    rail_plan(&polarity, &explicit, &leg_rails(c, net_of), &c.rail_order)
}

/// The rail holes a supply's flying leads should land in: the two at the rail's end nearest the connector, on
/// the rail of its net nearest the pin.
pub fn lead_room(c: &Circuit) -> HashSet<Hole> {
    let net_of = net_of(c);
    let plan = plan_of(c, &net_of);
    let first = &c.boards[c.default_board as usize];
    let all: Vec<Hole> = c.boards.iter().flat_map(|b| b.holes()).collect();
    let mut out = HashSet::default();
    for (pi, part) in c.parts.iter().enumerate() {
        if part.kind != Kind::Offboard {
            continue;
        }
        for (k, pin) in part.pins.iter().enumerate() {
            let rails = match net_of.get(&PinId { part: pi as u16, pin: k as u16 }).and_then(|n| plan.get(n)) {
                Some(r) if !r.is_empty() => r,
                _ => continue,
            };
            let xy = match pin.xy {
                Some(xy) => xy,
                None => continue,
            };
            let dy = |r: &String| ((first.rail_y(Row::parse(r).unwrap()) as f64) - xy.1).abs();
            let mut row = &rails[0];
            for r in &rails[1..] {
                if dy(r) < dy(row) {
                    row = r;
                }
            }
            let row = Row::parse(row).unwrap();
            let mut holes: Vec<Hole> = all.iter().copied().filter(|h| h.row == row).collect();
            holes.sort_by(|a, b| (c.xy(*a).0 - xy.0).abs().partial_cmp(&(c.xy(*b).0 - xy.0).abs()).unwrap());
            out.extend(holes.into_iter().take(2));
        }
    }
    out
}

const ROUND_GAP: f64 = 1.9;

fn span_max(kind: Kind) -> f64 {
    match kind {
        Kind::Resistor => 5.0,
        Kind::Diode => 5.0,
        Kind::Capacitor => 3.0,
        Kind::Electrolytic => 2.0,
        Kind::Led => 3.0,
        _ => 0.0,
    }
}

/// How good a built layout is: problems first, then anything slanted or overlong, then wire length.
pub fn score(c: &Circuit, result: &check::Result) -> f64 {
    let mut s = pyf::Sum::new();
    // score = 1000.0 * errors + 15.0 * warnings + 2.0 * wires, then += each term in turn
    let mut score = 1000.0 * result.errors() as f64 + 15.0 * result.warnings() as f64 + 2.0 * c.wires.len() as f64;
    let _ = &mut s;
    for w in &c.wires {
        let (a, b) = match (&w.a, &w.b) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        if w.flex {
            score += FLEX_WEIGHT + pyf::dist(c.end_xy(a), c.end_xy(b));
        } else if a.hole.is_some() && b.hole.is_some() && a.hole.unwrap().is_rail() && b.hole.unwrap().is_rail() && w.via.is_empty() {
            score += RAIL_LINK_WEIGHT;
        } else if a.hole.is_some() && b.hole.is_some() {
            let pts = c.wire_points(w);
            score += pyf::fsum(pts.windows(2).map(|p| pyf::dist(p[0], p[1])));
            score += 10.0 * pts.windows(2).filter(|p| !aligned(p[0], p[1])).count() as f64;
        } else {
            score += FLYING_WEIGHT * pyf::dist(c.end_xy(a), c.end_xy(b));
        }
    }
    score += KEEP_WEIGHT * c.cable_holes() as f64;
    score += CROSS_WEIGHT * crate::measure::crossings(c) as f64;
    let rounds: Vec<(f64, f64)> = c.parts.iter().filter(|p| matches!(p.kind, Kind::Led | Kind::Electrolytic) && p.pins.iter().all(|x| x.xy.is_some()))
        .map(|p| {
            let (a, b) = (p.pins[0].xy.unwrap(), p.pins[1].xy.unwrap());
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
        }).collect();
    let mut touching = 0;
    for i in 0..rounds.len() {
        for j in i + 1..rounds.len() {
            if pyf::dist(rounds[i], rounds[j]) < ROUND_GAP {
                touching += 1;
            }
        }
    }
    score += ROUND_COST * touching as f64;
    let mut middles: Vec<(f64, f64)> = Vec::new();
    for part in &c.parts {
        if part.kind.is_lead() && part.pins.iter().all(|p| p.xy.is_some()) {
            let (a, b) = (part.pins[0].xy.unwrap(), part.pins[1].xy.unwrap());
            score += LEG_SLANT_COST * (!aligned(a, b)) as i32 as f64
                + 3.0 * crate::circuit::pymax(0.0, pyf::dist(a, b) - span_max(part.kind));
            let over = c.bodies_crossed(a, b).into_iter().filter(|&i| c.body_name(i) != part.r#ref && !c.keepouts.contains(c.body_name(i))).count()
                + c.raised_crossed(a, b).len();
            score += LEG_OVER_COST * over as f64;
            middles.push(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
        }
    }
    for i in 0..middles.len() {
        let (x1, y1) = middles[i];
        let n = middles[i + 1..].iter().filter(|(x2, y2)| (x1 - x2).abs() < 2.5 && (y1 - y2).abs() < 1.5).count();
        score += LABEL_COST * n as f64;
    }
    score
}
