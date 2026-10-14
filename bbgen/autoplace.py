"""Placing what the file leaves unplaced: footprint parts without `at`, two-lead parts without `pins`.

Footprint parts are placed before the circuit is built. A quick estimate scores a placement: how
far each net's pins are apart, how far pins on a rail's net (or a two-lead part's trip to a rail)
are from that rail and whether their column has rail holes at all, how many signal wires would
run side by side past one column, how close parts sit, and how many signal nets have pins either
side of a part no wire can lie across (a display whose pins' strips are under its body). Annealing from a few fixed seeds finds
good placements by that estimate; the best few are built for real, wires and all.

Two-lead parts are placed while the circuit is built, after the footprint parts: each leg goes in
a strip or on a rail of its net (or an empty strip, which a wire then joins to it), the pair chosen
to lie straight, at a natural span for the part, without covering the free holes its strips
still need or standing in a pin's way to its rail. Decoupling caps go next to the pin they serve.

The best build is then repaired one move at a time, each move built and scored for real: another
leg pair for one two-lead part (the rest staying put), or a footprint part turned round, nudged or
moved to where the estimate likes it (its two-lead parts placed afresh). Parts named in warnings
are tried first; the search stops after a fixed number of builds, so the same file always gets
the same layout.
"""

from __future__ import annotations

import bisect
import copy
import itertools
import math
import multiprocessing
import os
import random

from . import trace
from .measure import crossings
from .board import RAILS, ROW_Y, STACK, Board, Hole, parse_hole
from .circuit import (Circuit, CircuitError, Issue, _aligned, _key, _line, check_rail_count, library_for,
                      net_polarity)
from .costs import (CROSS_WEIGHT, CROWD_COST, CROWD_LEG_COST, FLEX_WEIGHT, FLYING_WEIGHT, GAP_COST,
                    HALF_WALL_COST, JUNCTION_COST, KEEP_COST, KEEP_WEIGHT, KEPT_DROP_COST, LABEL_COST,
                    LEG_LINK_COST, LEG_OVER_COST, LEG_SLANT_COST, OFFBOARD_COST, RAIL_COST, RAIL_LINK_WEIGHT,
                    ROOMLESS_COST, ROOM_COST, ROUND_COST, SECOND_RAIL_COST, SPACE_COST, WALL_COST)
from .parts import FOOTPRINT_KINDS, LEAD_KINDS, PartError, body_rect, make_part, rotate

# the estimate, for footprint parts (its weights are in costs.py)
ROOM_REACH = 9         # how far (in columns) from its nets' pins a two-lead part's own strip may be
SEEDS, STEPS, KEEP = 4, 3000, 4  # annealing runs, steps in each, placements kept from each

# two-lead parts' legs (their weights are in costs.py)
ROUND = ("led", "electrolytic")  # parts with a round body about two holes across, drawn from above
ROUND_GAP = 1.9        # their centres closer than this: the bodies touch

# legs: (shortest, longest) natural span in holes
SPAN = {"resistor": (2, 5), "diode": (3, 5), "capacitor": (1, 3), "electrolytic": (1, 2), "led": (1, 3)}


# ---- which nets run on which rails ----

def rail_plan(polarity: dict, explicit: dict, leg_rails: dict, order=()) -> dict[str, list[str]]:
    """{net: [rail, ...]}, as autowire assigns them: the rails each net's `rail:` names and its two-lead parts'
    legs are written on; then a lone ground takes the - rails nobody else has, and each supply (in `order`)
    a free + rail."""
    plan = {name: list(dict.fromkeys([*explicit.get(name, []), *leg_rails.get(name, [])])) for name in polarity}
    taken = {r for rails in plan.values() for r in rails}
    claimed = {n for n, rails in plan.items() if rails}
    for sign in "-+":
        nets = [n for n, p in polarity.items() if p == sign and n not in claimed]
        nets.sort(key=lambda n: list(order).index(n) if n in order else len(order))
        rows = [r for r in (f"top{sign}", f"bot{sign}") if r not in taken]
        # a lone ground or supply plans on both: wiring takes the second when pins need it, and placement keeps
        # those pins' drops to it clear
        shares = [rows] if len(nets) == 1 else [[r] for r in rows]
        for net, share in zip(nets, shares):
            plan[net] += share
            taken |= set(share)
    return {name: rails for name, rails in plan.items() if rails}


def _free_supplies(polarity: dict, explicit: dict, leg_rails: dict) -> list[str]:
    """The supplies that pick their own + rail: the ones placement tries both ways round."""
    return [n for n, p in polarity.items() if p == "+" and not explicit.get(n) and not leg_rails.get(n)]


def _leg_rails(parts: dict, pin_net) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {}
    for part in parts.values():
        for pin, leg in zip(part.pins, part.legs):
            parsed = parse_hole(leg)
            net = pin_net(pin)
            if parsed and parsed[1] in RAILS and net:
                out.setdefault(net, []).append(parsed[1])
    return out


# ---- two-lead parts ----

class _Boards:
    """Every board as one, for placing legs: each hole and strip is looked up on its own board."""

    def __init__(self, c):
        self.by = c.boards
        self.first = c.boards[c.default_board]
        self.id = self.first.id
        self.columns = sum(b.columns for b in self.by.values())

    def strip(self, hole: Hole) -> tuple:
        return self.by[hole.board].strip(hole)

    def strip_holes(self, strip: tuple) -> list[Hole]:
        return self.by[strip[0]].strip_holes(strip)

    def xy(self, hole: Hole):
        return self.by[hole.board].xy(hole)

    def holes(self):
        for b in self.by.values():
            yield from b.holes()

    def rail_y(self, rail: str) -> int:
        return self.first.rail_y(rail)

    def drop_kept(self, hole: Hole, rails: list, room: set) -> bool:
        """A straight drop from the hole to its net's rail on its side of the trench would end in a hole kept
        for a supply's lead."""
        side = "top" if hole.row in "abcde" else "bot"
        return any(Hole(hole.board, r, hole.col) in room for r in rails if r.startswith(side))

    def under_rail(self, hole: Hole) -> bool:
        """The hole's column has rail holes, so a drop from it to a rail can run straight."""
        return hole.col in self.by[hole.board].rail_cols


