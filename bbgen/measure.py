"""Measures of how a built layout reads, beyond what the check reports: what the benchmarks track and
their expectations test, and (crossings) what placement scores.

All of them come from the layout itself, so the same circuit always measures the same.
"""

from __future__ import annotations

import itertools
import math

from .circuit import Circuit, _aligned
from .parts import LEAD_KINDS


def _segments(c: Circuit) -> list[tuple[int, tuple, tuple]]:
    """(wire number, a, b) for each straight run of each wire; a flexible jumper's from end to end."""
    out = []
    for w in c.wires:
        if w.a and w.b:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            out += [(w.num, p, q) for p, q in zip(pts, pts[1:])]
    return out


def _turn(a, b, p) -> float:
    return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])


def _cross(s, t) -> bool:
    """Two runs crossing each other properly (not merely meeting at an end)."""
    (a, b), (p, q) = s, t
    if {a, b} & {p, q}:
        return False
    return _turn(p, q, a) * _turn(p, q, b) < 0 and _turn(a, b, p) * _turn(a, b, q) < 0


def crossings(c: Circuit) -> int:
    """Pairs of wire runs that cross: what makes a layout look tangled."""
    segs = _segments(c)
    boxes = [(min(a[0], b[0]), max(a[0], b[0]), min(a[1], b[1]), max(a[1], b[1])) for _, a, b in segs]
    n = 0
    for i, j in itertools.combinations(range(len(segs)), 2):
        (wi, a, b), (wj, p, q) = segs[i], segs[j]
        bi, bj = boxes[i], boxes[j]
        if wi == wj or bi[1] < bj[0] or bj[1] < bi[0] or bi[3] < bj[2] or bj[3] < bi[2]:
            continue
        n += _cross((a, b), (p, q))
    return n


def bent(c: Circuit) -> int:
    return sum(1 for w in c.wires if w.via and not w.flex)


def slanted(c: Circuit) -> int:
    """Wires lying on the board and two-lead parts that slant, whether or not a warning or slant: covers them."""
    n = 0
    for w in c.wires:
        if w.a and w.b and w.a.hole and w.b.hole and not w.flex:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            n += any(not _aligned(p, q) for p, q in zip(pts, pts[1:]))
    for part in c.parts.values():
        if part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
            n += not _aligned(part.pins[0].xy, part.pins[1].xy)
    return n


def rail_links_inside(c: Circuit) -> int:
    """Wires joining a top rail to a bottom one anywhere but at the rails' ends (their first or last five holes),
    where they'd be out of the way."""
    n = 0
    for w in c.wires:
        if not (w.a and w.b and w.a.hole and w.b.hole and w.a.hole.is_rail and w.b.hole.is_rail):
            continue
        if w.a.hole.row[:3] == w.b.hole.row[:3]:
            continue  # + to - on one side would be a short, which the check reports
        cols = sorted(c.boards[w.a.hole.board].rail_cols)
        ends = {*cols[:5], *cols[-5:]}  # its first or last five holes
        n += not ({w.a.hole.col, w.b.hole.col} <= ends)
    return n


def label_clashes(c: Circuit, canvas) -> int:
    """Labels covering a wire's end or bend, or another label: where it's hard to tell what goes where."""
    spots = []
    for w in c.wires:
        if w.a and w.b:
            spots += [c.end_xy(w.a), c.end_xy(w.b), *w.via_xy]
    boxes = [box for box, _ in canvas.labels]
    n = 0
    for x0, y0, x1, y1 in boxes:
        n += sum(1 for x, y in spots if x0 < x < x1 and y0 < y < y1)
    for a, b in itertools.combinations(boxes, 2):
        n += a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]
    return n


def flying_length(c: Circuit) -> float:
    """Total length of the flying leads to off-board pins."""
    return sum(math.dist(c.end_xy(w.a), c.end_xy(w.b)) for w in c.wires
               if w.a and w.b and (w.a.pin is not None or w.b.pin is not None))
