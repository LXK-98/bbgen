"""Wires for every net the file leaves unconnected (`autowire: true`).

A net is in pieces: groups of its pins already joined by the board's strips, a part's own buses
or wires written in the file. Its rails are pieces too: the ones its parts already plug into,
the ones its `rail:` names, for a lone ground any `-` rail no other net uses, and for each supply
(`rail: +`, or a name like VSYS, VBUS, 5V) a `+` rail of its own.

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
from .circuit import Circuit, Issue, Net, Wire, _aligned, _key, _Option


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

    c._ends = {_key(c.end_xy(e)) for w in c.wires for e in (w.a, w.b) if e and e.hole is not None}
    c._lain = set().union(*(c._lies_over(w) for w in c.wires))
    c._pin_strips = {node[1:] for node in node_pins if node[0] == "strip"}
    c._strip_at = {_key(b.xy(h)): b.strip(h) for b in c.boards.values() for h in b.holes()}
    rails = _rails(c, dsu, node_pins)
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


def _rails(c: Circuit, dsu: _DSU, node_pins: dict) -> dict[str, list[tuple]]:
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
    for strip, name in owner.items():  # never a supply on a - rail, or a ground on a + one
        sign = c.nets[name].polarity
        if sign and not strip[2].endswith(sign):
            c.issues.append(Issue("error", "BAD_RAIL", f"nets.{name} is {'a supply' if sign == '+' else 'a ground'} "
                                                       f"but is on the {strip[2]} rail"))
    claimed = set(owner.values())
    for sign in "-+":  # the rest pick: a lone ground takes the - rails nobody uses; each supply a + rail
        nets = [n for n in c.nets.values() if n.polarity == sign and n.name not in claimed]
        nets.sort(key=lambda n: c.rail_order.index(n.name) if n.name in c.rail_order else len(c.rail_order))
        free = [s for s in every if s[2].endswith(sign) and s not in owner]
        rows = list(dict.fromkeys(s[2] for s in free))  # top first
        if len(nets) == 1 and sign == "-":
            shares = [free]
        elif len(nets) <= len(rows):
            shares = [[s for s in free if s[2] == row] for row in rows]
        else:  # split rails: a half each
            shares = [[s] for s in free]
        for net, share in zip(nets, shares):
            for strip in share:
                owner[strip] = net.name
        for net in nets[len(shares):]:
            c.issues.append(Issue("error", "BAD_RAIL", f"nets.{net.name}: no {sign} rail is left for it"))
    # the lone ground (and a lone supply) also takes its free rail on the other side of the trench when it
    # has pins there that nothing joins to a rail, and dropping them to it saves more than the wire linking
    # the two rails costs
    lone = [(nets[0], sign) for sign in "-+"
            for nets in [[n for n in c.nets.values() if n.polarity == sign and not n.rails]] if len(nets) == 1
            and sum(1 for n in c.nets.values() if n.polarity == sign) == 1]
    for net, sign in lone:
        mine = {strip[2] for strip, name in owner.items() if name == net.name}
        if not mine:
            continue  # it runs on no rail at all
        pieces: dict = {}
        for node in (_pin_node(c, p) for p in net.pins):
            if node:
                pieces.setdefault(dsu.find(node), []).append(node)
        for side, half in (("top", "a-e"), ("bot", "f-j")):
            rail = f"{side}{sign}"
            strips = [s for s in every if s[2] == rail]
            if rail in mine or not strips or any(s in owner for s in strips):
                continue
            own = [s for s in owner if owner[s] == net.name]
            stranded = [g for g in pieces.values()
                        if all(n[0] == "strip" and n[2] == half for n in g)
                        and not any(dsu.find(s) == dsu.find(g[0]) for s in own)]
            gain = 0.0
            for g in stranded:
                rest = [n for other in pieces.values() if other is not g for n in other] + own
                gain += max(0.0, _cheapest(c, net, node_pins, g, rest) - _cheapest(c, net, node_pins, g, strips))
            # the wire linking the rails: cheap at their ends, where it's board setup rather than circuit, if it
            # gets through there; otherwise costed like any other
            link = _link_cost(c, net, node_pins, [s for s in own if s[2].endswith(sign)], strips) if gain else 0.0
            if gain > link:
                for strip in strips:
                    owner[strip] = net.name
    out: dict[str, list[tuple]] = {}
    for strip, name in owner.items():
        out.setdefault(name, []).append(strip)
    return out


def _wire_net(c: Circuit, dsu: _DSU, net: Net, rails: list[tuple], node_pins: dict, flying: bool) -> None:
    """Join the net's pieces on the board, or (flying) give each off-board pin its lead; once the leads are in
    too, drop the wires that only reach a rail nothing else uses (unless the file named that rail)."""
    _join(c, dsu, net, rails, node_pins, flying)
    if flying:
        named = {spec.split(":")[-1] for spec in net.rails}
        _prune(c, [s for s in rails if s[2] not in named], node_pins)


def _join(c: Circuit, dsu: _DSU, net: Net, rails: list[tuple], node_pins: dict, flying: bool) -> None:
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
                oi, oj = options[i], options[j]
                if not flying and _has_rail(groups[i]) and _has_rail(groups[j]):  # rail to rail: at their ends
                    ei, ej = _rail_ends(c, oi), _rail_ends(c, oj)
                    if ei and ej:
                        oi, oj = ei, ej
                # a slant is let off lightly only when no pair of these pieces' holes could be straight
                unavoidable = not any(_could_be_straight(c, a, b) for a in _strip_holes(c, oi)
                                      for b in _strip_holes(c, oj))
                pairs += [(_bound(c, p[1], q[1]), p, q, unavoidable) for p in oi for q in oj
                          if p[1].pin is None or q[1].pin is None]  # two off-board pins: not a board wire
        pairs.sort(key=lambda t: t[0])
        best, flex = None, None
        if c.flex:  # the best flexible jumper, the price a wire lying on the board has to beat
            first = None  # pairs are sorted by bound: only those tied with the first that clears a keep-out
            found = []
            for bound, p, q, _ in pairs:
                if first is not None and bound > first + 1e-9:
                    break
                if p[1].hole is not None and q[1].hole is not None and not _over_keepout(c, p[1].xy, q[1].xy):
                    first = bound if first is None else first
                    found.append(((round(bound + FLEX_COST, 6), round(p[1].near + q[1].near, 6), p[1].xy, q[1].xy),
                                  p, q))
            best = flex = min(found, key=lambda t: t[0], default=None)
        # the full cost of a wire lying on the board (a detour around a part is costly to work out) only while
        # it can still win
        for bound, p, q, unavoidable in pairs:
            if best is not None and bound > best[0][0] + 1e-9:
                break
            key = _cost(c, p[1], q[1], unavoidable)
            if key[0] < math.inf and (best is None or key < best[0]):
                best, flex = (key, p, q), None
        if best is None:
            return  # no free hole to join them: the check reports the net as open
        _, (spec_a, pa, na), (spec_b, pb, nb) = best
        w = Wire(len(c.wires) + 1, net.name, (spec_a, spec_b), None, [], auto=True, flex=bool(flex))
        w.a, w.b = c._take(pa, f"W{w.num}", spec_a), c._take(pb, f"W{w.num}", spec_b)
        c._ends.update(_key(c.end_xy(e)) for e in (w.a, w.b) if e.hole is not None)
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
        c._lain |= c._lies_over(w)
        dsu.union(na, nb)
        what = "a flying lead" if flying else "bent around a part" if w.via else "a wire"
        trace.step(c, f"wiring {net.name}: {what}", focus=[f"W{w.num}"], wiring=True)


def _has_rail(group) -> bool:
    return any(n[0] == "strip" and n[2] in RAILS for n in group)


END_GROUP = 5  # a rail's first and last five holes are its ends


def _rail_ends(c: Circuit, options: list) -> list:
    """The options in a rail's end holes (its first or last five), the right end first: a wire joining two
    rails goes there, out of the circuit's way (and nearest where a supply's leads come in)."""
    out = []
    for o in options:
        h = o[1].hole
        if h is not None and h.is_rail:
            cols = sorted(x.col for x in c.boards[h.board].strip_holes(c.boards[h.board].strip(h)))
            if h.col in cols[:END_GROUP] or h.col in cols[-END_GROUP:]:
                out.append(o)
    return sorted(out, key=lambda o: -o[1].hole.col)


def _prune(c: Circuit, rails: list[tuple], node_pins: dict) -> None:
    """Take out wires whose only job is to reach a rail no pin uses."""
    changed = True
    while changed:
        changed = False
        for strip in rails:
            if node_pins.get(strip):
                continue
            board = c.boards[strip[1]]
            holes = set(board.strip_holes(strip[1:]))
            here = [w for w in c.wires if any(e and e.hole in holes for e in (w.a, w.b))]
            if any(not w.auto for w in here):
                continue  # a wire the file wrote
            if len(here) == 1:
                _unwire(c, here[0])
                changed = True


def _unwire(c: Circuit, w: Wire) -> None:
    c.wires.remove(w)
    who = f"W{w.num}"
    for end in (w.a, w.b):
        if end and end.hole is not None and c.occupied.get(end.hole) == who:
            del c.occupied[end.hole]
            c._used.pop(_key(c.boards[end.hole.board].xy(end.hole)), None)
    c._segments = [s for s in c._segments if s[2] != who]
    c._reindex()
    c._ends = {_key(c.end_xy(e)) for x in c.wires for e in (x.a, x.b) if e and e.hole is not None}
    c._lain = set().union(*(c._lies_over(x) for x in c.wires))


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
        if _key(xy) in c._lain:
            continue  # under a wire lying on the board: an end can't be plugged in there
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


def _cost(c: Circuit, p: _Option, q: _Option, unavoidable: bool = False):
    length = math.dist(p.xy, q.xy)
    if p.pin is None and q.pin is None and c.flex:
        if not _lies_clear(c, p.xy, q.xy):
            return math.inf, 0.0, p.xy, q.xy  # a flexible jumper instead
        length += SHAPE_COST[c._shape(p.xy, q.xy)[0]]
    elif p.pin is None and q.pin is None:
        detour = c._detour_cost(p.hole, q.hole) if c._bodies_crossed(p.xy, q.xy) else None
        if detour:  # it will be bent around the part: what that costs, by the same measures
            (trouble, length), _ = detour
            length += BEND_COST + 2.0 * trouble
        else:
            level = c._shape(p.xy, q.xy)[0]
            if level == 2 and unavoidable:
                level = 1  # no pair of these pieces' holes lines up: a slant is the only way
            length += SHAPE_COST[level] + CROSS_COST * len(c._bodies_crossed(p.xy, q.xy))
    else:
        board_end = q if p.pin is not None else p
        length += 0.0 if board_end.hole.is_rail else OFF_RAIL_COST
    length += COVERED_COST * (p.covered + q.covered)
    return round(length, 6), round(p.near + q.near, 6), p.xy, q.xy


def _lies_clear(c: Circuit, a, b) -> bool:
    """A wire lying on the board from a to b would be straight, clear of every part, along no other wire, and
    over no wire's end: the only kind bbgen lays down itself when it may use flexible jumpers."""
    if not _aligned(a, b) or c._bodies_crossed(a, b) or c._shape(a, b)[0] == 3:
        return False
    between = c._between(a, b)
    if any(k in c._ends for k in between):
        return False
    # never over a pin's strip's last free hole: a wire still to come must be able to plug in there
    covered: dict = {}
    for k in between:
        strip = c._strip_at.get(k)
        if strip in c._pin_strips and k not in c._lain and k not in c._used:
            covered[strip] = covered.get(strip, 0) + 1
    return all(_spare(c, strip) > n for strip, n in covered.items())