def place_legs(c, parts: list) -> None:
    """Choose holes for each two-lead part that has no `pins`, then place it as if they were written."""
    net_of = {pin: net.name for net in c.nets.values() for pin in net.pins}
    plan = rail_plan({n: net.polarity for n, net in c.nets.items()},
                     {n: [r.split(":")[-1] for r in net.rails] for n, net in c.nets.items()},
                     _leg_rails(c.parts, net_of.get), c.rail_order)
    board = _Boards(c)
    c._rounds = [((p.pins[0].xy[0] + p.pins[1].xy[0]) / 2, (p.pins[0].xy[1] + p.pins[1].xy[1]) / 2)
                 for p in c.parts.values() if p.kind in ROUND and all(x.xy for x in p.pins)]
    extra: dict[str, set] = {}   # strips a net gained from legs placed here
    drops = _Drops(board, _drops(c, board, plan, net_of))
    served: set = set()          # pins that already have a decoupling cap
    for part in parts:
        nets = [net_of.get(pin) for pin in part.pins]
        if None in nets:
            missing = [str(p) for p, n in zip(part.pins, nets) if n is None]
            c.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: {', '.join(missing)} is in no net, so bbgen "
                                                      "can't tell where it goes; give its pins"))
            continue
        options = [_leg_options(c, board, n, plan, extra) for n in nets]
        reach = SPAN[part.kind][1] + 4  # far longer than the part: only if nothing shorter fits
        options = [opts + _empty_near(c, board, opts, other, reach) for opts, other in zip(options, options[::-1])]
        for i, other in enumerate(options[::-1]):  # nothing free nearby (beside a Pico, say): look further out
            if not options[i]:
                options[i] = _empty_near(c, board, [], other, board.columns)
        if not all(options):
            c.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: no free hole for a leg; give its pins"))
            continue
        legs = _Legs(c, board, part, nets, [_strips(c, board, n, extra) for n in nets],
                     _home(c, part, nets, plan, served), drops, plan)
        xy = legs.xy
        pick = legs.choose(_pairs(board, options, reach), c._leg_rank.get(part.ref, 0))
        if pick is None:
            c.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: no pair of holes fits it; give its pins"))
            continue
        a, b = pick
        part.legs = [c.hole_name(a), c.hole_name(b)]
        for i, hole in enumerate((a, b)):
            c._occupy(hole, f"{part.ref} leg {i + 1}")
            if not hole.is_rail:
                extra.setdefault(nets[i], set()).add(board.strip(hole))
        if legs.home_pins:  # the pin this cap now serves: the one whose strip a leg is in, else the nearest
            strips = {board.strip(h) for h in (a, b) if not h.is_rail}
            mine = [p for p in legs.home_pins if p.hole is not None and board.strip(p.hole) in strips]
            x = (xy(a)[0] + xy(b)[0]) / 2
            served.add(mine[0] if mine else min(legs.home_pins, key=lambda p: abs(p.xy[0] - x)))
        c._place_leads(part)
        if part.kind in ROUND:
            c._rounds.append(((xy(a)[0] + xy(b)[0]) / 2, (xy(a)[1] + xy(b)[1]) / 2))


def _pairs(board: "_Boards", options: list[list[Hole]], reach: float) -> list[tuple]:
    """(a, b, a's position, b's position) for leg holes in different strips, at most reach apart (or any
    distance, if no pair is that close)."""
    first = [(h, board.xy(h), board.strip(h)) for h in options[0]]
    second = sorted(((board.xy(h), i, h, board.strip(h)) for i, h in enumerate(options[1])), key=lambda t: t[:2])
    xs = [t[0][0] for t in second]
    out = []
    for a, pa, sa in first:
        window = second[bisect.bisect_left(xs, pa[0] - reach):bisect.bisect_right(xs, pa[0] + reach)]
        for pb, i, b, sb in sorted(window, key=lambda t: t[1]):
            if sb != sa and (pb[0] - pa[0]) ** 2 + (pb[1] - pa[1]) ** 2 <= reach * reach:
                out.append((a, b, pa, pb))
    if not out:
        out = [(a, b, pa, board.xy(b)) for a, pa, sa in first for b in options[1] if board.strip(b) != sa]
    return out


class _Legs:
    """Scores leg pairs for one two-lead part, with what doesn't change between pairs worked out once. The
    cost is a cheap part (span, slant, drops in the way, extra wires, distance from home) and a costly
    part (what it lies along or over, strips it fills up); both are never negative, so pairs can be taken
    cheapest first and the costly part skipped once a pair can't win."""

    def __init__(self, c, board: "_Boards", part, nets: list, own: list, home, drops, plan: dict):
        self.c, self.board, self.part, self.drops = c, board, part, drops
        self.own = own
        self.own_x = [[board.xy(board.strip_holes(s)[0])[0] for s in mine] for mine in own]
        self.lead_x = []
        for net in nets:  # a net that goes off the board: keep its new strips near the connector
            lead = [p for p in c.nets[net].pins if p.part.kind == "offboard"]
            self.lead_x.append(lead[0].xy[0] if lead else None)
        self.on_rail = [n in plan for n in nets]
        self.main_rail = [plan[n][0] if plan.get(n) else None for n in nets]
        self.rails = [plan.get(n, []) for n in nets]
        self.room = getattr(c, "_lead_room", set())
        self.home_x = home[0] if home and home[0] is not None else None
        self.home_pins = home[1] if home and home[0] is None else None
        self._xy: dict = {}
        self._free: dict = {}
        self._leg: dict = {}

    def xy(self, hole: Hole):
        if hole not in self._xy:
            self._xy[hole] = self.board.xy(hole)
        return self._xy[hole]

    def leg(self, i: int, leg: Hole, xy) -> float:
        """What a pair's cost owes to one leg alone (the i-th) being in this hole: worked out once per hole."""
        if (i, leg) in self._leg:
            return self._leg[i, leg]
        used = self.c._used  # room around it, so parts and their labels don't pile up
        cost = CROWD_LEG_COST * sum(_key((xy[0] + dx, xy[1] + dy)) in used for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1)))
        if leg.is_rail and self.main_rail[i] and leg.row != self.main_rail[i]:
            cost += SECOND_RAIL_COST  # its net's other rail: a wire then has to link the two
        own = not leg.is_rail and self.board.strip(leg) in self.own[i]
        if self.lead_x[i] is not None and not own:
            # near the connector its net comes in by: a lead to a new strip gets shorter; on a rail, which carries it
            # anyway, only to break ties
            cost += (0.05 if leg.is_rail else 0.3) * abs(xy[0] - self.lead_x[i])
        if not (leg.is_rail or own):
            if self.own_x[i]:  # an empty strip for a net that has strips: it'll need a wire to them
                cost += JUNCTION_COST + 0.5 * min(abs(xy[0] - x) for x in self.own_x[i])
            else:  # ... or to its rail or connector: one wire, still better than a slant
                cost += JUNCTION_COST / 2
            if self.on_rail[i] and not self.board.under_rail(leg):
                cost += GAP_COST  # the wire down to its rail would slant
            elif self.on_rail[i] and self.board.drop_kept(leg, self.rails[i], self.room):
                cost += KEPT_DROP_COST  # its drop would take the rail's end, kept for the supply's lead
        self._leg[i, leg] = cost
        return cost

    def free(self, strip) -> list:
        """The strip's free holes that nothing lies over, with their positions' keys."""
        if strip not in self._free:
            self._free[strip] = [(h, _key(self.xy(h))) for h in self.board.strip_holes(strip)
                                 if h not in self.c.occupied and _key(self.xy(h)) not in self.c._covered]
        return self._free[strip]

    def cheap(self, a: Hole, b: Hole, pa, pb) -> float:
        lo, hi = SPAN[self.part.kind]
        span = math.dist(pa, pb)
        cost = 3.0 * max(0.0, lo - span, span - hi) + LEG_SLANT_COST * (not _aligned(pa, pb))
        cost += 8.0 * self.drops.blocked(a, b, pa, pb)  # in the way of pins' own straight drops to their rails
        cost += self.leg(0, a, pa) + self.leg(1, b, pb)
        x = (pa[0] + pb[0]) / 2
        if self.part.kind in ROUND:  # an LED or a can is about two holes across: never touching another
            mid = (x, (pa[1] + pb[1]) / 2)
            cost += ROUND_COST * sum(1 for other in self.c._rounds if math.dist(mid, other) < ROUND_GAP)
        if self.home_x is not None:
            cost += 0.5 * abs(x - self.home_x)
        elif self.home_pins:
            cost += 0.5 * min(abs(x - p.xy[0]) for p in self.home_pins)
        return cost

    def costly(self, a: Hole, b: Hole, pa, pb) -> float:
        c = self.c
        cost = {0: 0.0, 1: 3.0, 2: 0.0, 3: 20.0}[c._shape(pa, pb)[0]]
        cost += 30.0 * len(c._bodies_crossed(pa, pb))
        covered = c._between(pa, pb) if _aligned(pa, pb) else []
        for leg in (a, b):  # a strip it fills up can't take the wire its net still needs
            if not leg.is_rail and not any(h != a and h != b and k not in covered
                                           for h, k in self.free(self.board.strip(leg))):
                cost += 15.0
        return cost + 2.0 * sum(1 for k in covered if k in c._used)

    def choose(self, pairs: list, rank: int):
        """The best pair, or (for autoplace's repair search) the rank-th best."""
        top: list = []
        scored = sorted(((self.cheap(a, b, pa, pb), i) for i, (a, b, pa, pb) in enumerate(pairs)))
        for cheap, i in scored:
            if len(top) > rank and cheap > top[rank][0] + 1e-9:
                break  # nothing from here on can make the top
            a, b, pa, pb = pairs[i]
            top.append((cheap + self.costly(a, b, pa, pb), i, a, b))
            top.sort()
            del top[rank + 1:]
        return top[min(rank, len(top) - 1)][2:] if top else None


