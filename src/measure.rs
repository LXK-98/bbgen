//! Measures of how a built layout reads: what the benchmarks track and their expectations test, and
//! (crossings) what placement scores. All of them come from the layout itself.

use crate::board::Xy;
use crate::circuit::Circuit;

#[inline]
fn turn(a: Xy, b: Xy, p: Xy) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Two runs crossing each other properly (not merely meeting at an end).
#[inline]
pub fn cross(s: (Xy, Xy), t: (Xy, Xy)) -> bool {
    let ((a, b), (p, q)) = (s, t);
    if a == p || a == q || b == p || b == q {
        return false;
    }
    turn(p, q, a) * turn(p, q, b) < 0.0 && turn(a, b, p) * turn(a, b, q) < 0.0
}

/// Each wire's runs, those of the flying leads (to an off-board pin) or those lying on the board.
fn segments(c: &Circuit, leads: bool) -> Vec<(i64, Xy, Xy)> {
    let mut out = Vec::new();
    for w in &c.wires {
        if w.a.is_some() && w.b.is_some() && w.on_board() != leads {
            let pts = c.wire_points(w);
            for s in pts.windows(2) {
                out.push((w.num, s[0], s[1]));
            }
        }
    }
    out
}

/// Pairs of runs that cross, of one wire from `a` and one from `b` (or within `a`, when `b` is None).
fn crossed(a: &[(i64, Xy, Xy)], b: Option<&[(i64, Xy, Xy)]>) -> usize {
    let bx = |&(_, p, q): &(i64, Xy, Xy)| (crate::circuit::pymin(p.0, q.0), crate::circuit::pymax(p.0, q.0), crate::circuit::pymin(p.1, q.1), crate::circuit::pymax(p.1, q.1));
    let mut n = 0;
    for i in 0..a.len() {
        let others = match b { Some(b) => b, None => &a[i + 1..] };
        for t in others {
            let ((wi, p, q), (wj, r, s)) = (a[i], *t);
            let (bi, bj) = (bx(&a[i]), bx(t));
            if wi == wj || bi.1 < bj.0 || bj.1 < bi.0 || bi.3 < bj.2 || bj.3 < bi.2 {
                continue;
            }
            n += cross((p, q), (r, s)) as usize;
        }
    }
    n
}

/// Pairs of wires on the board that cross: what makes a layout look tangled. Flying leads don't count: where an
/// off-board part sits is the drawing's choice, not the build's, and a lead runs above the board.
pub fn crossings(c: &Circuit) -> usize {
    crossed(&segments(c, false), None)
}

/// Pairs that cross with a flying lead in them (another lead, or a wire on the board): how the drawing reads.
pub fn lead_crossings(c: &Circuit) -> usize {
    let (leads, board) = (segments(c, true), segments(c, false));
    crossed(&leads, None) + crossed(&leads, Some(&board))
}

/// Wires lying on the board that are bent around a part.
pub fn bent(c: &Circuit) -> usize {
    c.wires.iter().filter(|w| !w.via.is_empty() && !w.flex).count()
}

/// Wires lying on the board and two-lead parts that slant, whether or not a warning or slant: covers them.
pub fn slanted(c: &Circuit) -> usize {
    let mut n = 0;
    for w in &c.wires {
        if w.on_board() && !w.flex {
            let pts = c.wire_points(w);
            n += pts.windows(2).any(|s| !crate::circuit::aligned(s[0], s[1])) as usize;
        }
    }
    for part in &c.parts {
        if part.kind.is_lead() && part.pins.iter().all(|p| p.xy.is_some()) {
            n += !crate::circuit::aligned(part.pins[0].xy.unwrap(), part.pins[1].xy.unwrap()) as usize;
        }
    }
    n
}

/// Wires joining a top rail to a bottom one anywhere but at the rails' ends (their first or last five holes).
pub fn rail_links_inside(c: &Circuit) -> usize {
    let mut n = 0;
    for w in &c.wires {
        if !w.on_board() {
            continue;
        }
        let (a, b) = (w.a.as_ref().unwrap().hole.unwrap(), w.b.as_ref().unwrap().hole.unwrap());
        if !(a.is_rail() && b.is_rail()) || a.row.side() == b.row.side() {
            continue;
        }
        let mut at_end = true;
        for h in [a, b] {
            let board = &c.boards[h.board as usize];
            let mut cols: Vec<i32> = board.strip_holes(board.strip(h)).iter().map(|x| x.col).collect();
            cols.sort();
            let k = cols.len();
            at_end = at_end && (cols[..5.min(k)].contains(&h.col) || cols[k.saturating_sub(5)..].contains(&h.col));
        }
        n += !at_end as usize;
    }
    n
}

/// Labels covering a wire's end or bend, or another label.
pub fn label_clashes(c: &Circuit, canvas: &crate::render::Canvas) -> usize {
    let mut spots: Vec<Xy> = Vec::new();
    for w in &c.wires {
        if let (Some(a), Some(b)) = (&w.a, &w.b) {
            spots.push(c.end_xy(a));
            spots.push(c.end_xy(b));
            spots.extend(w.via_xy.iter().copied());
        }
    }
    let boxes: Vec<(f64, f64, f64, f64)> = canvas.labels.iter().map(|(b, _)| *b).collect();
    let mut n = 0;
    for &(x0, y0, x1, y1) in &boxes {
        n += spots.iter().filter(|(x, y)| x0 < *x && *x < x1 && y0 < *y && *y < y1).count();
    }
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            let (a, b) = (boxes[i], boxes[j]);
            n += (a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3) as usize;
        }
    }
    n
}

/// Total length of the flying leads to off-board pins.
pub fn flying_length(c: &Circuit) -> f64 {
    crate::pyf::fsum(c.wires.iter().filter(|w| {
        matches!((&w.a, &w.b), (Some(a), Some(b)) if a.pin.is_some() || b.pin.is_some())
    }).map(|w| crate::pyf::dist(c.end_xy(w.a.as_ref().unwrap()), c.end_xy(w.b.as_ref().unwrap()))))
}
