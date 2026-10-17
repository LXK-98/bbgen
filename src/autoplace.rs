//! Which nets run on which rails, the rail ends kept for a supply's flying leads, and a built layout's
//! score: what placement and a build both use. The search itself is in search.rs.

use crate::board::{parse_hole, Hole, Row, Xy, RAILS};
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

pub fn net_of(c: &Circuit) -> HashMap<PinId, String> {
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

pub fn plan_for(c: &Circuit, net_of: &HashMap<PinId, String>) -> IndexMap<String, Vec<String>> {
    let polarity: IndexMap<String, Option<char>> = c.nets.iter().map(|(n, net)| (n.clone(), net.polarity)).collect();
    let explicit: IndexMap<String, Vec<String>> = c.nets.iter()
        .map(|(n, net)| (n.clone(), net.rails.iter().map(|r| r.rsplit(':').next().unwrap().to_string()).collect())).collect();
    rail_plan(&polarity, &explicit, &leg_rails(c, net_of), &c.rail_order)
}

/// Where each flying lead of a rail's net lands, decided before anything else is placed: on a rail of its net
/// (never in a strip), every rail lead of one connector on the same side of the board (top or bottom: the side
/// where the most of them have a rail, then the shortest leads), in the free holes nearest the connectors (the
/// rails' ends), shared out so that the leads cross as little as they can, then are shortest. A net may take a
/// rail of its sign that no other net has (wiring gives it the rail then). A lead whose net has no rail on its
/// connector's side goes to its nearest rail.
pub fn lead_plan(c: &Circuit) -> IndexMap<PinId, Hole> {
    let net_of = net_of(c);
    let plan = plan_for(c, &net_of);
    let mut claimed: HashMap<Row, &String> = HashMap::default();
    for (n, rails) in &plan {
        for r in rails.iter().filter_map(|r| Row::parse(r)) {
            claimed.insert(r, n);
        }
    }
    let rows_of = |n: &String| -> Vec<Row> {
        let mut rows: Vec<Row> = plan.get(n).map(|r| r.iter().filter_map(|r| Row::parse(r)).collect()).unwrap_or_default();
        if let Some(sign) = c.nets.get(n).and_then(|net| net.polarity) {
            for r in RAILS {
                if r.sign() == sign && !claimed.contains_key(&r) && !rows.contains(&r) {
                    rows.push(r);
                }
            }
        }
        rows
    };
    // a footprint pin of a rail's net drops straight to its rail: a lead takes no hole in that drop's way
    let mut needed: HashSet<Hole> = HashSet::default();
    for (pi, part) in c.parts.iter().enumerate() {
        if !part.kind.is_footprint() {
            continue;
        }
        for (k, pin) in part.pins.iter().enumerate() {
            let (h, n) = match (pin.hole, net_of.get(&PinId { part: pi as u16, pin: k as u16 })) {
                (Some(h), Some(n)) if !h.row.is_rail() => (h, n),
                _ => continue,
            };
            let side = if h.row.upper() { "top" } else { "bot" };
            if rows_of(n).iter().any(|r| r.side() == side) {
                for r in RAILS.iter().filter(|r| r.side() == side) {
                    needed.insert(Hole::new(h.board, *r, h.col));
                }
            }
        }
    }
    let free: Vec<Hole> = c.boards.iter().flat_map(|b| b.holes())
        .filter(|h| h.row.is_rail() && !c.occupied.contains_key(h) && !needed.contains(h)).collect();
    let nearest = |rows: &[Row], xy: Xy| -> Option<f64> {
        free.iter().filter(|h| rows.contains(&h.row)).map(|&h| pyf::dist(c.xy(h), xy)).min_by(|a, b| a.partial_cmp(b).unwrap())
    };
    // (pin, its xy, the rail it goes to)
    let mut leads: Vec<(PinId, Xy, Row)> = Vec::new();
    for (pi, part) in c.parts.iter().enumerate() {
        if part.kind != Kind::Offboard {
            continue;
        }
        let mine: Vec<(PinId, Xy, Vec<Row>)> = part.pins.iter().enumerate().filter_map(|(k, pin)| {
            let p = PinId { part: pi as u16, pin: k as u16 };
            let rows = rows_of(net_of.get(&p)?);
            if rows.is_empty() { None } else { Some((p, pin.xy?, rows)) }
        }).collect();
        let on = |rows: &[Row], side: &str| -> Vec<Row> { rows.iter().copied().filter(|r| r.side() == side).collect() };
        let mut side = "top";
        let mut best = (i64::MAX, f64::INFINITY);
        for s in ["top", "bot"] {
            let (mut missing, mut length) = (0i64, 0.0);
            for (_, xy, rows) in &mine {
                match nearest(&on(rows, s), *xy) {
                    Some(d) => length += d,
                    None => missing += 1,
                }
            }
            if (missing, length) < best {
                best = (missing, length);
                side = s;
            }
        }
        for (p, xy, rows) in mine {
            let there = on(&rows, side);
            let rows = if there.is_empty() { rows } else { there };
            // of its rows (one on a side, for a net of one sign), the nearest
            let row = *rows.iter().min_by(|a, b| nearest(&[**a], xy).unwrap_or(f64::INFINITY)
                .partial_cmp(&nearest(&[**b], xy).unwrap_or(f64::INFINITY)).unwrap()).unwrap();
            leads.push((p, xy, row));
        }
    }
    // each rail's leads get as many of its free holes as there are leads, the nearest the leads
    let mut rows: Vec<Row> = Vec::new();
    for l in &leads {
        if !rows.contains(&l.2) {
            rows.push(l.2);
        }
    }
    let mut groups: Vec<(Vec<usize>, Vec<Hole>)> = Vec::new();
    for &r in &rows {
        let ry = c.boards[c.default_board as usize].rail_y(r) as f64;
        let mut who: Vec<usize> = (0..leads.len()).filter(|&i| leads[i].2 == r).collect();
        who.sort_by(|&a, &b| (leads[a].1 .1 - ry).abs().partial_cmp(&(leads[b].1 .1 - ry).abs()).unwrap());
        let (mx, my) = (who.iter().map(|&i| leads[i].1 .0).sum::<f64>() / who.len() as f64, who.iter().map(|&i| leads[i].1 .1).sum::<f64>() / who.len() as f64);
        let mut holes: Vec<Hole> = free.iter().copied().filter(|h| h.row == r).collect();
        holes.sort_by(|a, b| pyf::dist(c.xy(*a), (mx, my)).partial_cmp(&pyf::dist(c.xy(*b), (mx, my))).unwrap());
        holes.truncate(who.len());
        groups.push((who, holes));
    }
    // try every way of sharing each rail's holes out (few leads go to one rail), keep the one with the fewest
    // crossings, then the shortest leads; with too many ways, the nearest pin to its rail takes the nearest hole
    let ways: usize = groups.iter().map(|(w, _)| (1..=w.len()).product::<usize>()).product();
    let mut choice: Vec<Vec<usize>> = groups.iter().map(|(w, h)| (0..w.len().min(h.len())).collect()).collect();
    if ways <= 5040 {
        let perms: Vec<Vec<Vec<usize>>> = groups.iter().map(|(w, h)| permutations(w.len().min(h.len()))).collect();
        let mut best: Option<((usize, f64), Vec<Vec<usize>>)> = None;
        let mut idx = vec![0usize; groups.len()];
        loop {
            let pick: Vec<Vec<usize>> = (0..groups.len()).map(|g| perms[g][idx[g]].clone()).collect();
            let mut lines: Vec<(Xy, Xy)> = Vec::new();
            for (g, (who, holes)) in groups.iter().enumerate() {
                for (k, &i) in who.iter().enumerate().take(holes.len()) {
                    lines.push((leads[i].1, c.xy(holes[pick[g][k]])));
                }
            }
            let mut crossings = 0;
            for i in 0..lines.len() {
                for j in i + 1..lines.len() {
                    crossings += crate::measure::cross(lines[i], lines[j]) as usize;
                }
            }
            let length: f64 = lines.iter().map(|l| pyf::dist(l.0, l.1)).sum();
            if best.as_ref().map(|b| crossings < b.0 .0 || (crossings == b.0 .0 && length < b.0 .1 - 1e-9)).unwrap_or(true) {
                best = Some(((crossings, length), pick));
            }
            let mut g = 0;
            while g < groups.len() {
                idx[g] += 1;
                if idx[g] < perms[g].len() {
                    break;
                }
                idx[g] = 0;
                g += 1;
            }
            if g == groups.len() {
                break;
            }
        }
        choice = best.unwrap().1;
    }
    let mut out: IndexMap<PinId, Hole> = IndexMap::default();
    for (g, (who, holes)) in groups.iter().enumerate() {
        for (k, &i) in who.iter().enumerate().take(holes.len()) {
            out.insert(leads[i].0, holes[choice[g][k]]);
        }
    }
    out
}

/// Every ordering of 0..n.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![vec![]];
    }
    let mut out = Vec::new();
    for p in permutations(n - 1) {
        for at in 0..=p.len() {
            let mut q = p.clone();
            q.insert(at, n - 1);
            out.push(q);
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