def _drops(c, board: "_Boards", plan: dict, net_of: dict) -> list[tuple[Hole, Hole]]:
    """(strip hole nearest the rail, rail hole) for each footprint pin's straight drop to its net's rail."""
    out = []
    for part in c.parts.values():
        if part.kind not in FOOTPRINT_KINDS:
            continue
        for pin in part.pins:
            if pin.hole is None or pin.hole.is_rail:
                continue
            top = pin.hole.row in "abcde"
            rails = [r for r in plan.get(net_of.get(pin), []) if r.startswith("top" if top else "bot")]
            own = c.boards[pin.hole.board]
            if rails and own.has(rails[0], pin.hole.col):
                out.append((Hole(own.id, "a" if top else "j", pin.hole.col), Hole(own.id, rails[0], pin.hole.col)))
    return out


def _strips(c, board: "_Boards", net: str, extra: dict) -> set:
    """The strips a net has on the board so far."""
    return {board.strip(p.hole) for p in c.nets[net].pins if p.hole is not None and not p.hole.is_rail} | \
        extra.get(net, set())


class _Drops:
    """Pins' straight drops to their rails, indexed for quick lookups."""

    def __init__(self, board: "_Boards", drops: list[tuple[Hole, Hole]]):
        self.ends: dict[Hole, int] = {}
        self.lines: dict[tuple, list] = {}
        for start, rail in drops:
            for h in (start, rail):
                self.ends[h] = self.ends.get(h, 0) + 1
            line = _line(board.xy(start), board.xy(rail))
            self.lines.setdefault(line[0], []).append((line[1], line[2]))

    def blocked(self, a: Hole, b: Hole, pa, pb) -> int:
        """How many drops a part with legs in a and b would be in the way of."""
        n = self.ends.get(a, 0) + self.ends.get(b, 0)
        line = _line(pa, pb)
        if line:
            n += sum(min(line[2], hi) - max(line[1], lo) > 1e-6 for lo, hi in self.lines.get(line[0], ()))
        return n


def _leg_options(c, board: "_Boards", net: str, plan: dict, extra: dict) -> list[Hole]:
    """Free holes a leg on this net could go in: its strips and its rails (not the end kept for a supply's
    leads)."""
    strips = sorted({board.strip(p.hole) for p in c.nets[net].pins if p.hole is not None} | extra.get(net, set()))
    holes = [h for s in strips for h in board.strip_holes(s)]
    holes += [h for h in board.holes() if h.row in plan.get(net, [])]
    room = getattr(c, "_lead_room", set())
    return [h for h in holes if h not in c.occupied and h not in room]


def lead_room(c) -> set:
    """lead_room for a circuit with its parts placed: worked out the way placement and wiring both see it."""
    net_of = {pin: net.name for net in c.nets.values() for pin in net.pins}
    plan = rail_plan({n: net.polarity for n, net in c.nets.items()},
                     {n: [r.split(":")[-1] for r in net.rails] for n, net in c.nets.items()},
                     _leg_rails(c.parts, net_of.get), c.rail_order)
    return _lead_room(c, _Boards(c), plan, net_of)


def _lead_room(c, board: "_Boards", plan: dict, net_of: dict) -> set:
    """The rail holes a supply's flying leads should land in: the two at the rail's end nearest the connector,
    on the rail of its net nearest the pin. Parts keep out of them, so a lead comes in from the board's edge
    rather than reaching past them."""
    out = set()
    for part in c.parts.values():
        if part.kind != "offboard":
            continue
        for pin in part.pins:
            rails = plan.get(net_of.get(pin), [])
            if not rails or pin.xy is None:
                continue
            row = min(rails, key=lambda r: abs(board.rail_y(r) - pin.xy[1]))
            holes = sorted((h for h in board.holes() if h.row == row), key=lambda h: abs(board.xy(h)[0] - pin.xy[0]))
            out.update(holes[:2])
    return out


def _empty_near(c, board: "_Boards", mine: list[Hole], others: list[Hole], reach: float) -> list[Hole]:
    """Free holes in empty strips: within reach of the other leg's strips, or anywhere if the net has nothing
    on the board yet. A leg there is joined to its net later by a wire. For a net on a rail, only right
    beside the other leg (an LED lying flat beside its resistor, a wire then dropping to the rail) rather
    than slanting to the rail."""
    if any(h.is_rail for h in mine):
        reach = min(reach, 3)
    xs = {board.xy(h)[0] for h in others if not h.is_rail}
    if mine and not xs:
        return []
    out = []
    for strip in sorted({board.strip(h) for h in board.holes() if not h.is_rail}):
        x = board.xy(board.strip_holes(strip)[0])[0]
        if xs and not any(abs(x - other) <= reach for other in xs):
            continue
        holes = board.strip_holes(strip)
        if not any(h in c.occupied for h in holes):
            out += holes
    return out


def _home(c, part, nets, plan, served):
    """(x, pin) the part should sit near: its `near` part, or for a decoupling cap the nearest pin of its
    supply (on its `near` part) that has no cap yet."""
    decoupling = all(n in plan for n in nets)
    if part.near and part.near in c.parts and not decoupling:
        xs = [p.xy[0] for p in c.parts[part.near].pins if p.xy]
        return (sum(xs) / len(xs), None) if xs else None
    if decoupling:  # by a supply pin that has no cap yet (of its near part, if it names one)
        supply = [n for n in nets if c.nets[n].polarity != "-"] or nets
        pins = [p for p in c.nets[supply[0]].pins if p.hole is not None and p.part.kind in FOOTPRINT_KINDS
                and (not part.near or p.part.ref == part.near)]
        pins = [p for p in pins if p not in served] or pins
        if pins:
            return None, pins  # resolved per candidate: the nearest of these
    return None


# ---- footprint parts ----

