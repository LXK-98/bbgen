"""Wires for every net the file leaves unconnected (`autowire: true`).

A net is in pieces: groups of its pins already joined by the board's strips, a part's own buses
or wires written in the file. Its rails are pieces too: the ones its parts already plug into,
the ones its `rail:` names, for a lone ground any `-` rail no other net uses, and for each supply
(`rail: +`, or a name like VSYS, VBUS, 5V) a `+` rail of its own.

The pieces on the board are joined one wire at a time, cheapest first: length, plus a penalty for
slanting, lying along another wire or crossing a part (SHAPE_COST, CROSS_COST), then nearest the
pins. A wire that can't help crossing a part is bent around it. Where one wire won't do cleanly, a
route of two or three straight wires through strips nothing uses yet may (_relay: an A* search over
those strips, RELAY_COST a strip). Where every wire lying on the board would cost more than
FLEX_COST (and FLEX_OVER_COST a part it arches over) in trouble, a flexible jumper arches over
instead: it may slant and cross, but needs a hole at each end like any wire (`flex: false` at the
top of the file turns them off). A slant that no choice of holes could avoid (between two columns, across the trench) costs
little: a short rigid wire is fine there. Off-board pins come last, each
with a flying lead to the nearest free hole of its net, so a connector pin is never the junction
between two parts of the board.

Nets without a rail are routed first, the shortest first: they have the fewest ways to go, and the
rails' nets can nearly always reach a rail. The checklist still lists the rails' nets first, since
they're built first.

Tried and left out: rip-up and reroute. After the first pass, each net left with a flexible jumper
came out together with the nets whose wires lay where it arched over, and was wired again first, the
others after it; the result was kept only with fewer jumpers (then crossings, then wire). On the
benchmarks' layouts it removed no jumper (one crossing fewer on 13, and only with 20 tries) and took
up to six times as long. The jumpers left there are for want of room, not order: a bus between two
chips needs a row of its own for each line past the other's, and the rows beside a chip run out
(13's BUS3-BUS7), so rewiring the nets in the way only turned another of them into a jumper. What
helps there is placement leaving the room. Worth trying again if layouts turn up whose jumpers are
about order (a jumper whose net, wired first, would lie straight without forcing another net off the
board).
"""

from __future__ import annotations

import bisect
import heapq
import math

from .board import RAILS, ROW_Y, Hole, parse_hole
from . import trace
from .check import _DSU
from .circuit import Circuit, Issue, Net, Wire, _aligned, _key, _Option


def _node(c: Circuit, hole: Hole):
    return ("strip", *c.boards[hole.board].strip(hole))


def autowire(c: Circuit) -> None:
    dsu, node_pins = _joined(c)
    c._ends = {_key(c.end_xy(e)) for w in c.wires for e in (w.a, w.b) if e and e.hole is not None}
    c._lain = set().union(*(c._lies_over(w) for w in c.wires))
    in_nets = {p for net in c.nets.values() for p in net.pins}  # pins nothing connects need no hole kept free
    c._pin_strips = {node[1:] for node, pins in node_pins.items() if node[0] == "strip" and set(pins) & in_nets}
    c._strip_at = {_key(b.xy(h)): b.strip(h) for b in c.boards.values() for h in b.holes()}
    rails = _rails(c, dsu, node_pins)
    first = len(c.wires)
    trace.step(c, "parts placed: wiring next", wiring=True, seconds=1.5)
    order = sorted(c.nets.values(), key=lambda n: (bool(rails.get(n.name)), _span(c, n)))
    for flying in (False, True):
        for net in order:
            _wire_net(c, dsu, net, rails.get(net.name, []), node_pins, flying)
    # number them as they'll be built: the rails' nets first, then the rest, each in file order, and the
    # flexible jumpers last
    built = sorted(c.wires[first:], key=lambda w: (w.flex, not rails.get(w.group), list(c.nets).index(w.group), w.num))
    _renumber(c, built, first)
    groups = list(dict.fromkeys(w.group for w in built))
    c.groups += [g for g in groups if g not in c.groups]


