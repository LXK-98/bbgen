"""Placing what the file leaves unplaced: footprint parts without `at`, two-lead parts without `pins`.

Footprint parts are placed before the circuit is built. A quick estimate scores a placement: how
far each net's pins are apart, how far pins on a rail's net (or a two-lead part's trip to a rail)
are from that rail and whether their column has rail holes at all, how many signal wires would
run side by side past one column, and how close parts sit. Annealing from a few fixed seeds finds
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
import math
import random

from .board import RAILS, ROW_Y, Board, Hole, parse_hole
from .circuit import _GROUND, _SUPPLY, Circuit, CircuitError, Issue, _aligned, _key, _line
from .parts import FOOTPRINT_KINDS, LEAD_KINDS, PartError, make_part, rotate

# the estimate, for footprint parts
GAP_COST = 8.0         # a rail's pin in a column with no rail holes: its drop will slant
RAIL_COST = 0.5        # per row between a rail's pin and the rail
CROWD_COST = 6.0       # per signal wire past what a column has room for
LINK_COST = 1.0        # per hole between the nets a two-lead part joins
OFFBOARD_COST = 0.15   # per column between an off-board pin's net and the board's edge
SPACE_COST = 4.0       # per column short of three between two footprint parts
KEEP_COST = 0.2        # per column of keep-out on the board (a USB cable is best off its edge)
SEEDS, STEPS, KEEP = 4, 3000, 4  # annealing runs, steps in each, placements kept from each

# two-lead parts' legs
JUNCTION_COST = 6.0    # a leg in an empty strip of its own, which then needs a wire to its net
CROWD_LEG_COST = 1.5   # per hole in use right next to a leg

# a built layout's score
FLYING_WEIGHT = 0.3    # per hole of flying lead
LABEL_COST = 8.0       # per pair of two-lead parts close enough for their labels to collide

# legs: (shortest, longest) natural span in holes
SPAN = {"resistor": (2, 5), "diode": (3, 5), "capacitor": (1, 3), "electrolytic": (1, 2), "led": (1, 3)}


# ---- which nets run on which rails ----

def rail_plan(nets: dict, leg_rails: dict) -> dict[str, list[str]]:
    """{net: [rail, ...]} from each net's `rail:`, the rails its two-lead parts' legs are written on, and
    the names: a lone ground net takes the - rails nobody else has, each supply a free + rail."""
    plan = {name: list(dict.fromkeys([*rails, *leg_rails.get(name, [])])) for name, rails in nets.items()}
    taken = {r for rails in plan.values() for r in rails}
    grounds = [n for n in nets if _GROUND.match(n) and not plan[n]]
    if len(grounds) == 1:
        plan[grounds[0]] = [r for r in ("top-", "bot-") if r not in taken]
        taken |= set(plan[grounds[0]])
    for name in nets:
        if _SUPPLY.match(name) and not plan[name]:
            free = [r for r in ("top+", "bot+") if r not in taken]
            if free:
                plan[name] = free[:1]
                taken.add(free[0])
    return {name: rails for name, rails in plan.items() if rails}


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

def place_legs(c, parts: list) -> None:
    """Choose holes for each two-lead part that has no `pins`, then place it as if they were written."""
    net_of = {pin: net.name for net in c.nets.values() for pin in net.pins}
    plan = rail_plan({n: [r.split(":")[-1] for r in net.rails] for n, net in c.nets.items()},
                     _leg_rails(c.parts, net_of.get))
    board = c.boards[c.default_board]
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
        if not all(options):
            c.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: no free hole for a leg; give its pins"))
            continue
        legs = _Legs(c, board, part, nets, [_strips(c, board, n, extra) for n in nets],
                     _home(c, part, nets, plan, served), drops)
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
        if legs.home_pins:  # the pin this cap now serves
            x = (xy(a)[0] + xy(b)[0]) / 2
            served.add(min(legs.home_pins, key=lambda p: abs(p.xy[0] - x)))
        c._place_leads(part)


def _pairs(board: Board, options: list[list[Hole]], reach: float) -> list[tuple]:
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

    def __init__(self, c, board: Board, part, nets: list, own: list, home, drops):
        self.c, self.board, self.part, self.drops = c, board, part, drops
        self.own = own
        self.own_x = [[board.xy(board.strip_holes(s)[0])[0] for s in mine] for mine in own]
        self.lead_x = []
        for net in nets:  # a net that goes off the board: keep its new strips near the connector
            lead = [p for p in c.nets[net].pins if p.part.kind == "offboard"]
            self.lead_x.append(lead[0].xy[0] if lead else None)
        self.home_x = home[0] if home and home[0] is not None else None
        self.home_pins = home[1] if home and home[0] is None else None
        self._xy: dict = {}
        self._free: dict = {}

    def xy(self, hole: Hole):
        if hole not in self._xy:
            self._xy[hole] = self.board.xy(hole)
        return self._xy[hole]

    def free(self, strip) -> list:
        """The strip's free holes that nothing lies over, with their positions' keys."""
        if strip not in self._free:
            self._free[strip] = [(h, _key(self.xy(h))) for h in self.board.strip_holes(strip)
                                 if h not in self.c.occupied and _key(self.xy(h)) not in self.c._covered]
        return self._free[strip]

    def cheap(self, a: Hole, b: Hole, pa, pb) -> float:
        lo, hi = SPAN[self.part.kind]
        span = math.dist(pa, pb)
        cost = 3.0 * max(0.0, lo - span, span - hi) + 10.0 * (not _aligned(pa, pb))
        cost += 8.0 * self.drops.blocked(a, b, pa, pb)  # in the way of pins' own straight drops to their rails
        for xy in (pa, pb):  # room around each leg, so parts and their labels don't pile up
            cost += CROWD_LEG_COST * sum(_key((xy[0] + dx, xy[1] + dy)) in self.c._used
                                         for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1)))
        for i, (leg, xy) in enumerate(((a, pa), (b, pb))):
            if leg.is_rail or self.board.strip(leg) in self.own[i]:
                continue
            if self.own_x[i]:  # an empty strip for a net that has strips: it'll need a wire to them
                cost += JUNCTION_COST + 0.5 * min(abs(xy[0] - x) for x in self.own_x[i])
            if self.lead_x[i] is not None:
                cost += 0.3 * abs(xy[0] - self.lead_x[i])
        x = (pa[0] + pb[0]) / 2
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


def _drops(c, board: Board, plan: dict, net_of: dict) -> list[tuple[Hole, Hole]]:
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
            if rails and board.has(rails[0], pin.hole.col):
                out.append((Hole(board.id, "a" if top else "j", pin.hole.col), Hole(board.id, rails[0], pin.hole.col)))
    return out


def _strips(c, board: Board, net: str, extra: dict) -> set:
    """The strips a net has on the board so far."""
    return {board.strip(p.hole) for p in c.nets[net].pins if p.hole is not None and not p.hole.is_rail} | \
        extra.get(net, set())


class _Drops:
    """Pins' straight drops to their rails, indexed for quick lookups."""

    def __init__(self, board: Board, drops: list[tuple[Hole, Hole]]):
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