class _Model:
    """Just enough of a circuit to score placements quickly."""

    def __init__(self, data: dict):
        # the boards side by side, as one row of columns: x is a column's place along it (the first board's
        # columns are 1, 2, ...; each further board's follow on after a gap)
        self.boards: list[tuple[Board, int]] = []  # (board, x of its column 0)
        self.below: dict[str, Board] = {}  # a board's id -> the board snapped on under it
        self.above: dict[str, Board] = {}
        x0 = 0
        for i, spec in enumerate(data.get("boards") or [{"type": "full"}]):
            spec = {"type": spec} if isinstance(spec, str) else spec
            board = Board(spec.get("id", i + 1), spec.get("type", "full"), spec.get("rails"), spec.get("split_rails", False))
            if spec.get("below") is not None:
                up = next((b for b, _ in self.boards if b.id == str(spec["below"])), None)
                if up is not None:
                    self.below[up.id], self.above[board.id] = board, up
            self.boards.append((board, x0))
            x0 += board.columns + 6  # each board's columns along a row of its own: one under another too
        self.board = self.boards[0][0]
        self.offset = {board.id: x0 for board, x0 in self.boards}
        self.width = self.boards[-1][1] + self.boards[-1][0].columns
        self.gaps = {x0 + c for board, x0 in self.boards for c in range(1, board.columns + 1) if c not in board.rail_cols}
        self._lo, self._hi = 1, self.board.columns  # the columns of the board a layout is being worked out on
        self.strips = {(half, x0 + c) for board, x0 in self.boards for c in range(1, board.columns + 1)
                       for half in ("top", "bot")}
        library = library_for(data)
        self.parts = {str(ref): make_part(str(ref), spec, library) for ref, spec in (data.get("parts") or {}).items()}
        self.nets: dict[str, list] = {}
        explicit: dict[str, list[str]] = {}
        polarity: dict[str, str | None] = {}
        self.guessed: set = set()
        for name, spec in (data.get("nets") or {}).items():
            spec = {"pins": spec} if isinstance(spec, list) else spec
            pins = []
            for text in spec.get("pins") or []:
                ref, _, token = str(text).partition(".")
                if ref in self.parts:
                    pins += [p for p in self.parts[ref].find(token) if p not in pins]
            self.nets[str(name)] = pins
            sign, physical, guessed = net_polarity(str(name), spec)
            polarity[str(name)] = sign
            if guessed:
                self.guessed.add(str(name))
            explicit[str(name)] = [r.split(":")[-1] for r in physical]
        check_rail_count(polarity, self.guessed, [b for b, _ in self.boards])
        self.net_of = {pin: name for name, pins in self.nets.items() for pin in pins}
        legs = _leg_rails(self.parts, self.net_of.get)
        self.free_supplies = _free_supplies(polarity, explicit, legs)
        self.plan = rail_plan(polarity, explicit, legs, data.get("_rail_order") or ())
        self.rail_y = {r: self.board.rail_y(r) for r in RAILS}
        # two-lead parts join two nets
        self.links = [tuple(self.net_of.get(p) for p in part.pins) for part in self.parts.values()
                      if part.kind in LEAD_KINDS]
        self.links = [(a, b) for a, b in self.links if a and b]
        # a pin's net reaches a rail directly or through a two-lead part
        self.to_rail = {n: list(self.plan[n]) for n in self.plan}
        for a, b in self.links:
            for x, y in ((a, b), (b, a)):
                if y in self.plan and x not in self.plan:
                    self.to_rail.setdefault(x, []).extend(self.plan[y])
        self.offboard = {n for n, pins in self.nets.items() if any(p.part.kind == "offboard" for p in pins)}
        self.auto = [r for r, p in self.parts.items() if p.kind in FOOTPRINT_KINDS and not p.at]
        self.choices = {r: self._choices(self.parts[r]) for r in self.auto}
        for ref, choices in self.choices.items():
            if not choices:
                raise PartError(f"parts.{ref}: there's no place on the board where its pins all land in holes "
                                "without two of them sharing a strip; give it at:, or use another footprint")
        self.fixed = {}
        for ref, part in self.parts.items():
            if part.kind in FOOTPRINT_KINDS and part.at:
                parsed = parse_hole(str(part.at))
                if parsed and parsed[2] is not None and parsed[1] in ROW_Y:
                    board = next((b for b, _ in self.boards if b.id == str(parsed[0])), self.board)
                    self.fixed[ref] = self._layout(part, parsed[1], parsed[2], part.rot, board)

    def _views(self, board: Board) -> list:
        """(shift, dx, lo, hi) for the board a part is placed on and any board snapped on right below or above
        it: a height on the placed board's scale less shift is a height on that board, a column plus dx its
        column along the model's row of boards, and lo-hi its columns there. A part may reach across onto a board
        stacked next to it (an ESP32 across two), never onto one beside it."""
        off = self.offset[board.id]
        out = [(0, 0, off + 1, off + board.columns)]
        for other, shift in ((self.below.get(board.id), STACK), (self.above.get(board.id), -STACK)):
            if other is not None:
                o = self.offset[other.id]
                out.append((shift, o - off, o + 1, o + other.columns))
        return out

    def _layout(self, part, row: str, col: int, rot: int, board: Board | None = None):
        """(at, rot, {pin: (x, y)}, body [(half, lo, hi)], keep-out [(half, lo, hi)], walls, rows taken) for pin
        1 in row/col of a board (the first by default), or None if a pin misses the holes. A pin's x is its
        column along the model's row of boards, its y its row's height on its own board."""
        board = board or self.board
        off = self.offset[board.id]
        self._lo, self._hi = off + 1, off + board.columns
        views = self._views(board)
        ax, ay = off + col, ROW_Y[row]
        xy, view_of = {}, {}
        for pin in part.pins:
            dx, dy = rotate(pin.offset, rot)
            x, y = ax + dx, ay + dy
            for v in views:  # on the board it's placed on, else on one stacked next to it
                shift, vdx, lo, hi = v
                if y - shift in ROW_Y.values() and lo <= x + vdx <= hi:
                    xy[pin], view_of[pin] = (x + vdx, y - shift), v
                    break
            else:
                return None
        if part.kind in ("sip", "kicad"):  # no two of its pins in one strip, unless joined inside it anyway
            strips: dict = {}
            for pin, (x, y) in xy.items():
                other = strips.setdefault((x, y < 8), pin)
                if other is not pin and (part.bus_of(pin) is None or part.bus_of(pin) is not part.bus_of(other)):
                    return None
        if body_rect(part):  # the whole body on the boards, not hanging off an edge
            corners = [rotate((x, y), rot) for x, y in ((body_rect(part)[0], body_rect(part)[1]),
                                                        (body_rect(part)[2], body_rect(part)[3]))]
            if min(ax + x for x, _ in corners) < self._lo - 0.6 or max(ax + x for x, _ in corners) > self._hi + 0.6:
                return None
        across = len(set(view_of.values())) > 1
        # what it takes up, board by board: its pins and the holes under its body, half by half (sorted: compared,
        # and summed)
        spans = []
        for v in views:
            mine = [xy[p] for p in part.pins if view_of[p] == v]
            body = self._covered(body_rect(part), rot, ax, ay, v)
            if not mine and not body:
                continue
            xs = [x for x, _ in mine] + ([body[0], body[1]] if body else [])
            halves = {"top" if y < 8 else "bot" for _, y in mine} | (body[2] if body else set())
            if part.kind in ("pico", "dip") and not across:
                halves = {"bot", "top"}  # a DIP or Pico straddles the trench
            spans += [(h, min(xs), max(xs)) for h in sorted(halves)]
        spans.sort(key=lambda sp: (sp[1], sp[0]))
        lo, hi = min(sp[1] for sp in spans), max(sp[2] for sp in spans)
        keep = []
        for what, rect in part.keepouts:
            room = self._covered(rect, rot, ax, ay, views[0])
            if room:
                klo, khi = room[0], room[1]
                if "cable" in what.lower():  # the cable runs on past the plug, off the board's nearest edge
                    if khi < lo:
                        klo = self._lo
                    elif klo > hi:
                        khi = self._hi
                keep += [(h, klo, khi) for h in sorted(room[2])]
        prefix = "" if board is self.board else f"{board.id}:"
        walls = [w for v in views for w in self._walls(part, rot, ax, ay, xy, v)]
        return (f"{prefix}{row}{col}", rot, xy, spans, keep, walls, self._taken(part, rot, ax, ay, xy, views))

    @staticmethod
    def _box(rect, rot: int, ax: float, ay: float):
        corners = [rotate((x, y), rot) for x in (rect[0], rect[2]) for y in (rect[1], rect[3])]
        return (ax + min(x for x, _ in corners), ay + min(y for _, y in corners),
                ax + max(x for x, _ in corners), ay + max(y for _, y in corners))

    def _taken(self, part, rot: int, ax: int, ay: int, xy: dict, views: list) -> dict:
        """{(half, column): rows} the part takes in each strip it's in: its pins, and the holes under its body.
        What's left is the room wires have to run past it along the board."""
        taken: dict = {}
        for x, y in xy.values():
            key = ("top" if y < 8 else "bot", x)
            taken[key] = taken.get(key, 0) + 1
        rect = body_rect(part)
        if rect is not None:
            x0, y0, x1, y1 = self._box(rect, rot, ax, ay)
            pins = set(xy.values())
            for shift, dx, lo, hi in views:
                for x in range(max(lo, math.ceil(x0 + dx + 1e-6)), min(hi, math.floor(x1 + dx - 1e-6)) + 1):
                    for y in ROW_Y.values():
                        if y0 < y + shift < y1 and (x, y) not in pins:
                            key = ("top" if y < 8 else "bot", x)
                            taken[key] = taken.get(key, 0) + 1
        return taken

    def _walls(self, part, rot: int, ax: int, ay: int, xy: dict, view) -> list:
        """[(half, first column, last column)] for each half of a board the part walls off: its body covers it,
        or leaves its pins' strips a hole at most, which a wire lying across would take from the wire that pin
        still needs (a display whose pins' strips are under it). Nets with pins on both sides have to go round by
        the other half, or jump over."""
        rect = body_rect(part)
        if rect is None:
            return []
        shift, dx, vlo, vhi = view
        x0, y0, x1, y1 = self._box(rect, rot, ax, ay)
        lo, hi = max(vlo, math.ceil(x0 + dx + 1e-6)), min(vhi, math.floor(x1 + dx - 1e-6))
        if lo > hi:
            return []
        under = {y for y in ROW_Y.values() if y0 < y + shift < y1}
        pins = {y for x, y in xy.values() if vlo <= x <= vhi}
        out = []
        for half, rows in (("top", "abcde"), ("bot", "fghij")):
            ys = [ROW_Y[r] for r in rows]
            free = [y for y in ys if y not in under and y not in pins]
            if any(y in under for y in ys) and (not free or len(free) <= 1 and any(y in pins for y in ys)):
                out.append((half, lo, hi))
        return out

    def _covered(self, rect, rot: int, ax: int, ay: int, view=None):
        """(first column, last column, {halves}) of the holes inside a rect placed with pin 1 at (ax, ay), on the
        board a view is of (the one placed on, by default), or None if no hole there is."""
        if rect is None:
            return None
        shift, dx, vlo, vhi = view or (0, 0, self._lo, self._hi)
        x0, y0, x1, y1 = self._box(rect, rot, ax, ay)
        lo, hi = max(vlo, math.ceil(x0 + dx + 1e-6)), min(vhi, math.floor(x1 + dx - 1e-6))
        rows = [y for y in ROW_Y.values() if y0 < y + shift < y1]
        if lo > hi or not rows:
            return None
        return lo, hi, {"top" if y < 8 else "bot" for y in rows}

    def _choices(self, part) -> list:
        return [o for board, _ in self.boards for o in self._choices_on(part, board)]

    def _choices_on(self, part, board: Board) -> list:
        cols = range(1, board.columns + 1)
        if part.kind == "pico":
            # centred on the trench, or a row off it either way: more room on one side, if that's where it's needed
            spots = [(r, c, 0) for r in "ghi" for c in cols] + [(r, c, 180) for r in "bcd" for c in cols]
        elif part.kind == "dip" and part.width == 3:
            spots = [("f", c, 0) for c in cols] + [("e", c, 180) for c in cols]
        elif part.kind == "dip":  # wider: wherever its rows straddle the trench (a 0.6 in DIP: b and f, ... e and i)
            spots = [(r, c, rot) for r in ROW_Y for c in cols for rot in (0, 180)]
        elif part.kind == "kicad":
            spots = [(r, c, rot) for r in ROW_Y for c in cols for rot in (0, 90, 180, 270)]
        else:  # sip
            spots = [(r, c, rot) for r in ROW_Y for c in cols for rot in (0, 180)]
        out = [o for o in (self._layout(part, *s, board) for s in spots) if o]
        if part.kind == "dip" and part.width != 3:
            # straddling the trench (one row each side, or opposite pins would share a strip), and with a hole
            # free in every pin's strip for its wire: a 1.0 in part covers one side's strips whole on one board
            out = [o for o in out if len({y < 8 for _, y in o[2].values()}) == 2
                   and all(o[6].get(("top" if y < 8 else "bot", x), 0) < 5 for x, y in o[2].values())]
            if not out:
                raise PartError(f"parts.{part.ref}: a {part.width / 10:g} in DIP can't straddle the trench with a "
                                "free hole by each pin; give it at:")
        if part.kind == "kicad":  # straddling the trench: centred, or at most a row off (wide parts only)
            def room(lay):
                return min((y - 3) if y < 8 else (14 - y) for _, y in lay[2].values())
            best = {}
            for o in out:
                straddles = len({y < 8 for _, y in o[2].values()}) > 1
                best[o[1], straddles] = max(best.get((o[1], straddles), -1), room(o) if straddles else 0)
            out = [o for o in out if len({y < 8 for _, y in o[2].values()}) == 1
                   or room(o) >= best[o[1], True] - 1]
        return out

    def x_of(self, at: str) -> int:
        """Where along the boards a layout's pin 1 is ("2:f10" is column 10 of board 2)."""
        cache = self.__dict__.setdefault("_x_of", {})
        if at not in cache:
            board, _, col = parse_hole(at)
            cache[at] = self.offset.get(str(board) if board else self.board.id, 0) + col
        return cache[at]

    def cost(self, state: dict) -> float:
        layouts = {**self.fixed, **state}
        spans = [s for lay in layouts.values() for s in lay[3]]
        cost = 0.0
        for i, (h1, a1, b1) in enumerate(spans):  # one free column between parts, three to breathe
            for h2, a2, b2 in spans[i + 1:]:
                if h1 == h2:
                    gap = max(a1, a2) - min(b1, b2) - 1
                    if gap < 1:
                        return math.inf
                    cost += SPACE_COST * max(0, 3 - gap)
        for ref, lay in layouts.items():  # nothing in another part's keep-out (a USB cable's plug)
            for h1, a1, b1 in lay[4]:
                cost += KEEP_COST * (b1 - a1 + 1)
                for other, lay2 in layouts.items():
                    if other != ref and any(h1 == h2 and a1 <= b2 and a2 <= b1 for h2, a2, b2 in lay2[3]):
                        return math.inf
        where = {pin: xy for lay in layouts.values() for pin, xy in lay[2].items()}
        for pin, (x, y) in where.items():  # a pin in a net wants free rows in its strip, out toward the rail
            if pin in self.net_of and y in ROW_Y.values():
                room = (y - ROW_Y["a"]) if y < 8 else (ROW_Y["j"] - y)
                cost += ROOM_COST * max(0, 4 - room)
        crowd: dict = {}
        for name, pins in self.nets.items():
            pts = [where[p] for p in pins if p in where]
            if not pts:
                continue
            rails = self.to_rail.get(name)
            if rails:
                for x, y in pts:
                    cost += RAIL_COST * min(abs(y - self.rail_y[r]) for r in rails)
                    cost += GAP_COST * (x in self.gaps)
            if name not in self.plan:
                xs, ys = [x for x, _ in pts], [y for _, y in pts]
                cost += (max(xs) - min(xs)) + (max(ys) - min(ys))
                for half in ("top", "bot"):
                    hx = [x for x, y in pts if (y < 8) == (half == "top")]
                    if len(hx) > 1:
                        for x in range(min(hx), max(hx) + 1):
                            crowd[half, x] = crowd.get((half, x), 0) + 1
            if name in self.offboard:
                cost += OFFBOARD_COST * (self.width - max(x for x, _ in pts))
            if name not in self.plan and len(pts) > 1:
                lo, hi = min(x for x, _ in pts), max(x for x, _ in pts)
                for ref, lay in layouts.items():
                    walls = [w for w in lay[5] if lo < w[1] and w[2] < hi]
                    if walls and not any(self.net_of.get(p) == name for p in lay[2]):
                        cost += WALL_COST if len(walls) > 1 else HALF_WALL_COST
        taken: dict = {}
        for lay in layouts.values():
            for key, n in lay[6].items():
                taken[key] = taken.get(key, 0) + n
        # a signal wire runs past a column in a row of its own: past a part, only in the rows its pins and body
        # leave (two beside a Pico, four beside a DIP), less one for a wire ending there
        cost += CROWD_COST * sum(max(0, n - max(1, 4 - taken.get(key, 0))) for key, n in crowd.items())
        for a, b in self.links:
            pa = [where[p] for p in self.nets[a] if p in where]
            pb = [where[p] for p in self.nets[b] if p in where]
            if pa and pb and a not in self.plan and b not in self.plan:
                cost += LEG_LINK_COST * min(abs(x1 - x2) + abs(y1 - y2) for x1, y1 in pa for x2, y2 in pb)
            for x, y in ((a, b), (b, a)):  # a part from a net's pins to a rail spans the rows between
                pins = [where[p] for p in self.nets[x] if p in where]
                if y in self.plan and x not in self.plan and pins:
                    cost += LEG_LINK_COST * min(abs(py - self.rail_y[r]) for _, py in pins for r in self.plan[y])
        # room for the two-lead parts between two signal nets: each wants a strip nothing is in, near its nets'
        # pins, for a leg of its own (a wire then joins it to its net); one with nowhere to go ends up lying
        # across whatever is between its nets' strips, a chip as like as not
        claimed: set = set()
        for a, b in self.links:
            if a in self.plan or b in self.plan:
                continue  # a leg on a rail's net goes on the rail
            xs = {x for x, _ in (where[p] for p in self.nets[a] + self.nets[b] if p in where)}
            if not xs:
                continue
            spot = next((key for d in range(ROOM_REACH + 1) for x in sorted(xs) for k in {x - d, x + d}
                         for key in (("top", k), ("bot", k))
                         if key in self.strips and key not in claimed and not taken.get(key)), None)
            if spot is None:
                cost += ROOMLESS_COST
            else:
                claimed.add(spot)
        return cost

    def anneal(self, seed: int) -> list[tuple[float, dict]]:
        rng = random.Random(seed)
        state = {}

        for ref in self.auto:  # a valid start, part by part
            choices = self.choices[ref][:]
            rng.shuffle(choices)
            state[ref] = next((ch for ch in choices if self.cost({**state, ref: ch}) < math.inf), choices[0])
        if self.cost(state) == math.inf:  # no room left for some part: pack them from one end, widest first
            state = {}
            for ref in sorted(self.auto, key=lambda r: max(hi - lo for _, lo, hi in self.choices[r][0][3]),
                              reverse=True):
                choices = sorted(self.choices[ref], key=lambda ch: min(lo for _, lo, _ in ch[3]) * (-1) ** seed)
                state[ref] = next((ch for ch in choices if self.cost({**state, ref: ch}) < math.inf), choices[0])
        state = {ref: state[ref] for ref in self.auto}
        cost = self.cost(state)
        seen: dict = {_frozen(state): (cost, state)} if cost < math.inf else {}
        for step in range(STEPS):
            temp = 30.0 * (1 - step / STEPS) + 0.05
            ref = rng.choice(self.auto)
            choices = self.choices[ref]
            if rng.random() < 0.7:  # nudge it along the board
                at, rot = state[ref][0], state[ref][1]
                near = [ch for ch in choices if abs(self.x_of(ch[0]) - self.x_of(at)) <= 6]
                new = {**state, ref: rng.choice(near or choices)}
            elif len(self.auto) > 1 and rng.random() < 0.5:  # swap two parts' columns
                other = rng.choice([r for r in self.auto if r != ref])
                new = dict(state)
                new[ref] = min(choices, key=lambda ch: abs(self.x_of(ch[0]) - self.x_of(state[other][0])))
                new[other] = min(self.choices[other], key=lambda ch: abs(self.x_of(ch[0]) - self.x_of(state[ref][0])))
            else:
                new = {**state, ref: rng.choice(choices)}
            c2 = self.cost(new)
            if c2 < cost or (c2 < math.inf and rng.random() < math.exp((cost - c2) / temp)):
                state, cost = new, c2
                seen[_frozen(state)] = (cost, state)
        return sorted(seen.values(), key=lambda t: t[0])[:KEEP]


