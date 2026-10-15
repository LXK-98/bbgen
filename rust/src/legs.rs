//! Placing two-lead parts' legs (bbgen/autoplace.py's place_legs and its helpers): each leg goes in a strip
//! or on a rail of its net (or an empty strip, which a wire then joins to it), the pair chosen to lie straight,
//! at a natural span for the part, without covering the free holes its strips still need or standing in a
//! pin's way to its rail.

use crate::board::{Hole, Row, Strip, StripKind, Xy};
use crate::circuit::{aligned, line, pymax, pymin, Circuit, Issue, PinId, K};
use crate::costs::*;
use crate::parts::Kind;
use crate::pyf;
use crate::{HashMap, HashSet, IndexMap};

pub const ROUND_GAP: f64 = 1.9;

/// (shortest, longest) natural span in holes.
pub fn span(kind: Kind) -> (f64, f64) {
    match kind {
        Kind::Resistor => (2.0, 5.0),
        Kind::Diode => (3.0, 5.0),
        Kind::Capacitor => (1.0, 3.0),
        Kind::Electrolytic => (1.0, 2.0),
        Kind::Led => (1.0, 3.0),
        _ => (0.0, 0.0),
    }
}

pub fn is_round(kind: Kind) -> bool {
    matches!(kind, Kind::Led | Kind::Electrolytic)
}

/// Python's sort order for strips: (board id, kind's name, n).
pub fn strip_order(c: &Circuit, a: &Strip, b: &Strip) -> std::cmp::Ordering {
    (c.boards[a.board as usize].id.as_str(), a.kind.name(), a.n).cmp(&(c.boards[b.board as usize].id.as_str(), b.kind.name(), b.n))
}

