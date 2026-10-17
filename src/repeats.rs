//! Repeated groups of two-lead parts, placed the same way. A group is the two-lead parts bbgen places that hang
//! together, through nets no chip, rail or lead reaches, on one part the file places: an LED with its resistor and
//! diodes. Groups that are copies of each other (parts of the same kinds, joined the same way, their hand-placed
//! parts the same shape) get one shape, moved along the board to each copy's hand-placed part, as datapath placers
//! lay repeated bit slices and PCB tools copy one channel's layout to the others (docs/design/open-points.md,
//! point 16). A copy the shape doesn't fit is left to the usual leg placement (legs.rs).

use crate::board::{Hole, Strip, Xy, RAILS, TERMINAL};
use crate::circuit::{aligned, pymax, Circuit, PinId, K};
use crate::costs::*;
use crate::legs::{is_round, span, strips_of, Drops, ROUND_GAP};
use crate::parts::Kind;
use crate::pyf;
use crate::{HashMap, HashSet, IndexMap};

/// How many columns from its hand-placed part a group's legs may go.
const REACH: i32 = 8;
/// Leg pairs kept for each part, cheapest first, and shapes tried before taking the best so far.
const BEAM: usize = 8;
const NODES: usize = 600;

struct Group {
    anchor: usize,
    /// the parts bbgen places, outward from the anchor
    parts: Vec<usize>,
}

/// A group matched to a pattern: each pattern part's counterpart and whether its legs swap, and how many columns
/// right of the pattern it lies.
struct Copy {
    parts: Vec<(usize, bool)>,
    shift: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Inner,
    Rail,
    Outer,
}

fn class(c: &Circuit, plan: &IndexMap<String, Vec<String>>, net: &str) -> Class {
    if plan.contains_key(net) {
        Class::Rail
    } else if c.nets[net].pins.iter().all(|p| c.parts[p.part as usize].kind.is_lead()) {
        Class::Inner
    } else {
        Class::Outer
    }
}

fn net(net_of: &HashMap<PinId, String>, part: usize, pin: usize) -> Option<&String> {
    net_of.get(&PinId { part: part as u16, pin: pin as u16 })
}

fn swaps(kind: Kind) -> &'static [bool] {
    if matches!(kind, Kind::Resistor | Kind::Capacitor) { &[false, true] } else { &[false] }
}