def _frozen(state: dict):
    return tuple(sorted((r, lay[0], lay[1]) for r, lay in state.items()))


def _score(c, result) -> float:
    """How good a built layout is: problems first, then anything slanted or overlong, then wire length."""
    score = 1000.0 * len(result.errors) + 15.0 * len(result.warnings) + 2.0 * len(c.wires)
    for w in c.wires:
        if w.a and w.b and w.flex:  # tidier than a slant, but it hides what it arches over
            score += FLEX_WEIGHT + math.dist(c.end_xy(w.a), c.end_xy(w.b))
        elif w.a and w.b and w.a.hole and w.b.hole and w.a.hole.is_rail and w.b.hole.is_rail and not w.via:
            score += RAIL_LINK_WEIGHT  # two rails joined: board setup, out of the way at their ends
        elif w.a and w.b and w.a.hole and w.b.hole:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            score += sum(math.dist(p, q) for p, q in zip(pts, pts[1:]))
            score += 10.0 * sum(not _aligned(p, q) for p, q in zip(pts, pts[1:]))
        elif w.a and w.b:  # a flying lead: shorter is tidier, and it isn't drawn across the board
            score += FLYING_WEIGHT * math.dist(c.end_xy(w.a), c.end_xy(w.b))
    score += KEEP_WEIGHT * c.cable_holes()
    score += CROSS_WEIGHT * crossings(c)
    rounds = [((part.pins[0].xy[0] + part.pins[1].xy[0]) / 2, (part.pins[0].xy[1] + part.pins[1].xy[1]) / 2)
              for part in c.parts.values() if part.kind in ROUND and all(p.xy for p in part.pins)]
    score += ROUND_COST * sum(1 for i, a in enumerate(rounds) for b in rounds[i + 1:] if math.dist(a, b) < ROUND_GAP)
    middles = []
    for part in c.parts.values():
        if part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
            a, b = part.pins[0].xy, part.pins[1].xy
            score += LEG_SLANT_COST * (not _aligned(a, b)) + 3.0 * max(0.0, math.dist(a, b) - SPAN[part.kind][1])
            # lying over a part's body: it has to come out before the part can, and hides its pins
            score += LEG_OVER_COST * sum(1 for r in c._bodies_crossed(a, b) if r != part.ref and r not in c._keepouts)
            middles.append(((a[0] + b[0]) / 2, (a[1] + b[1]) / 2))
    for i, (x1, y1) in enumerate(middles):  # two parts this close: their labels collide
        score += LABEL_COST * sum(abs(x1 - x2) < 2.5 and abs(y1 - y2) < 1.5 for x2, y2 in middles[i + 1:])
    return score