fn sorted_strips(c: &Circuit, set: impl IntoIterator<Item = Strip>) -> Vec<Strip> {
    let mut v: Vec<Strip> = Vec::new();
    for s in set {
        if !v.contains(&s) {
            v.push(s);
        }
    }
    v.sort_by(|a, b| strip_order(c, a, b));
    v
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

pub fn all_holes(c: &Circuit) -> Vec<Hole> {
    c.boards.iter().flat_map(|b| b.holes()).collect()
}

fn drops(c: &Circuit, plan: &IndexMap<String, Vec<String>>, net_of: &HashMap<PinId, String>) -> Vec<(Hole, Hole)> {
    let mut out = Vec::new();
    for (pi, part) in c.parts.iter().enumerate() {
        if !part.kind.is_footprint() {
            continue;
        }
        for (k, pin) in part.pins.iter().enumerate() {
            let h = match pin.hole {
                Some(h) if !h.is_rail() => h,
                _ => continue,
            };
            let top = h.row.top_half();
            let side = if top { "top" } else { "bot" };
            let rails: Vec<&String> = net_of.get(&PinId { part: pi as u16, pin: k as u16 }).and_then(|n| plan.get(n))
                .map(|r| r.iter().filter(|r| r.starts_with(side)).collect()).unwrap_or_default();
            let own = &c.boards[h.board as usize];
            if let Some(r) = rails.first() {
                let row = Row::parse(r).unwrap();
                if own.has(row, h.col) {
                    out.push((Hole::new(own.index, if top { Row::A } else { Row::J }, h.col), Hole::new(own.index, row, h.col)));
                }
            }
        }
    }
    out
}

/// Pins' straight drops to their rails, indexed for quick lookups.
struct Drops {
    ends: HashMap<Hole, usize>,
    lines: HashMap<(u8, i64), Vec<(f64, f64)>>,
}

impl Drops {
    fn new(c: &Circuit, drops: &[(Hole, Hole)]) -> Drops {
        let mut ends: HashMap<Hole, usize> = HashMap::default();
        let mut lines: HashMap<(u8, i64), Vec<(f64, f64)>> = HashMap::default();
        for &(start, rail) in drops {
            for h in [start, rail] {
                *ends.entry(h).or_insert(0) += 1;
            }
            let (k, lo, hi) = line(c.xy(start), c.xy(rail)).unwrap();
            lines.entry(k).or_default().push((lo, hi));
        }
        Drops { ends, lines }
    }

    /// How many drops a part with legs in a and b would be in the way of.
    fn blocked(&self, a: Hole, b: Hole, pa: Xy, pb: Xy) -> usize {
        let mut n = self.ends.get(&a).copied().unwrap_or(0) + self.ends.get(&b).copied().unwrap_or(0);
        if let Some((k, lo, hi)) = line(pa, pb) {
            if let Some(v) = self.lines.get(&k) {
                n += v.iter().filter(|(l2, h2)| pymin(hi, *h2) - pymax(lo, *l2) > 1e-6).count();
            }
        }
        n
    }
}

fn strips_of(c: &Circuit, net: &str, extra: &HashMap<String, Vec<Strip>>) -> HashSet<Strip> {
    let mut out: HashSet<Strip> = c.nets[net].pins.iter().filter_map(|&p| c.pin(p).hole).filter(|h| !h.is_rail())
        .map(|h| c.boards[h.board as usize].strip(h)).collect();
    if let Some(e) = extra.get(net) {
        out.extend(e.iter().copied());
    }
    out
}

/// Free holes a leg on this net could go in: its strips and its rails (not the end kept for a supply's leads).
fn leg_options(c: &Circuit, all: &[Hole], net: &str, plan: &IndexMap<String, Vec<String>>, extra: &HashMap<String, Vec<Strip>>) -> Vec<Hole> {
    let mut set: Vec<Strip> = c.nets[net].pins.iter().filter_map(|&p| c.pin(p).hole).map(|h| c.boards[h.board as usize].strip(h)).collect();
    if let Some(e) = extra.get(net) {
        set.extend(e.iter().copied());
    }
    let strips = sorted_strips(c, set);
    let mut holes: Vec<Hole> = strips.iter().flat_map(|&s| c.boards[s.board as usize].strip_holes(s)).collect();
    if let Some(rails) = plan.get(net) {
        holes.extend(all.iter().copied().filter(|h| h.row.is_rail() && rails.iter().any(|r| r == h.row.name())));
    }
    holes.into_iter().filter(|h| !c.occupied.contains_key(h) && !c.lead_room.contains(h) && !c.under.contains_key(&pyf::key(c.xy(*h)))).collect()
}

/// Free holes in empty strips: within reach of the other leg's strips, or anywhere if the net has nothing on
/// the board yet.
fn empty_near(c: &Circuit, all: &[Hole], mine: &[Hole], others: &[Hole], reach: f64) -> Vec<Hole> {
    let reach = if mine.iter().any(|h| h.is_rail()) { pymin(reach, 3.0) } else { reach };
    let xs: Vec<f64> = {
        let mut v: Vec<f64> = Vec::new();
        for h in others.iter().filter(|h| !h.is_rail()) {
            let x = c.xy(*h).0;
            if !v.contains(&x) {
                v.push(x);
            }
        }
        v
    };
    if !mine.is_empty() && xs.is_empty() {
        return vec![];
    }
    let strips = sorted_strips(c, all.iter().filter(|h| !h.is_rail()).map(|&h| c.boards[h.board as usize].strip(h)));
    let mut out = Vec::new();
    for s in strips {
        let holes = c.boards[s.board as usize].strip_holes(s);
        let x = c.xy(holes[0]).0;
        if !xs.is_empty() && !xs.iter().any(|o| (x - o).abs() <= reach) {
            continue;
        }
        if !holes.iter().any(|h| c.occupied.contains_key(h)) {
            out.extend(holes.into_iter().filter(|h| !c.under.contains_key(&pyf::key(c.xy(*h)))));
        }
    }
    out
}

enum Home {
    None,
    X(f64),
    Pins(Vec<PinId>),
}

/// The x the part should sit near (its `near` part), or for a decoupling cap the supply pins with no cap yet.
fn home(c: &Circuit, pi: usize, nets: &[String], plan: &IndexMap<String, Vec<String>>, served: &[PinId]) -> Home {
    let part = &c.parts[pi];
    let decoupling = nets.iter().all(|n| plan.contains_key(n));
    if let Some(near) = &part.near {
        if let Some(&ni) = c.part_index.get(near) {
            if !decoupling {
                let xs: Vec<f64> = c.parts[ni as usize].pins.iter().filter_map(|p| p.xy).map(|xy| xy.0).collect();
                return if xs.is_empty() { Home::None } else { Home::X(pyf::fsum(xs.iter().copied()) / xs.len() as f64) };
            }
        }
    }
    if decoupling {
        let supply: Vec<&String> = nets.iter().filter(|n| c.nets[n.as_str()].polarity != Some('-')).collect();
        let first = supply.first().copied().unwrap_or(&nets[0]);
        let pins: Vec<PinId> = c.nets[first.as_str()].pins.iter().copied().filter(|&p| {
            let pp = &c.parts[p.part as usize];
            c.pin(p).hole.is_some() && pp.kind.is_footprint() && part.near.as_ref().map(|n| *n == pp.r#ref).unwrap_or(true)
        }).collect();
        let unserved: Vec<PinId> = pins.iter().copied().filter(|p| !served.contains(p)).collect();
        let pins = if unserved.is_empty() { pins } else { unserved };
        if !pins.is_empty() {
            return Home::Pins(pins);
        }
    }
    Home::None
}

/// Scores leg pairs for one two-lead part.
struct Legs<'a> {
    c: &'a Circuit,
    kind: Kind,
    drops: &'a Drops,
    own: Vec<HashSet<Strip>>,
    own_x: Vec<Vec<f64>>,
    lead_x: Vec<Option<f64>>,
    on_rail: Vec<bool>,
    main_rail: Vec<Option<Row>>,
    rails: Vec<Vec<String>>,
    home_x: Option<f64>,
    home_pins: Option<Vec<PinId>>,
    leg_cache: HashMap<(usize, Hole), f64>,
    free_cache: HashMap<Strip, Vec<(Hole, K)>>,
}

impl<'a> Legs<'a> {
    fn new(c: &'a Circuit, pi: usize, nets: &[String], own: Vec<HashSet<Strip>>, home: Home, drops: &'a Drops,
           plan: &IndexMap<String, Vec<String>>) -> Legs<'a> {
        let own_x = own.iter().map(|mine| mine.iter().map(|&s| c.xy(c.boards[s.board as usize].strip_holes(s)[0]).0).collect()).collect();
        let lead_x = nets.iter().map(|n| {
            c.nets[n.as_str()].pins.iter().find(|p| c.parts[p.part as usize].kind == Kind::Offboard).map(|&p| c.pin(p).xy.unwrap().0)
        }).collect();
        let (home_x, home_pins) = match home {
            Home::X(x) => (Some(x), None),
            Home::Pins(p) => (None, Some(p)),
            Home::None => (None, None),
        };
        Legs {
            c, kind: c.parts[pi].kind, drops, own, own_x, lead_x,
            on_rail: nets.iter().map(|n| plan.contains_key(n)).collect(),
            main_rail: nets.iter().map(|n| plan.get(n).and_then(|r| r.first()).and_then(|r| Row::parse(r))).collect(),
            rails: nets.iter().map(|n| plan.get(n).cloned().unwrap_or_default()).collect(),
            home_x, home_pins, leg_cache: HashMap::default(), free_cache: HashMap::default(),
        }
    }

    fn drop_kept(&self, hole: Hole, rails: &[String]) -> bool {
        let side = if hole.row.top_half() { "top" } else { "bot" };
        rails.iter().filter(|r| r.starts_with(side)).any(|r| self.c.lead_room.contains(&Hole::new(hole.board, Row::parse(r).unwrap(), hole.col)))
    }

    fn under_rail(&self, hole: Hole) -> bool {
        self.c.boards[hole.board as usize].rail_cols.contains(&hole.col)
    }

    /// What a pair's cost owes to one leg alone (the i-th) being in this hole.
    fn leg(&mut self, i: usize, leg: Hole, xy: Xy) -> f64 {
        if let Some(&v) = self.leg_cache.get(&(i, leg)) {
            return v;
        }
        let c = self.c;
        let crowd = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)].iter()
            .filter(|(dx, dy)| c.used.contains_key(&pyf::key((xy.0 + dx, xy.1 + dy)))).count();
        let mut cost = CROWD_LEG_COST * crowd as f64;
        if leg.is_rail() {
            if let Some(main) = self.main_rail[i] {
                if leg.row != main {
                    cost += SECOND_RAIL_COST;
                }
            }
        }
        let own = !leg.is_rail() && self.own[i].contains(&c.boards[leg.board as usize].strip(leg));
        if let Some(lx) = self.lead_x[i] {
            if !own {
                cost += (if leg.is_rail() { 0.05 } else { 0.3 }) * (xy.0 - lx).abs();
            }
        }
        if !(leg.is_rail() || own) {
            if !self.own_x[i].is_empty() {
                let m = self.own_x[i].iter().map(|x| (xy.0 - x).abs()).fold(f64::INFINITY, pymin);
                cost += JUNCTION_COST + 0.5 * m;
            } else {
                cost += JUNCTION_COST / 2.0;
            }
            if self.on_rail[i] && !self.under_rail(leg) {
                cost += GAP_COST;
            } else if self.on_rail[i] && self.drop_kept(leg, &self.rails[i].clone()) {
                cost += KEPT_DROP_COST;
            }
        }
        self.leg_cache.insert((i, leg), cost);
        cost
    }

    fn free(&mut self, s: Strip) -> &Vec<(Hole, K)> {
        let c = self.c;
        self.free_cache.entry(s).or_insert_with(|| {
            c.boards[s.board as usize].strip_holes(s).into_iter().map(|h| (h, pyf::key(c.xy(h))))
                .filter(|(h, k)| !c.occupied.contains_key(h) && !c.covered.contains_key(k)).collect()
        })
    }

    fn cheap(&mut self, a: Hole, b: Hole, pa: Xy, pb: Xy) -> f64 {
        let (lo, hi) = span(self.kind);
        let sp = pyf::dist(pa, pb);
        let mut cost = 3.0 * pymax(pymax(0.0, lo - sp), sp - hi) + LEG_SLANT_COST * (!aligned(pa, pb)) as i32 as f64;
        cost += 8.0 * self.drops.blocked(a, b, pa, pb) as f64;
        cost += self.leg(0, a, pa) + self.leg(1, b, pb);
        let x = (pa.0 + pb.0) / 2.0;
        if is_round(self.kind) {
            let mid = (x, (pa.1 + pb.1) / 2.0);
            cost += ROUND_COST * self.c.rounds.iter().filter(|&&o| pyf::dist(mid, o) < ROUND_GAP).count() as f64;
        }
        if let Some(hx) = self.home_x {
            cost += 0.5 * (x - hx).abs();
        } else if let Some(pins) = &self.home_pins {
            if !pins.is_empty() {
                let m = pins.iter().map(|&p| (x - self.c.pin(p).xy.unwrap().0).abs()).fold(f64::INFINITY, pymin);
                cost += 0.5 * m;
            }
        }
        cost
    }

    fn costly(&mut self, a: Hole, b: Hole, pa: Xy, pb: Xy) -> f64 {
        let c = self.c;
        let mut cost = [0.0, 3.0, 0.0, 20.0][c.shape_level(pa, pb) as usize];
        cost += 30.0 * (c.bodies_crossed(pa, pb).len() + c.raised_crossed(pa, pb).len()) as f64;
        let covered: Vec<K> = if aligned(pa, pb) { c.between(pa, pb) } else { vec![] };
        for leg in [a, b] {
            if !leg.is_rail() {
                let s = c.boards[leg.board as usize].strip(leg);
                let free = self.free(s);
                if !free.iter().any(|(h, k)| *h != a && *h != b && !covered.contains(k)) {
                    cost += 15.0;
                }
            }
        }
        cost + 2.0 * covered.iter().filter(|k| c.used.contains_key(k)).count() as f64
    }

    /// The best pair, or (for autoplace's repair search) the rank-th best.
    fn choose(&mut self, pairs: &[(Hole, Hole, Xy, Xy)], rank: usize) -> Option<(Hole, Hole)> {
        let mut scored: Vec<(f64, usize)> = pairs.iter().enumerate().map(|(i, &(a, b, pa, pb))| (self.cheap(a, b, pa, pb), i)).collect();
        scored.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let mut top: Vec<(f64, usize, Hole, Hole)> = Vec::new();
        for (cheap, i) in scored {
            if top.len() > rank && cheap > top[rank].0 + 1e-9 {
                break;
            }
            let (a, b, pa, pb) = pairs[i];
            top.push((cheap + self.costly(a, b, pa, pb), i, a, b));
            top.sort_by(|x, y| (x.0, x.1).partial_cmp(&(y.0, y.1)).unwrap());
            top.truncate(rank + 1);
        }
        if top.is_empty() {
            return None;
        }
        let t = top[rank.min(top.len() - 1)];
        Some((t.2, t.3))
    }
}