def _spare(c: Circuit, strip) -> int:
    """Free holes of a strip that no wire lies over."""
    board = c.boards[strip[0]]
    return sum(1 for h in board.strip_holes(strip) if h not in c.occupied and _key(board.xy(h)) not in c._lain)


def _over_keepout(c: Circuit, a, b) -> bool:
    """A flexible jumper from a to b would arch over a keep-out, where a USB plug goes."""
    return bool(c._keepouts) and any(z in c._keepouts for z in c._bodies_crossed(a, b))


LINK_COST = 3.0  # a wire joining two rails at their ends


def _link_cost(c: Circuit, net: Net, node_pins: dict, a: list, b: list) -> float:
    pa = _rail_ends(c, [o for node in a for o in _options(c, node, net, node_pins)])
    pb = _rail_ends(c, [o for node in b for o in _options(c, node, net, node_pins)])
    if any(p[1].hole.col == q[1].hole.col and _lies_clear(c, p[1].xy, q[1].xy) for p in pa for q in pb):
        return LINK_COST
    return _cheapest(c, net, node_pins, a, b)


def _cheapest(c: Circuit, net: Net, node_pins: dict, a: list, b: list) -> float:
    """Roughly what the cheapest wire between two sets of nodes would cost now (inf if there's none): as
    _cost, but a part in the way costs CROSS_COST rather than the detour around it, which is slow to find."""
    pa = [o for node in a for o in _options(c, node, net, node_pins) if o[1].hole is not None]
    pb = [o for node in b for o in _options(c, node, net, node_pins) if o[1].hole is not None]
    pairs = sorted(((_bound(c, p[1], q[1]), p[1], q[1]) for p in pa for q in pb), key=lambda t: t[0])
    best = math.inf
    for bound, p, q in pairs:
        if bound > best:
            break
        if c.flex and not _over_keepout(c, p.xy, q.xy):
            best = min(best, bound + FLEX_COST)
        if c.flex:
            if _lies_clear(c, p.xy, q.xy):
                best = min(best, bound + SHAPE_COST[c._shape(p.xy, q.xy)[0]])
            continue
        rough = bound + SHAPE_COST[c._shape(p.xy, q.xy)[0]] + CROSS_COST * len(c._bodies_crossed(p.xy, q.xy))
        best = min(best, rough)
    return best


def _strip_holes(c: Circuit, options: list) -> list[Hole]:
    """One hole per strip the options are in (enough for _could_be_straight, which looks at whole strips)."""
    out = {}
    for _, o, _ in options:
        if o.hole is not None:
            out.setdefault(c.boards[o.hole.board].strip(o.hole), o.hole)
    return list(out.values())


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