def _leg_options(c, board: Board, net: str, plan: dict, extra: dict) -> list[Hole]:
    """Free holes a leg on this net could go in: its strips and its rails."""
    strips = sorted({board.strip(p.hole) for p in c.nets[net].pins if p.hole is not None} | extra.get(net, set()))
    holes = [h for s in strips for h in board.strip_holes(s)]
    holes += [h for h in board.holes() if h.row in plan.get(net, [])]
    return [h for h in holes if h not in c.occupied]


def _empty_near(c, board: Board, mine: list[Hole], others: list[Hole], reach: float) -> list[Hole]:
    """Free holes in empty strips, for a leg whose net has no rail: within reach of the other leg's strips, or
    anywhere if the net has nothing on the board yet. A leg there is joined to its net later by a wire."""
    if any(h.is_rail for h in mine):
        return []
    cols = {h.col for h in others if not h.is_rail}
    if mine and not cols:
        return []
    out = []
    for strip in sorted({board.strip(h) for h in board.holes() if not h.is_rail}):
        if cols and not any(abs(strip[2] - col) <= reach for col in cols):
            continue
        holes = board.strip_holes(strip)
        if not any(h in c.occupied for h in holes):
            out += holes
    return out


def _home(c, part, nets, plan, served):
    """(x, pin) the part should sit near: its `near` part, or for a decoupling cap the nearest pin of its
    supply that has no cap yet."""
    if part.near and part.near in c.parts:
        xs = [p.xy[0] for p in c.parts[part.near].pins if p.xy]
        return (sum(xs) / len(xs), None) if xs else None
    if all(n in plan for n in nets):
        supply = [n for n in nets if not _GROUND.match(n)] or nets
        pins = [p for p in c.nets[supply[0]].pins if p.hole is not None and p.part.kind in FOOTPRINT_KINDS]
        pins = [p for p in pins if p not in served] or pins
        if pins:
            return None, pins  # resolved per candidate: the nearest of these
    return None