/// (a, b, a's position, b's position) for leg holes in different strips, at most reach apart (or any distance,
/// if no pair is that close).
fn pairs(c: &Circuit, options: &[Vec<Hole>], reach: f64) -> Vec<(Hole, Hole, Xy, Xy)> {
    let strip = |h: Hole| c.boards[h.board as usize].strip(h);
    let first: Vec<(Hole, Xy, Strip)> = options[0].iter().map(|&h| (h, c.xy(h), strip(h))).collect();
    let mut second: Vec<(Xy, usize, Hole, Strip)> = options[1].iter().enumerate().map(|(i, &h)| (c.xy(h), i, h, strip(h))).collect();
    second.sort_by(|x, y| (x.0 .0, x.0 .1, x.1).partial_cmp(&(y.0 .0, y.0 .1, y.1)).unwrap());
    let xs: Vec<f64> = second.iter().map(|t| t.0 .0).collect();
    let mut out = Vec::new();
    for &(a, pa, sa) in &first {
        let lo = xs.partition_point(|&x| x < pa.0 - reach);
        let hi = xs.partition_point(|&x| x <= pa.0 + reach);
        let mut window: Vec<&(Xy, usize, Hole, Strip)> = second[lo..hi].iter().collect();
        window.sort_by_key(|t| t.1);
        for &&(pb, _, b, sb) in &window {
            let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
            if sb != sa && dx * dx + dy * dy <= reach * reach {
                out.push((a, b, pa, pb));
            }
        }
    }
    if out.is_empty() {
        for &(a, pa, sa) in &first {
            for &b in &options[1] {
                if strip(b) != sa {
                    out.push((a, b, pa, c.xy(b)));
                }
            }
        }
    }
    out
}

