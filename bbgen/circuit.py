"""Load a circuit file: boards, parts, wires and nets, with every pin placed.

Problems with the layout itself (a pin in the trench, two things in one hole, a
wire to a pin that doesn't exist) are collected as issues so the drawing still
renders. Only a malformed file raises CircuitError.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field
from itertools import cycle
from pathlib import Path

import yaml

from .board import OUTLINE_Y, RAIL_PAIR_Y, RAILS, ROW_Y, Board, Hole, parse_hole
from .parts import (FOOTPRINT_KINDS, LEAD_KINDS, Part, PartError, Pin, body_rect, make_part, placed_box,
                    rotate, slant_reason)

COLORS = {
    "red": "#d62728", "black": "#262626", "blue": "#1f5fbf", "green": "#2ca02c",
    "yellow": "#f2c80f", "orange": "#ff7f0e", "white": "#f7f7f7", "purple": "#8c4fbf",
    "gray": "#8a8a8a", "grey": "#8a8a8a", "brown": "#8c564b", "pink": "#e377c2",
    "cyan": "#17becf",
}
NET_PALETTE = ("blue", "green", "yellow", "orange", "purple", "white", "brown", "cyan", "pink", "gray")
_GROUND = re.compile(r"^(A|D|P)?(GND|VSS|0V)", re.I)
_SUPPLY = re.compile(r"^(VSYS|VBUS|VIN|5V|12V)$", re.I)
CHAR_W = 0.2  # rough label glyph width in grid units, for laying out off-board labels


def color_hex(color: str) -> str:
    return COLORS.get(color.lower(), color)


class CircuitError(Exception):
    pass


class _Unresolved(Exception):
    pass


@dataclass
class Issue:
    level: str  # "error" or "warning"
    code: str
    message: str
    pins: list = field(default_factory=list)
    holes: list = field(default_factory=list)


@dataclass
class End:
    spec: str
    hole: Hole | None = None
    pin: Pin | None = None  # an off-board pin, reached directly


@dataclass
class Wire:
    num: int
    group: str
    spec: tuple[str, str]
    color: str | None
    via: list[str]
    slant: str | None = None  # why it may lie slanted
    a: End | None = None
    b: End | None = None
    via_xy: list = field(default_factory=list)


@dataclass
class Net:
    name: str
    pins: list[Pin]
    color: str


@dataclass
class _Option:
    """A place one end of a wire or leg could go."""
    hole: Hole | None             # None for a fixed point: an off-board pin or a via
    xy: tuple[float, float]
    near: float = 0.0             # distance from the pin it serves
    pin: Pin | None = None        # an off-board pin
    fixed: bool = False           # a hole written out, claimed already


EPS = 1e-6
# every height a hole sits at, rails included
_LEVELS = sorted({*ROW_Y.values(), *(y for pair in RAIL_PAIR_Y.values() for y in pair)})


def _key(xy) -> tuple[float, float]:
    return round(xy[0], 3), round(xy[1], 3)


def _aligned(a, b) -> bool:
    return abs(a[0] - b[0]) < EPS or abs(a[1] - b[1]) < EPS


def _along(s, t) -> bool:
    """Two straight runs lying on the same line and sharing some length."""
    (a, b), (c, d) = s, t
    for i in (0, 1):  # the axis they run along
        j = 1 - i
        if all(abs(p[j] - a[j]) < EPS for p in (b, c, d)):
            lo = max(min(a[i], b[i]), min(c[i], d[i]))
            hi = min(max(a[i], b[i]), max(c[i], d[i]))
            return hi - lo > EPS
    return False


def _crosses(a, b, box) -> bool:
    """Whether segment a-b passes through the inside of box."""
    x0, y0, x1, y1 = box[0] + 0.01, box[1] + 0.01, box[2] - 0.01, box[3] - 0.01
    t0, t1 = 0.0, 1.0
    dx, dy = b[0] - a[0], b[1] - a[1]
    for p, q in ((-dx, a[0] - x0), (dx, x1 - a[0]), (-dy, a[1] - y0), (dy, y1 - a[1])):
        if abs(p) < EPS:
            if q < 0:
                return False
        elif p < 0:
            t0 = max(t0, q / p)
        else:
            t1 = min(t1, q / p)
    return t1 - t0 > EPS


def _dist(p: tuple[float, float], hint) -> float:
    """Distance from p to a hint whose x or y may be None (meaning: any)."""
    if hint is None:
        return 0.0
    dx = p[0] - hint[0] if hint[0] is not None else 0.0
    dy = p[1] - hint[1] if hint[1] is not None else 0.0
    return math.hypot(dx, dy)


class Circuit:
    def __init__(self, data: dict, source: str = "circuit.yaml"):
        if not isinstance(data, dict):
            raise CircuitError("the file must be a mapping with title, boards, parts, wires and nets")
        self.source = source
        self.title = str(data.get("title") or Path(source).stem)
        self.notes = [str(n) for n in data.get("notes") or []]
        self.issues: list[Issue] = []
        self.occupied: dict[Hole, str] = {}
        self._used: dict[tuple, str] = {}          # the same, by drawing position
        self._segments: list[tuple] = []           # (a, b, who) for each wire run and two-lead part
        self._bodies: list[tuple] = []             # (box, ref) for each footprint part
        self._pin_holes: set[Hole] = set()         # holes a footprint part's pins sit in
        self._load_boards(data.get("boards") or [{"type": "full"}])
        self._load_parts(data.get("parts") or {})
        self._load_wires(data.get("wires") or [])
        self._place()
        self._load_nets(data.get("nets") or {})
        self._find_slants()

    @classmethod
    def load(cls, path) -> Circuit:
        path = Path(path)
        with path.open(encoding="utf-8") as f:
            return cls(yaml.safe_load(f), str(path))

    # ---- loading ----

    def _load_boards(self, specs) -> None:
        self.boards: dict[str, Board] = {}
        left = 0.0
        for i, spec in enumerate(specs):
            spec = {"type": spec} if isinstance(spec, str) else dict(spec)
            try:
                board = Board(spec.get("id", i + 1), spec.get("type", "full"), spec.get("rails"),
                              spec.get("split_rails", False))
            except ValueError as e:
                raise CircuitError(f"boards[{i}]: {e}") from None
            if board.id in self.boards:
                raise CircuitError(f"boards[{i}]: duplicate id {board.id!r}")
            board.x0 = left + 2
            left = board.right + 3
            self.boards[board.id] = board
        self.default_board = next(iter(self.boards))

    def _load_parts(self, specs: dict) -> None:
        self.parts: dict[str, Part] = {}
        for ref, spec in specs.items():
            ref = str(ref)
            if not re.fullmatch(r"[A-Za-z_][\w-]*", ref):
                raise CircuitError(f"parts.{ref}: refs are letters, digits, _ and - (and start with a letter)")
            try:
                self.parts[ref] = make_part(ref, spec)
            except PartError as e:
                raise CircuitError(str(e)) from None

    def _load_wires(self, wires) -> None:
        self.wires: list[Wire] = []
        self.groups: list[str] = []
        groups = wires.items() if isinstance(wires, dict) else [("wires", wires)]
        for group, items in groups:
            self.groups.append(str(group))
            for i, item in enumerate(items or []):
                where = f"wires.{group}[{i}]"
                slant = None
                if isinstance(item, list) and len(item) in (2, 3):
                    a, b, color, via = item[0], item[1], item[2] if len(item) == 3 else None, []
                elif isinstance(item, dict) and "from" in item and "to" in item:
                    a, b, color, via = item["from"], item["to"], item.get("color"), item.get("via") or []
                    try:
                        slant = slant_reason(item, where)
                    except PartError as e:
                        raise CircuitError(str(e)) from None
                else:
                    raise CircuitError(f"{where}: expected [from, to], [from, to, color] or "
                                       "{from, to, color, via, slant}")
                if color is not None and str(color).lower() not in COLORS and not str(color).startswith("#"):
                    raise CircuitError(f"{where}: unknown color {color!r} (use {', '.join(COLORS)} or #rrggbb)")
                self.wires.append(Wire(len(self.wires) + 1, str(group), (str(a), str(b)),
                                       str(color) if color else None, [str(v) for v in via], slant))

    def _load_nets(self, specs: dict) -> None:
        self.nets: dict[str, Net] = {}
        palette = cycle(NET_PALETTE)
        for name, spec in specs.items():
            name = str(name)
            if isinstance(spec, list):
                spec = {"pins": spec}
            if not isinstance(spec, dict) or not isinstance(spec.get("pins"), list):
                raise CircuitError(f"nets.{name}: expected a list of pins or {{pins: [...], color: ...}}")
            pins: list[Pin] = []
            for text in spec["pins"]:
                try:
                    found = self._net_pins(str(text))
                except _Unresolved as e:
                    self.issues.append(Issue("error", "BAD_NET", f"nets.{name}: {e}"))
                    continue
                pins += [p for p in found if p not in pins]
            color = spec.get("color")
            if not color:
                color = "black" if _GROUND.match(name) else "red" if _SUPPLY.match(name) else next(palette)
            self.nets[name] = Net(name, pins, str(color))

    # ---- placement ----

    def _place(self) -> None:
        for part in self.parts.values():
            if part.kind in FOOTPRINT_KINDS:
                self._place_footprint(part)
        self._layout_offboard()
        # Holes written out explicitly are claimed before anything is auto-placed.
        for part in self.parts.values():
            for i, leg in enumerate(part.legs):
                self._reserve(leg, f"{part.ref} leg {i + 1}")
        for w in self.wires:
            for spec in w.spec:
                self._reserve(spec, f"W{w.num}")
        pending = [p for p in self.parts.values() if p.kind in LEAD_KINDS]
        while pending:
            ready = [p for p in pending if not any(self._waits_on_leads(leg) for leg in p.legs)]
            if not ready:
                for part in pending:
                    self.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: its legs refer to two-lead parts "
                                                                "that in turn refer back to it"))
                break
            for part in ready:
                pending.remove(part)
                self._place_leads(part)
        for w in self.wires:
            self._route(w)

    def _place_footprint(self, part: Part) -> None:
        try:
            anchor = self._hole(str(part.at))
        except _Unresolved as e:
            self.issues.append(Issue("error", "BAD_PLACE", f"{part.ref} at {part.at}: {e}"))
            return
        board = self.boards[anchor.board]
        ax, ay = board.xy(anchor)
        for pin in part.pins:
            dx, dy = rotate(pin.offset, part.rot)
            pin.xy = (ax + dx, ay + dy)
            pin.hole = board.hole_at(*pin.xy)
            if pin.hole is None:
                self.issues.append(Issue("error", "OFF_HOLES", f"{pin} lands in the trench or off the board",
                                         pins=[pin]))
            else:
                self._occupy(pin.hole, str(pin))
                self._pin_holes.add(pin.hole)
        x0, y0, x1, y1 = placed_box(part, body_rect(part))
        self._bodies.append(((x0, y0, x1, y1), part.ref))
        own = {p.hole for p in part.pins}
        for hole in board.holes():
            x, y = board.xy(hole)
            if x0 < x < x1 and y0 < y < y1 and hole not in own:
                self._occupy(hole, f"the body of {part.ref}")

    def _layout_offboard(self) -> None:
        right = max(b.right for b in self.boards.values()) + 3
        below_x = min(b.left for b in self.boards.values())
        for part in self.parts.values():
            if part.kind != "offboard":
                continue
            n = len(part.pins)
            cols = max(1, min(part.cols, n))
            rows = -(-n // cols)
            if part.numbering == "row":
                grid = [(k % cols, k // cols) for k in range(n)]
            else:
                grid = [(k // rows, k % rows) for k in range(n)]
            ncols, nrows = max(c for c, _ in grid) + 1, max(r for _, r in grid) + 1
            for pin, (c, r) in zip(part.pins, grid):
                if ncols == 1:
                    pin.label_dir = (1, 0)
                elif ncols == 2:
                    pin.label_dir = (-1, 0) if c == 0 else (1, 0)
                else:
                    pin.label_dir = (0, 1) if nrows == 2 and r == 1 else (0, -1)
            lw = max(len(f"{p.num} {p.name}") for p in part.pins) * CHAR_W + 0.7
            pad_l = lw if ncols == 2 else 0.8
            pad_r = lw if ncols <= 2 else 0.8
            pad_t = lw if ncols > 2 else 0.8
            pad_b = lw if ncols > 2 and nrows == 2 else 0.8
            if part.place == "right":
                ox, oy = right + pad_l, 1.0
                right = ox + ncols - 1 + pad_r + 2
            else:
                ox, oy = below_x + pad_l, OUTLINE_Y[1] + 1.5 + pad_t
                below_x = ox + ncols - 1 + pad_r + 2
            for pin, (c, r) in zip(part.pins, grid):
                pin.xy = (ox + c, oy + r)
            part.box = (ox - 0.6, oy - 0.6, ox + ncols - 0.4, oy + nrows - 0.4)
            part.extent = (ox - pad_l, oy - max(pad_t, 1.6), ox + ncols - 1 + pad_r, oy + nrows - 1 + pad_b)

    def _place_leads(self, part: Part) -> None:
        options: list[list[_Option] | None] = []
        for i, spec in enumerate(part.legs):
            try:
                found = self._options(spec, self._hint(part.legs[1 - i]))
                if any(o.pin is not None for o in found):
                    raise _Unresolved("a leg goes in a breadboard hole, not an off-board pin")
            except _Unresolved as e:
                self.issues.append(Issue("error", "BAD_LEG", f"{part.ref} leg {i + 1} ({spec}): {e}"))
                found = None
            options.append(found)
        try:
            chosen = (self._best(*options) if all(options) else
                      [min(o, key=lambda o: (o.near, o.xy)) if o else None for o in options])
        except _Unresolved as e:
            self.issues.append(Issue("error", "BAD_LEG", f"{part.ref}: {e}"))
            return
        for i, (pin, option) in enumerate(zip(part.pins, chosen)):
            if option is not None:
                pin.hole = self._take(option, f"{part.ref} leg {i + 1}").hole
                pin.xy = option.xy
        if all(p.xy for p in part.pins):
            self._segments.append((part.pins[0].xy, part.pins[1].xy, part.ref))

    def _route(self, w: Wire) -> None:
        sa, sb = w.spec
        who = f"W{w.num}"
        try:
            w.via_xy = [self._via(v) for v in w.via]
            a, b = self._options(sa, self._hint(sb)), self._options(sb, self._hint(sa))
            if w.via_xy:  # each end runs to its nearest bend
                pa = self._best(a, [_Option(None, w.via_xy[0])])[0]
                pb = self._best([o for o in b if o.hole is None or o.hole != pa.hole],
                                [_Option(None, w.via_xy[-1])])[0]
            else:
                pa, pb = self._best(a, b)
            w.a, w.b = self._take(pa, who, sa), self._take(pb, who, sb)
        except _Unresolved as e:
            self.issues.append(Issue("error", "BAD_WIRE", f"W{w.num} ({sa} -> {sb}): {e}"))
            w.a = w.b = None
            return
        pts = [self.end_xy(w.a), *w.via_xy, self.end_xy(w.b)]
        self._segments += [(p, q, who) for p, q in zip(pts, pts[1:])]

    def _best(self, a: list[_Option], b: list[_Option]) -> tuple[_Option, _Option]:
        """The best pair of places for two ends: straight and clear, then over as few part bodies as
        possible, then nearest their pins, then shortest. Flying leads only care about the last two."""
        def key(pair):
            p, q = pair
            flying = p.pin is not None or q.pin is not None
            return (0 if flying else self._shape(p.xy, q.xy)[0],
                    0 if flying else len(self._bodies_crossed(p.xy, q.xy)),
                    round(p.near + q.near, 6), round(math.dist(p.xy, q.xy), 6), p.xy, q.xy)

        pairs = [(p, q) for p in a for q in b if p.hole is None or p.hole != q.hole]
        if not pairs:
            raise _Unresolved("both ends would need the same hole")
        return min(pairs, key=key)

    def _shape(self, a, b) -> tuple[int, list[str]]:
        """How a run from a to b would lie, and what is in its way: 0 straight and clear, 1 straight but
        over holes in use, 2 slanted, 3 straight along another wire or part."""
        if not _aligned(a, b):
            return 2, []
        along = [who for p, q, who in self._segments if _along((a, b), (p, q))]
        if along:
            return 3, along
        over = [self._used[k] for k in self._between(a, b) if k in self._used]
        return (1, over) if over else (0, [])

    def _between(self, a, b) -> list[tuple]:
        """Hole positions strictly between a and b on a straight run."""
        if abs(a[1] - b[1]) < EPS:
            lo, hi = sorted((a[0], b[0]))
            return [_key((x, a[1])) for x in range(math.floor(lo + EPS) + 1, math.ceil(hi - EPS))]
        lo, hi = sorted((a[1], b[1]))
        return [_key((a[0], y)) for y in _LEVELS if lo + EPS < y < hi - EPS]

    def _bodies_crossed(self, a, b) -> list[str]:
        return [ref for box, ref in self._bodies if _crosses(a, b, box)]

    def _take(self, option: _Option, who: str, spec: str = "") -> End:
        if option.pin is not None:
            return End(spec, pin=option.pin)
        if not option.fixed:
            self._occupy(option.hole, who)
        return End(spec, hole=option.hole)

    # ---- slants ----

    def _find_slants(self) -> None:
        """Warn about wires and two-lead parts that lie slanted where they could run straight."""
        for w in self.wires:
            if w.a and w.b and w.a.hole and w.b.hole and not w.via and not w.slant:
                self._slant(f"W{w.num}", w.spec, (w.a.hole, w.b.hole))
        for part in self.parts.values():
            if part.kind in LEAD_KINDS and not part.slant and all(p.hole for p in part.pins):
                self._slant(part.ref, part.legs, tuple(p.hole for p in part.pins))

    def _slant(self, name: str, specs, holes: tuple[Hole, Hole]) -> None:
        if holes[0].board != holes[1].board:
            return
        board = self.boards[holes[0].board]
        if _aligned(board.xy(holes[0]), board.xy(holes[1])):
            return
        route = f"{name} slants ({self.hole_name(holes[0])} → {self.hole_name(holes[1])})"
        accept = f"or accept it with slant: <reason> on {name}"
        strips = [board.strip_holes(board.strip(h)) for h in holes]
        straight = [(p, q) for p in strips[0] for q in strips[1]
                    if p != q and _aligned(board.xy(p), board.xy(q))]
        if not straight:
            rail = [h.is_rail for h in holes]
            if rail[0] != rail[1]:  # a strip in a column the rail has no hole in
                i = rail.index(False)
                ref, _, token = specs[i].partition(".")
                part = self.parts.get(ref) if token else None
                if part is None:
                    fix = "use a column that has rail holes"
                elif part.kind in LEAD_KINDS:  # the strip is wherever that leg was put
                    leg = part.find(token)[0]
                    fix = f"put {ref} leg {leg.num} ({self.hole_name(leg.hole)}) in a column that has rail holes"
                else:
                    fix = f"move {ref}"
                gaps = ", ".join(str(c) for c in range(1, board.columns + 1) if c not in board.rail_cols)
                self.issues.append(Issue(
                    "warning", "DIAGONAL", f"{route}: column {holes[i].col} has no {holes[1 - i].row} hole "
                    f"(the rails have none in columns {gaps}). {fix[0].upper()}{fix[1:]}, {accept}",
                    holes=list(holes)))
            return  # otherwise it crosses the trench between two columns, and can't be straight

        def mine(who: str) -> bool:
            return who == name or who.startswith(f"{name} ")

        blockers: list[str] = []
        usable = []
        for p, q in straight:
            if {p, q} & self._pin_holes:
                continue  # never free for a wire or a leg
            pxy, qxy = board.xy(p), board.xy(q)
            here = [self.occupied[h] for h, now in ((p, holes[0]), (q, holes[1]))
                    if h != now and h in self.occupied]
            here += self._bodies_crossed(pxy, qxy)
            level, way = self._shape(pxy, qxy)
            here += [who for who in way if level in (1, 3)]
            here = [who for who in here if not mine(who)]
            if here:
                blockers += here
            else:
                usable.append((p, q))
        if usable:
            def changes(pair):
                return [(new, now) for new, now in zip(pair, holes) if new != now]
            best = min(usable, key=lambda pair: (len(changes(pair)), math.dist(board.xy(pair[0]), board.xy(pair[1]))))
            fix = " and ".join(f"{self.hole_name(new)} instead of {self.hole_name(now)}" for new, now in changes(best))
            self.issues.append(Issue("warning", "DIAGONAL", f"{route}: {fix} would make it straight",
                                     holes=list(holes)))
            return
        names = []
        for who in blockers:
            who = re.sub(r"^the body of | leg \d$", "", who)
            if who not in names:
                names.append(who)
        what = f" (by {', '.join(names[:5])})" if names else ""
        self.issues.append(Issue("warning", "DIAGONAL", f"{route}: every straight route is blocked{what}. "
                                 f"Move parts to make room, bend it with via:, {accept}", holes=list(holes)))

    # ---- resolving holes and pins ----

    def _board(self, board_id: str | None) -> Board:
        board = self.boards.get(board_id or self.default_board)
        if board is None:
            raise _Unresolved(f"there is no board {board_id!r}")
        return board

    def _hole(self, text: str) -> Hole:
        parsed = parse_hole(text)
        if not parsed or parsed[2] is None:
            raise _Unresolved(f"'{text}' is not a hole (like a12, top+5 or 2:f30)")
        board_id, row, col = parsed
        board = self._board(board_id)
        if not board.has(row, col):
            raise _Unresolved(f"board {board.id} has no hole {row}{col}")
        return Hole(board.id, row, col)

    def _occupy(self, hole: Hole, who: str) -> None:
        if hole in self.occupied:
            name = self.boards[hole.board].name(hole, self.default_board)
            self.issues.append(Issue("error", "HOLE_TAKEN", f"{name}: {who} and {self.occupied[hole]} "
                                                            "both want this hole", holes=[hole]))
        else:
            self.occupied[hole] = who
            self._used[_key(self.boards[hole.board].xy(hole))] = who

    def _reserve(self, spec: str, who: str) -> None:
        parsed = parse_hole(spec)
        if parsed and parsed[2] is not None:
            try:
                self._occupy(self._hole(spec), who)
            except _Unresolved:
                pass  # reported when the leg or wire is resolved

    def _waits_on_leads(self, spec: str) -> bool:
        ref = spec.partition(".")[0]
        part = self.parts.get(ref)
        return bool(part and part.kind in LEAD_KINDS and "." in spec and any(p.hole is None for p in part.pins))

    def _pin(self, spec: str, hint) -> Pin:
        ref, dot, token = spec.partition(".")
        if not dot:
            raise _Unresolved(f"'{spec}' is neither a hole (like a12 or top+5) nor a part pin (like U1.3 or U1.VCC)")
        part = self.parts.get(ref) or next((p for r, p in self.parts.items() if r.lower() == ref.lower()), None)
        if part is None:
            raise _Unresolved(f"there is no part {ref}")
        pins = part.find(token)
        if not pins:
            raise _Unresolved(f"{part.ref} has no pin {token!r}")
        if len(pins) == 1:
            return pins[0]
        bus = part.bus_of(pins[0])
        if bus and all(p in bus for p in pins):  # e.g. Pico.GND: take the one nearest the other end
            placed = [p for p in pins if p.xy]
            return min(placed, key=lambda p: _dist(p.xy, hint)) if placed else pins[0]
        raise _Unresolved(f"{part.ref}.{token} is ambiguous: pins {', '.join(str(p.num) for p in pins)}")

    def _net_pins(self, spec: str) -> list[Pin]:
        ref, dot, token = spec.partition(".")
        part = self.parts.get(ref)
        if not dot or part is None:
            raise _Unresolved(f"'{spec}' is not a part pin (like U1.3 or U1.VCC)")
        pins = part.find(token)
        if not pins:
            raise _Unresolved(f"{ref} has no pin {token!r}")
        out: list[Pin] = []
        for pin in pins:  # a name can cover several pins; a bus pin brings its whole bus
            for p in part.bus_of(pin) or (pin,):
                if p not in out:
                    out.append(p)
        return out

    def _hint(self, spec: str):
        parsed = parse_hole(spec)
        if parsed:
            board_id, row, col = parsed
            board = self.boards.get(board_id or self.default_board)
            if board is None:
                return None
            if col is None:
                return (None, board.y(row)) if row in RAILS else None
            return board.xy(Hole(board.id, row, col)) if board.has(row, col) else None
        try:
            return self._pin(spec, None).xy
        except _Unresolved:
            return None

    def _options(self, spec: str, hint) -> list[_Option]:
        """Every place one end could go: a hole written out, a free hole on a bare rail, a free hole in a
        pin's strip, or an off-board pin."""
        parsed = parse_hole(spec)
        if parsed:
            board_id, row, col = parsed
            if col is not None:
                hole = self._hole(spec)  # claimed by _reserve
                return [_Option(hole, self.boards[hole.board].xy(hole), fixed=True)]
            if row not in RAILS:
                raise _Unresolved(f"'{spec}' needs a column number")
            board = self._board(board_id)
            return self._free(board, [h for h in board.holes() if h.row == row], None, f"the {row} rail")
        pin = self._pin(spec, hint)
        if pin.part.kind == "offboard":
            return [_Option(None, pin.xy, pin=pin)]
        if pin.hole is None:
            raise _Unresolved(f"{pin} is not on the board")
        board = self.boards[pin.hole.board]
        return self._free(board, board.strip_holes(board.strip(pin.hole)), pin.xy, f"the strip of {pin}")

    def _free(self, board: Board, holes: list[Hole], near, what: str) -> list[_Option]:
        free = [_Option(h, board.xy(h), _dist(board.xy(h), near)) for h in holes if h not in self.occupied]
        if not free:
            raise _Unresolved(f"no free hole left in {what}")
        return free

    def _via(self, spec: str) -> tuple[float, float]:
        hole = self._hole(spec)
        return self.boards[hole.board].xy(hole)

    def end_xy(self, end: End) -> tuple[float, float]:
        if end.pin is not None:
            return end.pin.xy
        return self.boards[end.hole.board].xy(end.hole)

    def hole_name(self, hole: Hole) -> str:
        return self.boards[hole.board].name(hole, self.default_board)