def _span(c: Circuit, net: Net) -> float:
    """How far a net's pins on the board are spread (half its bounding box's perimeter). Short nets are routed
    first: they have the fewest ways to go, and a long one can still go round them, or over them as a
    flexible jumper, where a short one lying in its way would have had to."""
    pts = [p.xy for p in net.pins if p.xy and p.hole is not None]
    if len(pts) < 2:
        return 0.0
    xs, ys = [x for x, _ in pts], [y for _, y in pts]
    return (max(xs) - min(xs)) + (max(ys) - min(ys))


def _joined(c: Circuit) -> tuple[_DSU, dict]:
    """What's connected so far (the strips, the parts' own buses and the wires in), and the pins in each
    node."""
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
    return dsu, node_pins


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
                        # the outermost column where it can run straight across
                        col = next((p[1].hole.col for p in ei for q in ej if p[1].hole.col == q[1].hole.col
                                    and _lies_clear(c, p[1].xy, q[1].xy)), None)
                        if col is not None:
                            oi = [p for p in ei if p[1].hole.col == col]
                            oj = [q for q in ej if q[1].hole.col == col]
                # a slant is let off lightly only when no pair of these pieces' holes could be straight
                unavoidable = not any(_could_be_straight(c, a, b) for a in _strip_holes(c, oi)
                                      for b in _strip_holes(c, oj))
                pairs += [(_bound(c, p[1], q[1]), p, q, unavoidable) for p in oi for q in oj
                          if p[1].pin is None or q[1].pin is None]  # two off-board pins: not a board wire
        pairs.sort(key=lambda t: t[0])
        best, flex = None, None
        if c.flex:  # the best flexible jumper, the price a wire lying on the board has to beat
            found = []
            for bound, p, q, _ in pairs:
                if found and bound + FLEX_COST > found[0][0][0] + 1e-9:
                    break  # pairs are sorted by bound: none further on can be cheaper
                if p[1].hole is not None and q[1].hole is not None and not _over_keepout(c, p[1].xy, q[1].xy):
                    key = (round(bound + FLEX_COST + _flex_over(c, p[1].xy, q[1].xy), 6),
                           round(p[1].near + q[1].near, 6), p[1].xy, q[1].xy)
                    found.append((key, p, q))
                    found.sort(key=lambda t: t[0])
            best = flex = found[0] if found else None
        # the full cost of a wire lying on the board (a detour around a part is costly to work out) only while
        # it can still win
        for bound, p, q, unavoidable in pairs:
            if best is not None and bound > best[0][0] + 1e-9:
                break
            key = _cost(c, p[1], q[1], unavoidable)
            if key[0] < math.inf and (best is None or key < best[0]):
                best, flex = (key, p, q), None
        # straight wires through empty strips, where one wire would be a jumper or pay for its trouble
        if not flying and (best is None or flex or best[0][0] > _bound(c, best[1][1], best[2][1]) + RELAY_COST):
            # from the piece the best wire would leave (any piece, when nothing else will do), to any other
            limit = best[0][0] if best is not None else RELAY_LIMIT
            froms = None if best is None else {i for i, g in enumerate(groups) if best[1][2] in g}
            route = _relay(c, net, options, limit, froms)
            if route is not None and _lay_route(c, dsu, net, route, nodes):
                continue
        if best is None:
            return  # no free hole to join them: the check reports the net as open
        _, end_a, end_b = best
        _lay(c, dsu, net, end_a, end_b, bool(flex), flying)