/// The groups with exactly one part the file places and at least one bbgen places, left to right.
fn groups(c: &Circuit, auto: &[usize], net_of: &HashMap<PinId, String>, plan: &IndexMap<String, Vec<String>>) -> Vec<Group> {
    let lead: Vec<usize> = (0..c.parts.len()).filter(|&i| c.parts[i].kind.is_lead()).collect();
    let mut root: HashMap<usize, usize> = lead.iter().map(|&i| (i, i)).collect();
    fn find(root: &mut HashMap<usize, usize>, i: usize) -> usize {
        let mut r = i;
        while root[&r] != r {
            r = root[&r];
        }
        root.insert(i, r);
        r
    }
    let mut inner: HashMap<&str, Vec<usize>> = HashMap::default();
    for (name, n) in &c.nets {
        if class(c, plan, name) == Class::Inner {
            let parts: Vec<usize> = n.pins.iter().map(|p| p.part as usize).collect();
            for &p in &parts[1..] {
                let (a, b) = (find(&mut root, parts[0]), find(&mut root, p));
                root.insert(b, a);
            }
            inner.insert(name.as_str(), parts);
        }
    }
    let mut comps: IndexMap<usize, Vec<usize>> = IndexMap::default();
    for &i in &lead {
        let r = find(&mut root, i);
        comps.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for comp in comps.values() {
        let placed: Vec<usize> = comp.iter().copied().filter(|i| !auto.contains(i)).collect();
        if placed.len() != 1 || placed.len() == comp.len() || !c.parts[placed[0]].pins.iter().all(|p| p.hole.is_some())
            || comp.iter().any(|i| c.leg_rank.get(&c.parts[*i].r#ref).copied().unwrap_or(0) > 0) {
            continue;
        }
        let anchor = placed[0];
        let mut order = Vec::new();
        let mut queue = vec![anchor];
        let mut seen: HashSet<usize> = [anchor].into_iter().collect();
        while !queue.is_empty() {
            let p = queue.remove(0);
            let mut next: Vec<usize> = (0..c.parts[p].pins.len()).filter_map(|k| net(net_of, p, k))
                .filter_map(|n| inner.get(n.as_str())).flatten().copied().filter(|q| !seen.contains(q)).collect();
            next.sort();
            next.dedup();
            for q in next {
                seen.insert(q);
                order.push(q);
                queue.push(q);
            }
        }
        out.push(Group { anchor, parts: order });
    }
    out.sort_by_key(|g| {
        let h = c.parts[g.anchor].pins[0].hole.unwrap();
        (h.board, h.col, g.anchor)
    });
    out
}

/// b as a copy of a: the hand-placed parts the same shape, and a part for each of a's, joined the same way.
fn copy_of(c: &Circuit, net_of: &HashMap<PinId, String>, plan: &IndexMap<String, Vec<String>>, a: &Group, b: &Group) -> Option<Copy> {
    let (pa, pb) = (&c.parts[a.anchor], &c.parts[b.anchor]);
    if pa.kind != pb.kind || a.parts.len() != b.parts.len() {
        return None;
    }
    let (ha, hb) = (pa.pins[0].hole?, pb.pins[0].hole?);
    let shift = hb.col - ha.col;
    for k in 0..2 {
        let (x, y) = (pa.pins[k].hole?, pb.pins[k].hole?);
        if x.board != y.board || x.row != y.row || y.col - x.col != shift {
            return None;
        }
    }
    type Map = (HashMap<String, String>, HashMap<String, String>);
    let bind = |m: &mut Map, x: &str, y: &str| -> bool {
        let (cx, cy) = (class(c, plan, x), class(c, plan, y));
        if cx != cy || (cx == Class::Rail && x != y) {
            return false;
        }
        match (m.0.get(x), m.1.get(y)) {
            (Some(v), _) if v != y => false,
            (_, Some(v)) if v != x => false,
            _ => {
                m.0.insert(x.to_string(), y.to_string());
                m.1.insert(y.to_string(), x.to_string());
                true
            }
        }
    };
    let mut m: Map = (HashMap::default(), HashMap::default());
    for k in 0..2 {
        if !bind(&mut m, net(net_of, a.anchor, k)?, net(net_of, b.anchor, k)?) {
            return None;
        }
    }
    fn walk(i: usize, a: &Group, b: &Group, c: &Circuit, net_of: &HashMap<PinId, String>, m: Map, used: &mut Vec<bool>,
            out: &mut Vec<(usize, bool)>, bind: &dyn Fn(&mut Map, &str, &str) -> bool) -> bool {
        if i == a.parts.len() {
            return true;
        }
        let p = a.parts[i];
        for (j, &q) in b.parts.iter().enumerate() {
            if used[j] || c.parts[q].kind != c.parts[p].kind {
                continue;
            }
            for &swap in swaps(c.parts[p].kind) {
                let mut m2 = m.clone();
                let ok = (0..2).all(|k| match (net(net_of, p, k), net(net_of, q, k ^ swap as usize)) {
                    (Some(x), Some(y)) => bind(&mut m2, x, y),
                    _ => false,
                });
                if ok {
                    used[j] = true;
                    out.push((q, swap));
                    if walk(i + 1, a, b, c, net_of, m2, used, out, bind) {
                        return true;
                    }
                    out.pop();
                    used[j] = false;
                }
            }
        }
        false
    }
    let mut used = vec![false; b.parts.len()];
    let mut parts = Vec::new();
    walk(0, a, b, c, net_of, m, &mut used, &mut parts, &bind).then_some(Copy { parts, shift })
}

/// What the copies placed so far hold.
#[derive(Clone, Default)]
struct Claims {
    holes: HashSet<Hole>,
    strips: HashMap<Strip, String>,
    nets: HashMap<String, Vec<Strip>>,
    legs: HashSet<K>,
    covered: HashSet<K>,
    bodies: Vec<(Xy, Xy)>,
}

struct Search<'a> {
    c: &'a Circuit,
    net_of: &'a HashMap<PinId, String>,
    plan: &'a IndexMap<String, Vec<String>>,
    drops: &'a Drops,
    pattern: &'a Group,
    /// how many of the pattern's parts, from the anchor outward, the shape covers
    n: usize,
    copies: Vec<Copy>,
    own: std::cell::RefCell<HashMap<String, HashSet<Strip>>>,
    nodes: usize,
}

impl<'a> Search<'a> {
    fn own(&self, net: &str) -> HashSet<Strip> {
        self.own.borrow_mut().entry(net.to_string()).or_insert_with(|| strips_of(self.c, net, &HashMap::default())).clone()
    }

    fn empty(&self, s: Strip) -> bool {
        !self.c.boards[s.board as usize].strip_holes(s).iter().any(|h| self.c.occupied.contains_key(h))
    }

    /// Whether a leg of this net may go in h: free, and on a rail of its net, in a strip of its net, or in an empty
    /// strip (an inner net keeps to the one strip it has).
    fn fits(&self, cl: &Claims, h: Hole, net: &str) -> bool {
        let c = self.c;
        let b = &c.boards[h.board as usize];
        if !b.has(h.row, h.col) || c.occupied.contains_key(&h) || c.lead_room.contains(&h) || cl.holes.contains(&h)
            || c.under.contains_key(&pyf::key(c.xy(h))) {
            return false;
        }
        if h.is_rail() {
            return self.plan.get(net).map(|r| r.iter().any(|r| r == h.row.name())).unwrap_or(false);
        }
        let s = b.strip(h);
        if let Some(n) = cl.strips.get(&s) {
            return n == net;
        }
        let own = self.own(net);
        if own.contains(&s) {
            return true;
        }
        let has = !own.is_empty() || cl.nets.get(net).map(|v| !v.is_empty()).unwrap_or(false);
        if has && class(c, self.plan, net) == Class::Inner {
            return false;
        }
        self.empty(s)
    }

    /// One part's legs in a and b (pin 0, pin 1) for one copy: what it costs, or None if it doesn't fit.
    fn leg_pair(&self, cl: &mut Claims, part: usize, legs: [Hole; 2], home: f64) -> Option<f64> {
        let c = self.c;
        let nets: Vec<String> = (0..2).map(|k| net(self.net_of, part, k).cloned()).collect::<Option<_>>()?;
        for k in 0..2 {
            if !self.fits(cl, legs[k], &nets[k]) {
                return None;
            }
            cl.holes.insert(legs[k]);
            if !legs[k].is_rail() {
                let s = c.boards[legs[k].board as usize].strip(legs[k]);
                if !cl.strips.contains_key(&s) && !self.own(&nets[k]).contains(&s) {
                    cl.strips.insert(s, nets[k].clone());
                    cl.nets.entry(nets[k].clone()).or_default().push(s);
                }
            }
        }
        let (pa, pb) = (c.xy(legs[0]), c.xy(legs[1]));
        let (ka, kb) = (pyf::key(pa), pyf::key(pb));
        let between = if aligned(pa, pb) { c.between(pa, pb) } else { vec![] };
        if cl.covered.contains(&ka) || cl.covered.contains(&kb) || between.iter().any(|k| cl.legs.contains(k))
            || cl.bodies.iter().any(|&s| crate::measure::cross(s, (pa, pb))) {
            return None;
        }
        cl.legs.extend([ka, kb]);
        cl.covered.extend(between.iter().copied());
        cl.bodies.push((pa, pb));

        let kind = c.parts[part].kind;
        let (lo, hi) = span(kind);
        let sp = pyf::dist(pa, pb);
        let mut cost = 3.0 * pymax(pymax(0.0, lo - sp), sp - hi) + LEG_SLANT_COST * (!aligned(pa, pb)) as i32 as f64;
        cost += 8.0 * self.drops.blocked(legs[0], legs[1], pa, pb) as f64;
        cost += [0.0, 3.0, 0.0, 20.0][c.shape_level(pa, pb) as usize];
        cost += 30.0 * (c.bodies_crossed(pa, pb).len() + c.raised_crossed(pa, pb).len()) as f64;
        cost += 2.0 * between.iter().filter(|k| c.used.contains_key(k)).count() as f64;
        for k in 0..2 {
            let xy = c.xy(legs[k]);
            let crowd = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)].iter()
                .filter(|(dx, dy)| c.used.contains_key(&pyf::key((xy.0 + dx, xy.1 + dy)))).count();
            cost += CROWD_LEG_COST * crowd as f64;
            if legs[k].is_rail() {
                if self.plan.get(&nets[k]).and_then(|r| r.first()).map(|r| r != legs[k].row.name()).unwrap_or(false) {
                    cost += SECOND_RAIL_COST;
                }
            } else if class(c, self.plan, &nets[k]) == Class::Outer {
                let own = self.own(&nets[k]);
                if !own.contains(&c.boards[legs[k].board as usize].strip(legs[k])) {
                    let xs: Vec<f64> = own.iter().map(|&s| c.xy(c.boards[s.board as usize].strip_holes(s)[0]).0).collect();
                    let m = xs.iter().map(|x| (xy.0 - x).abs()).fold(f64::INFINITY, f64::min);
                    cost += if xs.is_empty() { JUNCTION_COST / 2.0 } else { JUNCTION_COST + 0.5 * m };
                }
            }
        }
        cost += 0.5 * ((pa.0 + pb.0) / 2.0 - home).abs();
        if is_round(kind) {
            let mid = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
            cost += ROUND_COST * c.rounds.iter().filter(|&&o| pyf::dist(mid, o) < ROUND_GAP).count() as f64;
        }
        Some(cost)
    }

    /// The middle x of the pattern's hand-placed part, where its parts should gather.
    fn home(&self) -> f64 {
        let xs: Vec<f64> = self.c.parts[self.pattern.anchor].pins.iter().filter_map(|p| p.xy).map(|p| p.0).collect();
        xs.iter().sum::<f64>() / xs.len() as f64
    }

    /// The i-th pattern part's legs in a and b, moved to the k-th copy: what it costs there, or None.
    fn moved(&self, cl: &mut Claims, k: usize, i: usize, a: Hole, b: Hole) -> Option<f64> {
        let copy = &self.copies[k];
        let (q, swap) = copy.parts[i];
        let m = |h: Hole| Hole::new(h.board, h.row, h.col + copy.shift);
        let legs = if swap { [m(b), m(a)] } else { [m(a), m(b)] };
        self.leg_pair(cl, q, legs, self.home() + copy.shift as f64)
    }

    /// The shape so far with the i-th pattern part's legs in a and b: the copies it still fits (the pattern and at
    /// least one more), and what they hold and cost.
    fn extend(&self, st: &Shape, i: usize, a: Hole, b: Hole) -> Option<Shape> {
        let mut alive = st.alive.clone();
        let mut cl = st.claims.clone();
        for k in 0..self.copies.len() {
            if alive[k] {
                let mut tried = cl.clone();
                match self.moved(&mut tried, k, i, a, b) {
                    Some(_) => cl = tried,
                    None => alive[k] = false,
                }
            }
        }
        if !alive[0] || alive.iter().filter(|&&x| x).count() < 2 {
            return None;
        }
        // what the copies left out held is free again: lay the shape out afresh on those that remain
        let mut picks = st.picks.clone();
        picks.push((a, b));
        let mut claims = Claims::default();
        let mut cost = 0.0;
        for (j, &(a, b)) in picks.iter().enumerate() {
            for k in (0..self.copies.len()).filter(|&k| alive[k]) {
                cost += self.moved(&mut claims, k, j, a, b)?;
            }
        }
        Some(Shape { picks, alive, claims, cost })
    }

    /// The shapes one part longer, the most copies first, then the cheapest.
    fn options(&self, st: &Shape) -> Vec<Shape> {
        let c = self.c;
        let i = st.picks.len();
        let p = self.pattern.parts[i];
        let anchor = c.parts[self.pattern.anchor].pins[0].hole.unwrap();
        let board = &c.boards[anchor.board as usize];
        let mut near: Vec<Hole> = Vec::new();
        for col in anchor.col - REACH..=anchor.col + REACH {
            for row in TERMINAL.iter().copied().chain(RAILS) {
                if board.has(row, col) {
                    near.push(Hole::new(anchor.board, row, col));
                }
            }
        }
        let nets: Option<Vec<String>> = (0..2).map(|k| net(self.net_of, p, k).cloned()).collect();
        let nets = match nets {
            Some(n) => n,
            None => return vec![],
        };
        let legs: Vec<Vec<Hole>> = nets.iter().map(|n| near.iter().copied().filter(|&h| self.fits(&st.claims, h, n)).collect()).collect();
        let (lo, hi) = span(c.parts[p].kind);
        let mut first: Vec<(f64, Hole, Hole)> = Vec::new();
        for &a in &legs[0] {
            for &b in &legs[1] {
                let (pa, pb) = (c.xy(a), c.xy(b));
                let d = pyf::dist(pa, pb);
                if d < lo - 1e-9 || d > hi + 1.0 + 1e-9 {
                    continue;
                }
                // the copies are there to read alike: every part in the shape lies straight
                if !aligned(pa, pb) || (!a.is_rail() && !b.is_rail() && board.strip(a) == board.strip(b)) {
                    continue;
                }
                let mut alone = st.claims.clone();
                if let Some(cost) = self.moved(&mut alone, 0, i, a, b) {
                    first.push((cost, a, b));
                }
            }
        }
        first.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let mut out: Vec<Shape> = first.into_iter().take(4 * BEAM).filter_map(|(_, a, b)| self.extend(st, i, a, b)).collect();
        out.sort_by(|x, y| x.key().partial_cmp(&y.key()).unwrap());
        out.truncate(BEAM);
        out
    }

    fn search(&mut self, st: &Shape, best: &mut Option<Shape>) {
        if st.picks.len() == self.n {
            if best.as_ref().map(|b| st.key() < b.key()).unwrap_or(true) {
                *best = Some(st.clone());
            }
            return;
        }
        for next in self.options(st) {
            if self.nodes >= NODES || best.as_ref().map(|b| next.key() >= b.key()).unwrap_or(false) {
                break;
            }
            self.nodes += 1;
            self.search(&next, best);
        }
    }
}