# ---- footprint parts ----

class _Model:
    """Just enough of a circuit to score placements quickly."""

    def __init__(self, data: dict):
        board_spec = (data.get("boards") or [{"type": "full"}])[0]
        board_spec = {"type": board_spec} if isinstance(board_spec, str) else board_spec
        self.board = Board(1, board_spec.get("type", "full"), board_spec.get("rails"))
        self.gaps = {c for c in range(1, self.board.columns + 1) if c not in self.board.rail_cols}
        self.parts = {str(ref): make_part(str(ref), spec) for ref, spec in (data.get("parts") or {}).items()}
        self.nets: dict[str, list] = {}
        explicit: dict[str, list[str]] = {}
        for name, spec in (data.get("nets") or {}).items():
            spec = {"pins": spec} if isinstance(spec, list) else spec
            pins = []
            for text in spec.get("pins") or []:
                ref, _, token = str(text).partition(".")
                if ref in self.parts:
                    pins += [p for p in self.parts[ref].find(token) if p not in pins]
            self.nets[str(name)] = pins
            rails = spec.get("rail") or []
            explicit[str(name)] = [str(r).split(":")[-1] for r in ([rails] if isinstance(rails, str) else rails)]
        self.net_of = {pin: name for name, pins in self.nets.items() for pin in pins}
        self.plan = rail_plan(explicit, _leg_rails(self.parts, self.net_of.get))
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
        self.fixed = {}
        for ref, part in self.parts.items():
            if part.kind in FOOTPRINT_KINDS and part.at:
                parsed = parse_hole(str(part.at))
                if parsed and parsed[2] is not None and parsed[1] in ROW_Y:
                    self.fixed[ref] = self._layout(part, parsed[1], parsed[2], part.rot)

    def _layout(self, part, row: str, col: int, rot: int):
        """(at, rot, {pin: (x, y)}, body [(half, lo, hi)], keep-out [(half, lo, hi)]) for pin 1 in row/col, or
        None if a pin misses the holes."""
        ax, ay = col, ROW_Y[row]
        xy = {}
        for pin in part.pins:
            dx, dy = rotate(pin.offset, rot)
            x, y = ax + dx, ay + dy
            if not (1 <= x <= self.board.columns) or y not in ROW_Y.values():
                return None
            xy[pin] = (x, y)
        xs = [x for x, _ in xy.values()]
        halves = {"top" if y < 8 else "bot" for _, y in xy.values()}
        if part.kind != "sip":
            halves = {"top", "bot"}  # a DIP or Pico straddles the trench
        keep = []
        for _, rect in part.keepouts:  # the columns and halves of holes inside it, on the board
            corners = [rotate((x, y), rot) for x in (rect[0], rect[2]) for y in (rect[1], rect[3])]
            x0, x1 = ax + min(x for x, _ in corners), ax + max(x for x, _ in corners)
            y0, y1 = ay + min(y for _, y in corners), ay + max(y for _, y in corners)
            lo, hi = max(1, math.ceil(x0 + 1e-6)), min(self.board.columns, math.floor(x1 - 1e-6))
            rows = [y for y in ROW_Y.values() if y0 < y < y1]
            if lo <= hi and rows:
                keep += [(h, lo, hi) for h in {"top" if y < 8 else "bot" for y in rows}]
        return f"{row}{col}", rot, xy, [(h, min(xs), max(xs)) for h in halves], keep

    def _choices(self, part) -> list:
        cols = range(1, self.board.columns + 1)
        if part.kind == "pico":
            spots = [("h", c, 0) for c in cols] + [("c", c, 180) for c in cols]
        elif part.kind == "dip":
            if part.width != 3:
                raise PartError(f"parts.{part.ref}: bbgen only places 0.3 in DIPs; give it at:")
            spots = [("f", c, 0) for c in cols] + [("e", c, 180) for c in cols]
        else:  # sip
            spots = [(r, c, rot) for r in ROW_Y for c in cols for rot in (0, 180)]
        out = [self._layout(part, *s) for s in spots]
        return [o for o in out if o]

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
                cost += OFFBOARD_COST * (self.board.columns - max(x for x, _ in pts))
        cost += CROWD_COST * sum(max(0, n - 2) for n in crowd.values())
        for a, b in self.links:
            pa = [where[p] for p in self.nets[a] if p in where]
            pb = [where[p] for p in self.nets[b] if p in where]
            if pa and pb and a not in self.plan and b not in self.plan:
                cost += LINK_COST * min(abs(x1 - x2) + abs(y1 - y2) for x1, y1 in pa for x2, y2 in pb)
            for x, y in ((a, b), (b, a)):  # a part from a net's pins to a rail spans the rows between
                pins = [where[p] for p in self.nets[x] if p in where]
                if y in self.plan and x not in self.plan and pins:
                    cost += LINK_COST * min(abs(py - self.rail_y[r]) for _, py in pins for r in self.plan[y])
        return cost

    def anneal(self, seed: int) -> list[tuple[float, dict]]:
        rng = random.Random(seed)
        state = {}
        for ref in self.auto:  # a valid start, part by part
            choices = self.choices[ref][:]
            rng.shuffle(choices)
            state[ref] = next((ch for ch in choices if self.cost({**state, ref: ch}) < math.inf), choices[0])
        cost = self.cost(state)
        seen: dict = {}
        for step in range(STEPS):
            temp = 30.0 * (1 - step / STEPS) + 0.05
            ref = rng.choice(self.auto)
            choices = self.choices[ref]
            if rng.random() < 0.7:  # nudge it along the board
                at, rot = state[ref][0], state[ref][1]
                near = [ch for ch in choices if abs(int(ch[0][1:]) - int(at[1:])) <= 6]
                new = {**state, ref: rng.choice(near or choices)}
            elif len(self.auto) > 1 and rng.random() < 0.5:  # swap two parts' columns
                other = rng.choice([r for r in self.auto if r != ref])
                new = dict(state)
                new[ref] = min(choices, key=lambda ch: abs(int(ch[0][1:]) - int(state[other][0][1:])))
                new[other] = min(self.choices[other], key=lambda ch: abs(int(ch[0][1:]) - int(state[ref][0][1:])))
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
        if w.a and w.b and w.a.hole and w.b.hole:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            score += sum(math.dist(p, q) for p, q in zip(pts, pts[1:]))
            score += 10.0 * sum(not _aligned(p, q) for p, q in zip(pts, pts[1:]))
        elif w.a and w.b:  # a flying lead: shorter is tidier, and it isn't drawn across the board
            score += FLYING_WEIGHT * math.dist(c.end_xy(w.a), c.end_xy(w.b))
    middles = []
    for part in c.parts.values():
        if part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
            a, b = part.pins[0].xy, part.pins[1].xy
            score += 10.0 * (not _aligned(a, b)) + 3.0 * max(0.0, math.dist(a, b) - SPAN[part.kind][1])
            middles.append(((a[0] + b[0]) / 2, (a[1] + b[1]) / 2))
    for i, (x1, y1) in enumerate(middles):  # two parts this close: their labels collide
        score += LABEL_COST * sum(abs(x1 - x2) < 2.5 and abs(y1 - y2) < 1.5 for x2, y2 in middles[i + 1:])
    return score