def _lay(c: Circuit, dsu: _DSU | None, net: Net, end_a, end_b, flex: bool = False, flying: bool = False,
         quiet: bool = False) -> Wire:
    """Put in one wire between two (spec, option, node) ends (joining their nodes, unless dsu is None)."""
    (spec_a, pa, na), (spec_b, pb, nb) = end_a, end_b
    num = max((x.num for x in c.wires), default=0) + 1  # wires taken out leave gaps: never reuse a number
    w = Wire(num, net.name, (spec_a, spec_b), None, [], auto=True, flex=flex)
    w.a, w.b = c._take(pa, f"W{w.num}", spec_a), c._take(pb, f"W{w.num}", spec_b)
    c._ends.update(_key(c.end_xy(e)) for e in (w.a, w.b) if e.hole is not None)
    if w.flex:
        c.wires.append(w)  # it arches over the board: nothing lies under it
        if dsu is not None:
            dsu.union(na, nb)
        trace.step(c, f"wiring {net.name}: a flexible jumper", focus=[f"W{w.num}"], wiring=True)
        return w
    if w.a.hole and w.b.hole and c._bodies_crossed(c.end_xy(w.a), c.end_xy(w.b)):
        w.via = c._detour(w.a.hole, w.b.hole, f"W{w.num}") or []  # around the part it would cross
        w.via_xy = [c._via(v) for v in w.via]
    pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
    for p, q in zip(pts, pts[1:]):
        c._add_segment(p, q, f"W{w.num}")
    c.wires.append(w)
    c._lain |= c._lies_over(w)
    if dsu is not None:
        dsu.union(na, nb)
    if not quiet:
        what = "a flying lead" if flying else "bent around a part" if w.via else "a wire"
        trace.step(c, f"wiring {net.name}: {what}", focus=[f"W{w.num}"], wiring=True)
    return w


RELAY_COST = 3.0  # each empty strip a route passes through: one more wire to build
RELAY_HOPS = 3    # at most this many wires in a route through empty strips
RELAY_LIMIT = 80.0  # the most a route may cost when there's nothing else to join two pieces


def _relay(c: Circuit, net: Net, options: list, limit: float, froms=None):
    """The cheapest route of straight, clear wires from one piece of the net to another through strips nothing
    uses yet, costing less than limit: [(from end, to end), ...] with ends as (spec, option, node), or None.

    It's what a builder does when two strips don't line up or something is in the way: run to a free strip,
    and on from another of its holes (the strip itself carries the signal across its rows, or the column
    across the trench)."""
    target: dict[Hole, tuple] = {}
    for gi, opts in enumerate(options):
        for spec, o, node in opts:
            if o.hole is not None:
                target.setdefault(o.hole, (gi, (spec, o, node)))
    if len(target) < 2:
        return None
    empty = _empty_strips(c)
    best = None
    best_cost = limit - 1e-9
    cache: dict = {}
    for gi, opts in enumerate(options):
        if froms is not None and gi not in froms:
            continue
        starts = [(spec, o, node) for spec, o, node in opts if o.hole is not None]
        xs = sorted(c.boards[h.board].xy(h)[0] for h, (gj, _) in target.items() if gj != gi)
        if not starts or not xs:
            continue

        def ahead(xy) -> float:
            """A floor under what's left to go from here: the columns to the nearest hole of another piece."""
            i = bisect.bisect_left(xs, xy[0])
            return min(abs(xs[k] - xy[0]) for k in (i - 1, i) if 0 <= k < len(xs))

        # A*: (cost so far plus the floor, tie, cost so far, wires so far, the ends a next wire may leave from,
        # the path)
        floor = min(ahead(o.xy) for _, o, _ in starts) + RELAY_COST
        heap = [(floor, 0, 0.0, 0, starts, [])]
        seen: dict = {}
        tie = 1
        while heap:
            f, _, cost, hops, ends, path = heapq.heappop(heap)
            if f >= best_cost:
                break
            for end in ends:
                o = end[1]
                for t, length in _reach(c, o.hole, cache):
                    step = cost + length + COVERED_COST * o.covered
                    if step >= best_cost:
                        continue
                    hit = target.get(t)
                    if hit is not None:
                        if hit[0] == gi or not path:
                            continue  # its own piece, or a single wire (not this search's business)
                        total = step + COVERED_COST * hit[1][1].covered
                        if total < best_cost:
                            route = path + [(end, hit[1])]
                            total += CROSS_WIRE_COST * sum(_wires_crossed(c, x[1].xy, y[1].xy) for x, y in route)
                            if total < best_cost:
                                best, best_cost = route, total
                        continue
                    if hops + 1 >= RELAY_HOPS:
                        continue
                    board = c.boards[t.board]
                    strip = board.strip(t)
                    if strip not in empty:
                        continue
                    nxt = step + RELAY_COST
                    txy = board.xy(t)
                    if nxt + ahead(txy) >= best_cost or seen.get(strip, math.inf) <= nxt:
                        continue
                    seen[strip] = nxt
                    node = ("strip", *strip)
                    here = (c.hole_name(t), _Option(t, txy), node)
                    out = [(c.hole_name(h), _Option(h, board.xy(h)), node) for h in board.strip_holes(strip)
                           if h != t and h not in c.occupied and _key(board.xy(h)) not in c._lain]
                    if out:
                        heapq.heappush(heap, (nxt + ahead(txy), tie, nxt, hops + 1, out, path + [(end, here)]))
                        tie += 1
    return best