TURN_ALL = 4          # up to this many footprint parts, every combination of them turned round is built
REPAIR_BUILDS = 120   # full builds the repair search may spend
LOOKAHEAD = 60.0      # a chip move scoring within this of the best gets its two-lead parts polished before judging
LOOKAHEAD_BUILDS = 12 # ... in at most this many builds
LOOKAHEAD_TIMES = 6   # ... and at most this many times a search
PATIENCE = 40         # builds in a row without an improvement before it stops


def autoplace(data: dict, source: str) -> dict:
    """The file with every part bbgen has to place placed: `at` and `rot` for footprint parts, `pins` for
    two-lead parts. Annealing on the estimate picks footprint placements to build; the best build is then
    repaired move by move, each move built and scored for real."""
    try:
        model = _Model(data)
    except (PartError, ValueError) as e:
        raise CircuitError(str(e)) from None
    leads = [r for r, p in model.parts.items() if p.kind in LEAD_KINDS and not p.legs]
    base = copy.deepcopy(data)
    base.setdefault("autowire", True)  # placed by bbgen, so wired by it unless told otherwise
    base["_placed"] = [*model.auto, *leads]
    # two supplies picking their own + rails: which takes the top one changes where parts want to go, so
    # both ways round compete
    orders = [model.free_supplies]
    if len(model.free_supplies) > 1:
        orders.append(model.free_supplies[::-1])
    models, bases = [], []
    for order in orders:
        bases.append({**base, "_rail_order": order} if len(orders) > 1 else base)
        models.append(model if not models else _Model(bases[-1]))
    candidates: dict = {}
    for k, m in enumerate(models):
        if m.auto:
            for seed in range(SEEDS):
                for cost, state in m.anneal(seed):
                    candidates.setdefault((k, _frozen(state)), (cost, state, k))
        else:
            candidates[(k, ())] = (0.0, {}, k)
    best = None
    starts = sorted(candidates.values(), key=lambda t: t[0])[:KEEP * 2]
    trials = [((i, k), _with(bases[k], {ref: {"at": at, "rot": rot} for ref, (at, rot, *_) in state.items()}))
              for i, (_, state, k) in enumerate(starts, 1)]
    for (i, k), trial, score, c in _scored(trials, source):
        kept = best is None or score < best[0]
        trace.step(c, f"starting layout {i} of {len(starts)}: score {score:.0f}" + (" ✓ best so far" if kept else ""),
                   kept=kept)
        if kept:
            best, model = (score, trial, c), models[k]
    if best is None:
        raise CircuitError("bbgen found no way to fit these parts on the board without overlapping; place the "
                           "biggest yourself (at:), or use more boards")
    best = (best[0], best[1], _built(best[1], best[2], source))
    best = _turned(model, best, source)
    return _repair(model, leads, best, source)