/// A shape, or the start of one: the pattern's parts' legs so far, which copies it fits, and what those hold and
/// cost.
#[derive(Clone)]
struct Shape {
    picks: Vec<(Hole, Hole)>,
    alive: Vec<bool>,
    claims: Claims,
    cost: f64,
}

impl Shape {
    /// The copies it leaves out, then the cost: smaller is better, and neither ever gets smaller as it grows.
    fn key(&self) -> (usize, f64) {
        (self.alive.iter().filter(|&&x| !x).count(), self.cost)
    }
}

/// The best shape for the first n of the pattern's (the first group's) parts, and the groups it fits.
fn shape(c: &Circuit, net_of: &HashMap<PinId, String>, plan: &IndexMap<String, Vec<String>>, drops: &Drops,
         gs: &[&Group], n: usize) -> Option<(Vec<Copy>, Shape)> {
    let pattern = gs[0];
    let copies: Vec<Copy> = gs.iter().map(|g| copy_of(c, net_of, plan, pattern, g)).collect::<Option<_>>()?;
    let alive = vec![true; copies.len()];
    let mut s = Search { c, net_of, plan, drops, pattern, n, copies, own: Default::default(), nodes: 0 };
    let mut best = None;
    s.search(&Shape { picks: vec![], alive, claims: Claims::default(), cost: 0.0 }, &mut best);
    let best = best?;
    let copies = s.copies.into_iter().zip(&best.alive).filter(|(_, &a)| a).map(|(k, _)| k).collect();
    Some((copies, best))
}