def _reach(c: Circuit, hole: Hole, cache: dict) -> list[tuple[Hole, float]]:
    """(hole, cost) for each free hole a straight wire from this one could end in, lying clear as _lies_clear
    has it: along its row, and up and down its column. Each line is walked out from the hole a step at a time,
    stopping at the first thing in the way, which is in the way of everything further on too."""
    if hole in cache:
        return cache[hole]
    board = c.boards[hole.board]
    a = board.xy(hole)
    if "levels" not in cache:
        cache["levels"] = sorted((board.y(r), r) for r in (*ROW_Y, *RAILS))
        cache["spare"] = {}
    lines = [[Hole(board.id, hole.row, col) for col in range(hole.col - 1, 0, -1)],
             [Hole(board.id, hole.row, col) for col in range(hole.col + 1, board.columns + 1)],
             [Hole(board.id, r, hole.col) for y, r in reversed(cache["levels"]) if y < a[1]],
             [Hole(board.id, r, hole.col) for y, r in cache["levels"] if y > a[1]]]
    spare = cache["spare"]
    out = []
    for line in lines:
        prev, over, covered = a, False, {}
        for t in line:
            if not board.has(t.row, t.col):
                continue  # a rail's gap: no hole to end in, nor to lie over
            b = board.xy(t)
            if c._bodies_crossed(prev, b) or c._along_who(prev, b):
                break
            if prev is not a:  # the hole stepped past is under the wire now
                k = _key(prev)
                if k in c._ends:
                    break
                over = over or k in c._used
                strip = c._strip_at.get(k)
                if strip in c._pin_strips and k not in c._lain and k not in c._used:
                    covered[strip] = covered.get(strip, 0) + 1
                    if strip not in spare:
                        spare[strip] = _spare(c, strip)
                    if spare[strip] <= covered[strip]:
                        break  # a pin's strip's last free hole: a wire still to come must plug in there
            prev = b
            if t in c.occupied or _key(b) in c._lain or _key(b) in c._covered:
                continue
            out.append((t, math.dist(a, b) + SHAPE_COST[1 if over else 0]))
    cache[hole] = out
    return out


def _empty_strips(c: Circuit) -> set:
    """Strips of the terminal area with nothing in them and nothing lying over any of their holes."""
    out = set()
    for board in c.boards.values():
        for col in range(1, board.columns + 1):
            for half in ("a-e", "f-j"):
                strip = (board.id, half, col)
                holes = board.strip_holes(strip)
                if all(h not in c.occupied and _key(board.xy(h)) not in c._lain and _key(board.xy(h)) not in c._covered
                       for h in holes):
                    out.add(strip)
    return out