REPAIR_BUILDS = 120   # full builds the repair search may spend
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
    candidates: dict = {}
    if model.auto:
        for seed in range(SEEDS):
            for cost, state in model.anneal(seed):
                candidates.setdefault(_frozen(state), (cost, state))
    else:
        candidates[()] = (0.0, {})
    best = None
    for _, state in sorted(candidates.values(), key=lambda t: t[0])[:KEEP * 2]:
        trial = _with(base, {ref: {"at": at, "rot": rot} for ref, (at, rot, *_) in state.items()})
        score, c = _evaluate(trial, source)
        if best is None or score < best[0]:
            best = (score, trial, c)
    return _repair(model, leads, best, source)


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


def _pinned(c, leads: list[str]) -> dict:
    """Changes that write down where bbgen put each two-lead part's legs."""
    return {ref: {"pins": [c.hole_name(p.hole) for p in c.parts[ref].pins]} for ref in leads
            if all(p.hole for p in c.parts[ref].pins)}


def _trouble(c, result) -> list[str]:
    """Parts named in the warnings, or owning a net a warned wire carries, most mentioned first."""
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
    return [ref for ref, _ in count.most_common()]


def _moves(model: _Model, leads: list[str], c, result, data: dict):
    """Changes to try, the parts in trouble first: another leg pair for a two-lead part (the others staying
    where they are), or a footprint part shifted, flipped or moved (the two-lead parts placed afresh)."""
    order = _trouble(c, result)
    rank = {ref: i for i, ref in enumerate(order)}
    pinned = _pinned(c, leads)
    for ref in sorted(leads, key=lambda r: rank.get(r, len(rank))):
        for k in range(4):
            yield {**{r: ch for r, ch in pinned.items() if r != ref}, ref: {"pins": None}}, {ref: k}
    unpinned = {ref: {"pins": None} for ref in leads}
    state = {ref: next(ch for ch in model.choices[ref]
                       if (ch[0], ch[1]) == (data["parts"][ref]["at"], data["parts"][ref]["rot"]))
             for ref in model.auto}
    for ref in sorted(model.auto, key=lambda r: rank.get(r, len(rank))):
        spec = data["parts"][ref]
        row, col = spec["at"][0], int(spec["at"][1:])
        layouts = [ch for ch in model.choices[ref] if ch is not state[ref]]

        def near(ch):
            return abs(int(ch[0][1:]) - col) + (ch[0][0] != row) * 2 + (ch[1] != spec["rot"]) * 3
        # turned round where it is, a few nudges, then the places the estimate likes best with everything
        # else where it is
        span = state[ref][3]
        flips = [ch for ch in layouts if ch[1] != spec["rot"] and ch[3] == span]
        scored = sorted(layouts, key=lambda ch: model.cost({**state, ref: ch}))
        tries = flips[:1] + sorted(layouts, key=near)[:4]
        tries += [ch for ch in scored[:6] if model.cost({**state, ref: ch}) < math.inf]
        for ch in {id(ch): ch for ch in tries}.values():
            yield {**unpinned, ref: {"at": ch[0], "rot": ch[1]}}, {}


def _repair(model: _Model, leads: list[str], best, source: str) -> dict:
    from .check import check
    score, data, c = best
    builds = since = 0
    improved = True
    while improved and builds < REPAIR_BUILDS and since < PATIENCE:
        improved = False
        for changes, ranks in _moves(model, leads, c, check(c), data):
            trial = _with(data, changes)
            trial["_leg_rank"] = ranks
            s2, c2 = _evaluate(trial, source)
            builds += 1
            since += 1
            if s2 < score - 1e-9:
                since = 0
                # keep it, with every leg written down so the next move starts from exactly this
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