/// Choose holes for each two-lead part that has no `pins`, then place it as if they were written.
pub fn place_legs(c: &mut Circuit, parts: &[usize]) {
    let net_of = net_of(c);
    let plan = crate::autoplace::plan_for(c, &net_of);
    let all = all_holes(c);
    let total_columns: i32 = c.boards.iter().map(|b| b.columns).sum();
    c.rounds = c.parts.iter().filter(|p| is_round(p.kind) && p.pins.iter().all(|x| x.xy.is_some()))
        .map(|p| {
            let (a, b) = (p.pins[0].xy.unwrap(), p.pins[1].xy.unwrap());
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
        }).collect();
    let mut extra: HashMap<String, Vec<Strip>> = HashMap::default();
    let drops = Drops::new(c, &drops(c, &plan, &net_of));
    let mut served: Vec<PinId> = Vec::new();
    for &pi in parts {
        let r = c.parts[pi].r#ref.clone();
        let kind = c.parts[pi].kind;
        let nets: Vec<Option<String>> = (0..c.parts[pi].pins.len()).map(|k| net_of.get(&PinId { part: pi as u16, pin: k as u16 }).cloned()).collect();
        if nets.iter().any(|n| n.is_none()) {
            let missing: Vec<String> = nets.iter().enumerate().filter(|(_, n)| n.is_none()).map(|(k, _)| c.parts[pi].pin_str(k)).collect();
            c.issues.push(Issue::new("error", "BAD_LEG", format!("{}: {} is in no net, so bbgen can't tell where it goes; give its pins",
                                                                 r, missing.join(", "))));
            continue;
        }
        let nets: Vec<String> = nets.into_iter().map(Option::unwrap).collect();
        let base: Vec<Vec<Hole>> = nets.iter().map(|n| leg_options(c, &all, n, &plan, &extra)).collect();
        let reach = span(kind).1 + 4.0;
        let mut options: Vec<Vec<Hole>> = vec![
            [base[0].clone(), empty_near(c, &all, &base[0], &base[1], reach)].concat(),
            [base[1].clone(), empty_near(c, &all, &base[1], &base[0], reach)].concat(),
        ];
        let snapshot = [options[1].clone(), options[0].clone()];
        for (i, other) in snapshot.iter().enumerate() {
            if options[i].is_empty() {
                options[i] = empty_near(c, &all, &[], other, total_columns as f64);
            }
        }
        if options.iter().any(|o| o.is_empty()) {
            c.issues.push(Issue::new("error", "BAD_LEG", format!("{}: no free hole for a leg; give its pins", r)));
            continue;
        }
        let own: Vec<HashSet<Strip>> = nets.iter().map(|n| strips_of(c, n, &extra)).collect();
        let home = home(c, pi, &nets, &plan, &served);
        let rank = c.leg_rank.get(&r).copied().unwrap_or(0) as usize;
        let ps = pairs(c, &options, reach);
        let (pick, home_pins) = {
            let mut legs = Legs::new(c, pi, &nets, own, home, &drops, &plan);
            (legs.choose(&ps, rank), legs.home_pins.clone())
        };
        let (a, b) = match pick {
            Some(p) => p,
            None => {
                c.issues.push(Issue::new("error", "BAD_LEG", format!("{}: no pair of holes fits it; give its pins", r)));
                continue;
            }
        };
        c.parts[pi].legs = vec![c.hole_name(a), c.hole_name(b)];
        if crate::dump::building() {
            crate::dump::event("legs", vec![("ref", serde_json::json!(r)), ("legs", serde_json::json!(c.parts[pi].legs)),
                                            ("rank", serde_json::json!(rank))]);
        }
        for (i, hole) in [a, b].into_iter().enumerate() {
            c.occupy(hole, &format!("{} leg {}", r, i + 1));
            if !hole.is_rail() {
                let s = c.boards[hole.board as usize].strip(hole);
                let e = extra.entry(nets[i].clone()).or_default();
                if !e.contains(&s) {
                    e.push(s);
                }
            }
        }
        if let Some(hp) = home_pins {
            if !hp.is_empty() {
                let strips: Vec<Strip> = [a, b].iter().filter(|h| !h.is_rail()).map(|&h| c.boards[h.board as usize].strip(h)).collect();
                let mine: Vec<PinId> = hp.iter().copied().filter(|&p| c.pin(p).hole.map(|h| strips.contains(&c.boards[h.board as usize].strip(h))).unwrap_or(false)).collect();
                let x = (c.xy(a).0 + c.xy(b).0) / 2.0;
                let pick = if let Some(&m) = mine.first() { m } else {
                    let mut best = hp[0];
                    let mut bd = (c.pin(best).xy.unwrap().0 - x).abs();
                    for &p in &hp[1..] {
                        let d = (c.pin(p).xy.unwrap().0 - x).abs();
                        if d < bd {
                            best = p;
                            bd = d;
                        }
                    }
                    best
                };
                if !served.contains(&pick) {
                    served.push(pick);
                }
            }
        }
        c.place_leads_pub(pi);
        if is_round(kind) {
            let (pa, pb) = (c.xy(a), c.xy(b));
            c.rounds.push(((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0));
        }
    }
}

#[allow(dead_code)]
fn _kinds(_: StripKind) {}
