"""Wires for every net the file leaves unconnected (`autowire: true`).

A net is in pieces: groups of its pins already joined by the board's strips, a part's own buses
or wires written in the file. Its rails are pieces too: the ones its parts already plug into,
the ones its `rail:` names, for a lone ground net any `-` rail no other net uses, and for a
supply net (VSYS, VBUS, 5V, ...) a `+` rail no other net uses.

The pieces on the board are joined one wire at a time, cheapest first: length, plus a penalty for
slanting, lying along another wire or crossing a part (SHAPE_COST, CROSS_COST), then nearest the
pins. A wire that can't help crossing a part is bent around it. Where every wire lying on the board
would cost more than FLEX_COST in trouble, a flexible jumper arches over instead: it may slant and
cross, but needs a hole at each end like any wire (`flex: false` at the top of the file turns them
off). A slant that no choice of holes could avoid (between two columns, across the trench) costs
little: a short rigid wire is fine there. Off-board pins come last, each
with a flying lead to the nearest free hole of its net, so a connector pin is never the junction
between two parts of the board.

Nets without a rail are routed first: they have the fewest ways to go, and the rails' nets can
nearly always reach a rail. The checklist still lists the rails' nets first, since they're built
first.
"""

from __future__ import annotations

import math

from .board import RAILS, Hole, parse_hole
from . import trace
from .check import _DSU
from .circuit import _GROUND, _SUPPLY, Circuit, Issue, Net, Wire, _aligned, _key, _Option


def _node(c: Circuit, hole: Hole):
    return ("strip", *c.boards[hole.board].strip(hole))


def autowire(c: Circuit) -> None:
    dsu = _DSU()
    node_pins: dict[tuple, list] = {}
    for part in c.parts.values():
        for pin in part.pins:
            node = _pin_node(c, pin)
            if node:
                dsu.find(node)
                node_pins.setdefault(node, []).append(pin)
        for bus in part.buses:
            nodes = [n for n in (_pin_node(c, p) for p in bus) if n]
            for node in nodes[1:]:
                dsu.union(nodes[0], node)
    for w in c.wires:
        if w.a and w.b:
            dsu.union(_end_node(c, w.a), _end_node(c, w.b))

    rails = _rails(c, dsu)
    first = len(c.wires)
    trace.step(c, "parts placed: wiring next", wiring=True, seconds=1.5)
    for flying in (False, True):
        for net in sorted(c.nets.values(), key=lambda n: bool(rails.get(n.name))):
            _wire_net(c, dsu, net, rails.get(net.name, []), node_pins, flying)
    # number them as they'll be built: the rails' nets first, then the rest, each in file order, and the
    # flexible jumpers last
    built = sorted(c.wires[first:], key=lambda w: (w.flex, not rails.get(w.group), list(c.nets).index(w.group), w.num))
    _renumber(c, built, first)
    groups = list(dict.fromkeys(w.group for w in built))
    c.groups += [g for g in groups if g not in c.groups]


def _renumber(c: Circuit, wires: list[Wire], first: int) -> None:
    names = {f"W{w.num}": f"W{first + i + 1}" for i, w in enumerate(wires)}
    for i, w in enumerate(wires):
        w.num = first + i + 1
    c.wires[first:] = wires
    for table in (c.occupied, c._used):
        for k, who in table.items():
            table[k] = names.get(who, who)
    c._segments = [(p, q, names.get(who, who)) for p, q, who in c._segments]
    c._reindex()


def _pin_node(c: Circuit, pin):
    if pin.hole is not None:
        return _node(c, pin.hole)
    if pin.part.kind == "offboard":
        return ("off", pin.part.ref, pin.num)
    return None


def _end_node(c: Circuit, end):
    return ("off", end.pin.part.ref, end.pin.num) if end.pin is not None else _node(c, end.hole)


def _rail_strips(c: Circuit, spec: str) -> list[tuple] | None:
    parsed = parse_hole(spec)
    if not parsed or parsed[1] not in RAILS or parsed[2] is not None:
        return None
    board = c.boards.get(parsed[0] or c.default_board)
    if board is None:
        return None
    return sorted({_node(c, h) for h in board.holes() if h.row == parsed[1]})