def _lay_route(c: Circuit, dsu: _DSU, net: Net, route: list, nodes: list) -> bool:
    """Put in a route's wires one by one, each checked against the ones before; if one no longer lies clear,
    take them all out again (False)."""
    laid = []
    for end_a, end_b in route:
        pa, pb = end_a[1], end_b[1]
        if pa.hole in c.occupied or pb.hole in c.occupied or not _lies_clear(c, pa.xy, pb.xy) \
                or _key(pa.xy) in c._lain or _key(pb.xy) in c._lain:
            for w in laid:
                _unwire(c, w)
            return False
        laid.append(_lay(c, None, net, end_a, end_b, quiet=True))
    for (_, _, na), (_, _, nb) in route:  # joined only now it's all in; and the strips it passed through are the
        dsu.union(na, nb)                 # net's now: later wires may use them
        for node in (na, nb):
            if node not in nodes:
                nodes.append(node)
                c._pin_strips.add(node[1:])
    trace.step(c, f"wiring {net.name}: {len(laid)} wires through {'an empty strip' if len(laid) == 2 else 'empty strips'}",
               focus=[f"W{w.num}" for w in laid], wiring=True)
    return True


def _has_rail(group) -> bool:
    return any(n[0] == "strip" and n[2] in RAILS for n in group)


END_GROUP = 5  # a rail's first and last five holes are its ends


def _rail_ends(c: Circuit, options: list) -> list:
    """The options in a rail's end holes (its first or last five), the outermost first and the right end
    before the left: a wire joining two rails goes there, as far out of the circuit's way as it can (and
    nearest where a supply's leads come in)."""
    out = []
    for o in options:
        h = o[1].hole
        if h is not None and h.is_rail:
            cols = sorted(x.col for x in c.boards[h.board].strip_holes(c.boards[h.board].strip(h)))
            if h.col in cols[:END_GROUP] or h.col in cols[-END_GROUP:]:
                out.append((min(h.col - cols[0], cols[-1] - h.col), -h.col, o))
    return [o for *_, o in sorted(out, key=lambda t: t[:2])]


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
                trace.step(c, f"dropping W{here[0].num}: it only reached a rail nothing uses", wiring=True)
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
        if not mine and kind not in RAILS:
            spec = c.hole_name(hole)  # a strip a route passed through
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
CROSS_WIRE_COST = 1.0  # per wire a wire lying on the board crosses
FLEX_OVER_COST = 6.0  # per part a flexible jumper arches over: it hides the part's pins, and has to come out first


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
        if p.hole is not None and q.hole is not None:
            length += CROSS_WIRE_COST * _wires_crossed(c, p.xy, q.xy)  # wires may cross, but a tangle is hard to trace
    else:  # a flying lead: from the board's edge, over as little as can be
        board_end = q if p.pin is not None else p
        length += 0.0 if board_end.hole.is_rail else OFF_RAIL_COST
        over = [b for b in c._bodies_crossed(p.xy, q.xy) if b not in c._keepouts]
        length += LEAD_OVER_COST * (len(over) + _wires_crossed(c, p.xy, q.xy))
        length += LEAD_TALL_COST * sum(1 for b in over if b in c._tall)  # it would hang right against it
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


def _flex_over(c: Circuit, a, b) -> float:
    """What a flexible jumper from a to b costs for the parts it arches over (a tall one counts double: the
    jumper sags against it)."""
    over = [r for r in c._bodies_crossed(a, b) if r not in c._keepouts]
    return FLEX_OVER_COST * (len(over) + sum(1 for r in over if r in c._tall))


def _over_keepout(c: Circuit, a, b) -> bool:
    """A flexible jumper from a to b would arch over a keep-out, where a USB plug goes."""
    return bool(c._keepouts) and any(z in c._keepouts for z in c._bodies_crossed(a, b))


LINK_COST = 3.0  # a wire joining two rails at their ends
LEAD_OVER_COST = 5.0  # per part or wire a flying lead passes over: more than landing off the rail
LEAD_TALL_COST = 40.0  # per tall part (a can, an LED, a pot) it would pass over: only if there's no other way


def _wires_crossed(c: Circuit, a, b) -> int:
    from .measure import _cross
    n = 0
    for w in c.wires:
        if w.a and w.b:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            n += sum(_cross((a, b), (p, q)) for p, q in zip(pts, pts[1:]))
    return n


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
            best = min(best, bound + FLEX_COST + _flex_over(c, p.xy, q.xy))
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