/// Places the groups that repeat, each copy the same way where it fits; returns the parts it placed.
pub(crate) fn place(c: &mut Circuit, auto: &[usize], net_of: &HashMap<PinId, String>, plan: &IndexMap<String, Vec<String>>,
             drops: &Drops, extra: &mut HashMap<String, Vec<Strip>>) -> Vec<usize> {
    let all = groups(c, auto, net_of, plan);
    let mut classes: Vec<Vec<usize>> = Vec::new();
    for (i, g) in all.iter().enumerate() {
        match classes.iter_mut().find(|k| copy_of(c, net_of, plan, &all[k[0]], g).is_some()) {
            Some(k) => k.push(i),
            None => classes.push(vec![i]),
        }
    }
    let mut done = Vec::new();
    for class in classes.into_iter().filter(|k| k.len() > 1) {
        // as many parts as repeat, from the hand-placed part outward, on as many copies as they fit: the most copies,
        // then the longest shape; each group in turn is the pattern, in case the first is the one that's blocked
        let mut found: Option<(Vec<Copy>, Shape)> = None;
        for n in (1..=all[class[0]].parts.len()).rev() {
            for k in 0..class.len() {
                let mut gs: Vec<&Group> = vec![&all[class[k]]];
                gs.extend(class.iter().filter(|&&i| i != class[k]).map(|&i| &all[i]));
                if let Some(f) = shape(c, net_of, plan, drops, &gs, n) {
                    let better = match &found {
                        None => true,
                        Some(b) => (f.0.len(), n, -f.1.cost) > (b.0.len(), b.1.picks.len(), -b.1.cost),
                    };
                    if better {
                        found = Some(f);
                    }
                }
            }
        }
        let (copies, picks) = match found {
            Some((copies, best)) => (copies, best.picks),
            None => continue,
        };
        for copy in &copies {
            for (i, &(a, b)) in picks.iter().enumerate() {
                let (q, swap) = copy.parts[i];
                let moved = |h: Hole| Hole::new(h.board, h.row, h.col + copy.shift);
                let legs = if swap { [moved(b), moved(a)] } else { [moved(a), moved(b)] };
                let r = c.parts[q].r#ref.clone();
                c.parts[q].legs = legs.iter().map(|&h| c.hole_name(h)).collect();
                if crate::dump::building() {
                    crate::dump::event("legs", vec![("ref", serde_json::json!(r)), ("legs", serde_json::json!(c.parts[q].legs)),
                                                    ("rank", serde_json::json!(0))]);
                }
                for (k, &h) in legs.iter().enumerate() {
                    c.occupy(h, &format!("{} leg {}", r, k + 1));
                    if !h.is_rail() {
                        let s = c.boards[h.board as usize].strip(h);
                        let n = net(net_of, q, k).unwrap().clone();
                        let e = extra.entry(n).or_default();
                        if !e.contains(&s) {
                            e.push(s);
                        }
                    }
                }
                c.place_leads_pub(q);
                if is_round(c.parts[q].kind) {
                    let (pa, pb) = (c.xy(legs[0]), c.xy(legs[1]));
                    c.rounds.push(((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0));
                }
                c.repeated.push(r);
                done.push(q);
            }
        }
    }
    done
}