def _rails(c: Circuit, dsu: _DSU) -> dict[str, list[tuple]]:
    """The rail strips each net owns."""
    every = [_node(c, h) for b in c.boards.values() for h in b.holes() if h.row in RAILS]
    every = list(dict.fromkeys(every))
    owner: dict[tuple, str] = {}
    for net in c.nets.values():  # rails a net's parts already plug into
        roots = {dsu.find(_pin_node(c, p)) for p in net.pins if _pin_node(c, p)}
        for strip in every:
            if dsu.find(strip) in roots:
                owner[strip] = net.name
    for net in c.nets.values():  # rails a net names
        for spec in net.rails:
            strips = _rail_strips(c, spec)
            if strips is None:
                c.issues.append(Issue("error", "BAD_RAIL", f"nets.{net.name}: rail {spec!r} is not a rail "
                                                           "(top+, top-, bot+ or bot-, optionally 2:top+)"))
                continue
            for strip in strips:
                if owner.get(strip, net.name) != net.name:
                    c.issues.append(Issue("error", "BAD_RAIL", f"nets.{net.name}: {spec} already carries "
                                                               f"{owner[strip]}"))
                    break
                owner[strip] = net.name
    claimed = set(owner.values())
    grounds = [n for n in c.nets.values() if _GROUND.match(n.name) and n.name not in claimed]
    if len(grounds) == 1:  # a lone ground net takes the - rails nobody uses
        for strip in every:
            if strip[2].endswith("-") and strip not in owner:
                owner[strip] = grounds[0].name
    for net in c.nets.values():  # each supply takes a + rail nobody uses, top first
        if _SUPPLY.match(net.name) and net.name not in claimed:
            free = [s for s in every if s[2].endswith("+") and s not in owner]
            for strip in [s for s in free if s[2] == free[0][2]] if free else []:
                owner[strip] = net.name
    out: dict[str, list[tuple]] = {}
    for strip, name in owner.items():
        out.setdefault(name, []).append(strip)
    return out


def _wire_net(c: Circuit, dsu: _DSU, net: Net, rails: list[tuple], node_pins: dict, flying: bool) -> None:
    """Join the net's pieces on the board, or (flying) give each off-board pin its lead."""
    nodes = list(dict.fromkeys([n for n in (_pin_node(c, p) for p in net.pins) if n] + rails))
    if not flying:
        nodes = [n for n in nodes if n[0] != "off"]
    while True:
        pieces: dict = {}
        for node in nodes:
            pieces.setdefault(dsu.find(node), []).append(node)
        if len(pieces) < 2:
            return
        groups = list(pieces.values())
        options = [[o for node in group for o in _options(c, node, net, node_pins)] for group in groups]
        lone = [i for i, g in enumerate(groups) if len(g) == 1 and g[0][0] == "off"]
        pairs = []
        for i in range(len(groups)):
            for j in range(i + 1, len(groups)):
                if flying and (i in lone) == (j in lone):
                    continue  # each lead joins one off-board pin to the board
                pairs += [(_bound(c, p[1], q[1]), p, q) for p in options[i] for q in options[j]
                          if p[1].pin is None or q[1].pin is None]  # two off-board pins: not a board wire
        # the full cost (a detour around a part is costly to work out) only while it can still win
        pairs.sort(key=lambda t: t[0])
        best = None
        for bound, p, q in pairs:
            if best is not None and bound > best[0][0] + 1e-9:
                break
            key = _cost(c, p[1], q[1])
            if best is None or key < best[0]:
                best = (key, p, q)
        if best is None:
            return  # no free hole to join them: the check reports the net as open
        flex = None
        if c.flex:  # the best flexible jumper: its length and ends are all that count
            flex = min((((round(bound + FLEX_COST, 6), round(p[1].near + q[1].near, 6), p[1].xy, q[1].xy), p, q)
                        for bound, p, q in pairs if p[1].hole is not None and q[1].hole is not None), key=lambda t: t[0], default=None)
        if flex and flex[0] < best[0]:
            best = flex
        else:
            flex = None
        _, (spec_a, pa, na), (spec_b, pb, nb) = best
        w = Wire(len(c.wires) + 1, net.name, (spec_a, spec_b), None, [], auto=True, flex=bool(flex))
        w.a, w.b = c._take(pa, f"W{w.num}", spec_a), c._take(pb, f"W{w.num}", spec_b)
        if w.flex:
            c.wires.append(w)  # it arches over the board: nothing lies under it
            dsu.union(na, nb)
            trace.step(c, f"wiring {net.name}: a flexible jumper", focus=[f"W{w.num}"], wiring=True)
            continue
        if w.a.hole and w.b.hole and c._bodies_crossed(c.end_xy(w.a), c.end_xy(w.b)):
            w.via = c._detour(w.a.hole, w.b.hole, f"W{w.num}") or []  # around the part it would cross
            w.via_xy = [c._via(v) for v in w.via]
        pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
        for p, q in zip(pts, pts[1:]):
            c._add_segment(p, q, f"W{w.num}")
        c.wires.append(w)
        dsu.union(na, nb)
        what = "a flying lead" if flying else "bent around a part" if w.via else "a wire"
        trace.step(c, f"wiring {net.name}: {what}", focus=[f"W{w.num}"], wiring=True)