def _flip(model, data: dict, ref: str):
    """The layout with a footprint part turned round where it is (the same holes), or None."""
    spec = data["parts"][ref]
    now = next(ch for ch in model.choices[ref] if (ch[0], ch[1]) == (spec["at"], spec["rot"]))
    return next((ch for ch in model.choices[ref] if ch is not now and ch[1] != now[1] and ch[3] == now[3]), None)


def _turned(model, best, source: str):
    """The best layout with its footprint parts turned round where they are, in every combination (single
    turns and pairs past TURN_ALL parts). The estimate can't tell which way round a chip reads best, and
    turning one of two chips that talk to each other often helps only with the other turned too; built
    fresh, the layouts compete fairly before the repair search refines the winner."""
    score, data, c = best
    flips = {ref: ch for ref in model.auto if (ch := _flip(model, data, ref))}
    refs = sorted(flips)
    if len(refs) <= TURN_ALL:
        combos = [combo for n in range(1, len(refs) + 1) for combo in itertools.combinations(refs, n)]
    else:
        combos = [combo for n in (1, 2) for combo in itertools.combinations(refs, n)]
    start = 0
    while start < len(combos):  # each turn tried on the layout as it stands, the turns kept so far in it
        here = data
        trials = ((j, _with(here, {ref: {"at": flips[ref][0], "rot": flips[ref][1]} for ref in combos[j]}))
                  for j in range(start, len(combos)))
        start = len(combos)
        for j, trial, s2, c2 in _scored(trials, source):
            kept = s2 < score - 1e-9
            trace.step(c2, f"turn {' and '.join(combos[j])}: score {score:.0f} → {s2:.0f} {'✓' if kept else '✗'}",
                       kept=kept, focus=combos[j])
            if kept:
                score, data, c = s2, trial, _built(trial, c2, source)
                start = j + 1
                break
    return score, data, c


def _with(data: dict, changes: dict) -> dict:
    """A copy of data with some parts' fields changed (a field set to None is removed)."""
    out = dict(data)
    out["parts"] = dict(data["parts"])
    for ref, fields in changes.items():
        spec = {**out["parts"][ref], **fields}
        out["parts"][ref] = {k: v for k, v in spec.items() if v is not None}
    return out


def _evaluate(data: dict, source: str):
    from .check import check
    c = Circuit(data, source)
    return _score(c, check(c)), c


# ---- building trial layouts side by side ----
#
# The search builds a trial layout for every move it weighs, and keeps the first that scores better, in a fixed
# order. Trials don't depend on one another, so a few worker processes build the next several at once; the
# search still takes them in order and stops at the same one, so the layout it ends with is the same as when
# built one at a time. Only the score comes back from a worker; the search builds a trial again itself when it
# needs the circuit (one it keeps, or one it polishes). BBGEN_JOBS=1 builds one at a time; so does a recording
# (its frames come from the circuits), and a worker never starts workers of its own.

JOBS = max(1, int(os.environ.get("BBGEN_JOBS") or os.cpu_count() or 1))
_pool_ = None
_in_worker = False


def _worker_start() -> None:
    global _in_worker, _pool_
    _in_worker, _pool_ = True, None


def _worker_score(data: dict, source: str) -> float:
    return _evaluate(data, source)[0]


def _parallel() -> bool:
    return (JOBS > 1 and not _in_worker and not trace.active()
            and "fork" in multiprocessing.get_all_start_methods())


def _pool():
    global _pool_
    if _pool_ is None:
        from concurrent.futures import ProcessPoolExecutor
        _pool_ = ProcessPoolExecutor(JOBS, mp_context=multiprocessing.get_context("fork"), initializer=_worker_start)
    return _pool_


def _scored(trials, source: str):
    """(item, trial, score, circuit or None) for each (item, trial) in order, the next few built side by side."""
    it = iter(trials)
    if not _parallel():
        for item, trial in it:
            score, c = _evaluate(trial, source)
            yield item, trial, score, c
        return
    pool = _pool()
    while True:
        batch = list(itertools.islice(it, JOBS))
        if not batch:
            return
        futures = [pool.submit(_worker_score, trial, source) for _, trial in batch]
        try:
            for (item, trial), future in zip(batch, futures):
                yield item, trial, future.result(), None
        finally:
            for future in futures:
                future.cancel()  # the search stopped short: the rest aren't wanted


def _built(trial: dict, c, source: str):
    """The trial's circuit: the one built in this process, or built again here (a worker's wasn't sent back)."""
    return c if c is not None else _evaluate(trial, source)[1]


def _pinned(c, leads: list[str]) -> dict:
    """Changes that write down where bbgen put each two-lead part's legs."""
    return {ref: {"pins": [c.hole_name(p.hole) for p in c.parts[ref].pins]} for ref in leads
            if all(p.hole for p in c.parts[ref].pins)}


