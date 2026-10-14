//! Measures of how a built layout reads (bbgen/measure.py): crossings, which placement scores.

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

fn segments(c: &Circuit) -> Vec<(i64, Xy, Xy)> {
    let mut out = Vec::new();
    for w in &c.wires {
        if w.a.is_some() && w.b.is_some() {
            let pts = c.wire_points(w);
            for s in pts.windows(2) {
                out.push((w.num, s[0], s[1]));
            }
        }
    }
    out
}

/// Pairs of wire runs that cross: what makes a layout look tangled.
pub fn crossings(c: &Circuit) -> usize {
    let segs = segments(c);
    let boxes: Vec<(f64, f64, f64, f64)> = segs.iter().map(|&(_, a, b)| {
        (crate::circuit::pymin(a.0, b.0), crate::circuit::pymax(a.0, b.0), crate::circuit::pymin(a.1, b.1), crate::circuit::pymax(a.1, b.1))
    }).collect();
    let mut n = 0;
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let ((wi, a, b), (wj, p, q)) = (segs[i], segs[j]);
            let (bi, bj) = (boxes[i], boxes[j]);
            if wi == wj || bi.1 < bj.0 || bj.1 < bi.0 || bi.3 < bj.2 || bj.3 < bi.2 {
                continue;
            }
            n += cross((a, b), (p, q)) as usize;
        }
    }
    n
}