def _options(c: Circuit, node, net: Net, node_pins: dict) -> list[tuple[str, _Option, tuple]]:
    """(spec, option, node) for each place a wire could join this node."""
    if node[0] == "off":
        pin = next(p for p in node_pins[node])
        return [(f"{pin.part.ref}.{pin.num}", _Option(None, pin.xy, pin=pin), node)]
    _, board_id, kind, _ = node
    board = c.boards[board_id]
    mine = [p for p in node_pins.get(node, []) if p in net.pins]
    spec = f"{mine[0].part.ref}.{mine[0].num}" if mine else kind
    out = []
    for hole in board.strip_holes(node[1:]):
        if hole in c.occupied:
            continue
        xy = board.xy(hole)
        near = min((math.dist(xy, p.xy) for p in mine if p.xy), default=0.0)
        out.append((spec, _Option(hole, xy, near, covered=_key(xy) in c._covered), node))
    return out


# What a wire's shape is worth in holes of length (see Circuit._shape for the levels): a slant is
# worth a detour of about twelve holes, lying along another wire much more.
SHAPE_COST = {0: 0.0, 1: 2.0, 2: 12.0, 3: 20.0}
CROSS_COST = 12.0     # per part body crossed, before it's bent around
BEND_COST = 4.0       # a wire bent around a part, on top of its length
COVERED_COST = 20.0   # an end in a hole under a two-lead part's body
OFF_RAIL_COST = 4.0   # a flying lead from a part's strip rather than a rail
FLEX_COST = 10.0      # a flexible jumper rather than one lying on the board: more than a short bend, less than a slant


def _bound(c: Circuit, p: _Option, q: _Option) -> float:
    """A floor under _cost's first term: the straight length and the penalties that don't depend on the route."""
    length = math.dist(p.xy, q.xy)
    if p.pin is not None or q.pin is not None:
        length += 0.0 if (q if p.pin is not None else p).hole.is_rail else OFF_RAIL_COST
    return length + COVERED_COST * (p.covered + q.covered)


def _cost(c: Circuit, p: _Option, q: _Option):
    length = math.dist(p.xy, q.xy)
    if p.pin is None and q.pin is None:
        detour = c._detour_cost(p.hole, q.hole) if c._bodies_crossed(p.xy, q.xy) else None
        if detour:  # it will be bent around the part: what that costs, by the same measures
            (trouble, length), _ = detour
            length += BEND_COST + 2.0 * trouble
        else:
            level = c._shape(p.xy, q.xy)[0]
            if level == 2 and not _could_be_straight(c, p.hole, q.hole):
                level = 1  # no pair of holes in these two strips lines up: a slant is the only way
            length += SHAPE_COST[level] + CROSS_COST * len(c._bodies_crossed(p.xy, q.xy))
    else:
        board_end = q if p.pin is not None else p
        length += 0.0 if board_end.hole.is_rail else OFF_RAIL_COST
    length += COVERED_COST * (p.covered + q.covered)
    return round(length, 6), round(p.near + q.near, 6), p.xy, q.xy


def _could_be_straight(c: Circuit, a: Hole, b: Hole) -> bool:
    """Some hole of a's strip lines up with some hole of b's, so a wire between them needn't slant."""
    if a.board != b.board:
        return False
    board = c.boards[a.board]
    sa, sb = board.strip(a), board.strip(b)
    cache = c.__dict__.setdefault("_straight_strips", {})
    if (sa, sb) not in cache:
        pa = [board.xy(h) for h in board.strip_holes(sa)]
        pb = [board.xy(h) for h in board.strip_holes(sb)]
        cache[sa, sb] = any(_aligned(p, q) for p in pa for q in pb if p != q)
    return cache[sa, sb]