def _trouble(c, result) -> list[str]:
    """Parts named in the warnings, or owning a net a warned wire or a flexible jumper carries, most mentioned
    first."""
    import re
    from collections import Counter
    count: Counter = Counter()
    nets = {pin: net.name for net in c.nets.values() for pin in net.pins}
    for issue in result.warnings:
        for word in re.findall(r"[A-Za-z_][\w-]*", issue.message):
            if word in c.parts:
                count[word] += 2
            elif re.fullmatch(r"W\d+", word):
                w = c.wires[int(word[1:]) - 1] if int(word[1:]) <= len(c.wires) else None
                if w:
                    for part in c.parts.values():
                        if any(nets.get(p) == w.group for p in part.pins):
                            count[part.ref] += 1
    for w in c.wires:
        if w.flex:
            for part in c.parts.values():
                if any(nets.get(p) == w.group for p in part.pins):
                    count[part.ref] += 1
    return [ref for ref, _ in count.most_common()]


def _moves(model: _Model, leads: list[str], c, result, data: dict):
    """Changes to try, the parts in trouble first: each footprint part turned round where it is (cheap to
    try, and the estimate can't tell which way round reads best), then another leg pair for a two-lead part
    (the others staying where they are), then a footprint part shifted or moved (the two-lead parts placed
    afresh)."""
    order = _trouble(c, result)
    rank = {ref: i for i, ref in enumerate(order)}
    pinned = _pinned(c, leads)
    unpinned = {ref: {"pins": None} for ref in leads}
    state = {ref: next(ch for ch in model.choices[ref]
                       if (ch[0], ch[1]) == (data["parts"][ref]["at"], data["parts"][ref]["rot"]))
             for ref in model.auto}
    chips = sorted(model.auto, key=lambda r: rank.get(r, len(rank)))
    flipped = {}
    for ref in chips:
        flips = [ch for ch in model.choices[ref]
                 if ch is not state[ref] and ch[1] != state[ref][1] and ch[3] == state[ref][3]]
        if flips:
            flipped[ref] = flips[0]
            yield {**unpinned, ref: {"at": flips[0][0], "rot": flips[0][1]}}, {}
    shifted = _shifted(model, data, state)  # every chip a row up (or down) together: room on one side for wires
    for change in shifted:
        yield {**unpinned, **change}, {}
    for ref in sorted(leads, key=lambda r: rank.get(r, len(rank))):
        for k in range(4):
            yield {**{r: ch for r, ch in pinned.items() if r != ref}, ref: {"pins": None}}, {ref: k}
    for ref in chips:
        spec = data["parts"][ref]
        _, row, _ = parse_hole(spec["at"])
        col = model.x_of(spec["at"])
        layouts = [ch for ch in model.choices[ref] if ch is not state[ref] and ch is not flipped.get(ref)]

        def near(ch):
            return abs(model.x_of(ch[0]) - col) + (parse_hole(ch[0])[1] != row) * 2 + (ch[1] != spec["rot"]) * 3
        # a few nudges, then the places the estimate likes best with everything else where it is
        scored = sorted(layouts, key=lambda ch: model.cost({**state, ref: ch}))
        tries = sorted(layouts, key=near)[:4]
        tries += [ch for ch in scored[:6] if model.cost({**state, ref: ch}) < math.inf]
        for ch in {id(ch): ch for ch in tries}.values():
            yield {**unpinned, ref: {"at": ch[0], "rot": ch[1]}}, {}


def _describe(model, data: dict, changes: dict, ranks: dict) -> tuple[str, list[str]]:
    """What a repair move does, in words, and the parts it moves."""
    if ranks:
        [(ref, k)] = ranks.items()
        return f"{ref} into its {('best', '2nd best', '3rd best', '4th best')[k]} legs", [ref]
    moved = [ref for ref, ch in changes.items() if "at" in ch]
    if len(moved) > 1:
        return f"move {' and '.join(moved)} a row", moved
    for ref, ch in changes.items():
        if "at" in ch:
            flip = _flip(model, data, ref)
            if flip is not None and (flip[0], flip[1]) == (ch["at"], ch["rot"]):
                return f"turn {ref}", [ref]
            return f"move {ref} from {data['parts'][ref]['at']} to {ch['at']}", [ref]
    return "re-place the two-lead parts", []


def _shifted(model, data: dict, state: dict) -> list[dict]:
    """The layout with every footprint part bbgen places moved a row up, or a row down, where they all can go."""
    out = []
    for dr in (1, -1):
        change = {}
        for ref, now in state.items():
            board, row, col = parse_hole(now[0])
            at = f"{board}:" * bool(board) + chr(ord(row) + dr) + str(col)
            ch = next((c for c in model.choices[ref] if (c[0], c[1]) == (at, now[1])), None)
            if ch is None:
                break
            change[ref] = {"at": ch[0], "rot": ch[1]}
        else:
            if change and model.cost({**state, **{r: next(c for c in model.choices[r] if (c[0], c[1]) ==
                                                           (v["at"], v["rot"])) for r, v in change.items()}}) < math.inf:
                out.append(change)
    return out


def _polish(leads: list[str], data: dict, score: float, c, source: str, budget: int):
    """A few leg-pair moves on a fresh layout, greedily: (score, data, circuit, builds spent)."""
    spent = 0
    data = _with(data, _pinned(c, leads))
    improved = True
    while improved and spent < budget:
        improved = False
        from .check import check
        rank = {ref: i for i, ref in enumerate(_trouble(c, check(c)))}
        here = data

        def trials():
            for ref in sorted(leads, key=lambda r: rank.get(r, len(rank)))[:4]:
                for k in (1, 2):
                    yield ref, {**_with(here, {ref: {"pins": None}}), "_leg_rank": {ref: k}}
        if spent >= budget:
            break
        for _, trial, s2, c2 in _scored(itertools.islice(trials(), budget - spent), source):
            spent += 1
            if s2 < score - 1e-9:
                c2 = _built(trial, c2, source)
                score, c = s2, c2
                data = _with(trial, _pinned(c2, leads))
                data.pop("_leg_rank", None)
                improved = True
                break
    return score, data, c, spent


def _repair(model: _Model, leads: list[str], best, source: str) -> dict:
    from .check import check
    score, data, c = best
    builds = since = polishes = 0
    improved = True
    while improved and builds < REPAIR_BUILDS and since < PATIENCE:
        improved = False
        here = data
        trials = (((changes, ranks), {**_with(here, changes), "_leg_rank": ranks})
                  for changes, ranks in _moves(model, leads, c, check(c), data))
        for (changes, ranks), trial, s2, c2 in _scored(trials, source):
            builds += 1
            since += 1
            if not ranks and score <= s2 < score + LOOKAHEAD and polishes < LOOKAHEAD_TIMES:
                # a chip moved, its two-lead parts placed afresh: they deserve a polish before it's judged (on a
                # budget of its own, so the search itself keeps its builds)
                c2 = _built(trial, c2, source)
                s3, d3, c3, _ = _polish(leads, trial, s2, c2, source, LOOKAHEAD_BUILDS)
                polishes += 1
                if s3 < s2:
                    s2, c2, trial = s3, c3, d3
                what, refs = _describe(model, data, changes, ranks)
                kept = s2 < score - 1e-9
                trace.step(c2, f"repair {builds}: {what}: score {score:.0f} → {s2:.0f} {'✓' if kept else '✗'}",
                           kept=kept, focus=refs)
            if s2 < score - 1e-9:
                since = 0
                # keep it, with every leg written down so the next move starts from exactly this
                c2 = _built(trial, c2, source)
                score, c = s2, c2
                data = _with(trial, _pinned(c2, leads))
                data.pop("_leg_rank", None)
                improved = True
                break
            if builds >= REPAIR_BUILDS or since >= PATIENCE:
                break
    if leads and "_leg_rank" not in data:
        data = _with(data, _pinned(c, leads))
    return data
