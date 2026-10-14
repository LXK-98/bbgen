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

from .board import OUTLINE_Y, RAIL_PAIR_Y, RAILS, ROW_Y, STACK, Board, Hole, parse_hole
from .parts import (FOOTPRINT_KINDS, LEAD_KINDS, Part, PartError, Pin, body_rect, make_part, placed_box,
                    rotate, slant_reason)

COLORS = {
    "red": "#d62728", "black": "#262626", "blue": "#1f5fbf", "green": "#2ca02c",
    "yellow": "#f2c80f", "orange": "#ff7f0e", "white": "#f7f7f7", "purple": "#8c4fbf",
    "gray": "#8a8a8a", "grey": "#8a8a8a", "brown": "#8c564b", "pink": "#e377c2",
    "cyan": "#17becf",
}
SUPPLY_COLORS = ("red", "orange", "pink", "brown")  # a second or third supply mustn't look like the first
NET_PALETTE = ("blue", "green", "yellow", "orange", "purple", "white", "brown", "cyan", "pink", "gray")
_GROUND = re.compile(r"^(A|D|P)?(GND|VSS|0V)", re.I)
_SUPPLY = re.compile(r"^(VSYS|VBUS|VIN|VCC|VDD|VBAT|\+?\d+V\d*|\+?\d+\.\d+V)$", re.I)  # 5V, 3V3, 3.3V
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
    auto: bool = False        # added by autowire, not written in the file
    flex: bool = False        # a flexible jumper, arching over the board instead of lying on it
    a: End | None = None
    b: End | None = None
    via_xy: list = field(default_factory=list)


@dataclass
class Net:
    name: str
    pins: list[Pin]
    color: str
    rails: list[str] = field(default_factory=list)  # physical rails the file puts it on (top+, 2:bot-)
    polarity: str | None = None  # "+" a supply, "-" a ground (on a rail of that sign), None neither
    guessed: bool = False        # polarity taken from the name, not said in the file


@dataclass
class _Option:
    """A place one end of a wire or leg could go."""
    hole: Hole | None             # None for a fixed point: an off-board pin or a via
    xy: tuple[float, float]
    near: float = 0.0             # distance from the pin it serves
    pin: Pin | None = None        # an off-board pin
    fixed: bool = False           # a hole written out, claimed already
    covered: bool = False         # under a two-lead part's body (set by autowire)
    under: bool = False           # under a raised module: a flat wire may end here, nothing else
    kept: bool = False            # a rail end kept for a supply's flying lead (set by autowire)


EPS = 1e-6
CLEARANCE = 0.25  # room a wire lying on the board keeps from a part's body
ROUND_BODY = {"led": 0.9, "electrolytic": 0.9}  # half the width of a 5 mm LED or can, in holes
DETOUR_REACH = 12  # how many columns past its ends a wire may go to get around a part


def net_polarity(name: str, spec) -> tuple[str | None, list[str], bool]:
    """(polarity, physical rails, guessed) from a net's `rail:`: + or - for a supply or a ground (bbgen picks
    which rail), a rail's name (top+, 2:bot-) to pin it there, none for neither; without one, the name
    decides (GND, VSS: ground; 5V, 3V3, VCC, VSYS...: supply) and the checklist says so."""
    raw = spec.get("rail") if isinstance(spec, dict) else None
    items = [] if raw is None else [raw] if isinstance(raw, str) else raw
    if not isinstance(items, list):
        raise CircuitError(f"nets.{name}: rail is +, -, none, a rail's name (top+, bot-) or a list of names")
    items = [str(r).strip() for r in items]
    if items == ["none"]:
        return None, [], False
    signs = {r[-1] for r in items if r and r[-1] in "+-"}
    if len(signs) > 1:
        raise CircuitError(f"nets.{name}: rail gives both + and - rails; a net is a supply or a ground, not both")
    physical = [r for r in items if r not in ("+", "-")]
    if signs:
        return signs.pop(), physical, False
    if physical:
        return None, physical, False
    guess = "-" if _GROUND.match(name) else "+" if _SUPPLY.match(name) else None
    return guess, [], guess is not None


def check_rail_count(polarity: dict, guessed: set, boards) -> None:
    """Every supply needs a + rail of its own, every ground a - rail: a CircuitError when there are more than
    the boards have."""
    for sign, what in (("+", "positive supplies"), ("-", "grounds")):
        nets = [n for n, p in polarity.items() if p == sign]
        strips = {b.strip(h) + (b.id,) for b in boards for h in b.holes() if h.row.endswith(sign)}
        if len(nets) > len(strips):
            guesses = [n for n in nets if n in guessed]
            note = (f" ({', '.join(guesses)} {'is' if len(guesses) == 1 else 'are'} taken as "
                    f"{'a supply' if sign == '+' else 'a ground'} from the name: say rail: none if not)" if guesses else "")
            raise CircuitError(f"nets: {len(nets)} {what} ({', '.join(nets)}) but the board{'s' * (len(boards) > 1)} "
                               f"{'have' if len(boards) > 1 else 'has'} {len(strips)} {sign} rail"
                               f"{'s' * (len(strips) != 1)}: each needs its own. Split the rails (split_rails: true) "
                               f"or add a board{note}")


def _key(xy) -> tuple[float, float]:
    return round(xy[0], 3), round(xy[1], 3)


def _aligned(a, b) -> bool:
    return abs(a[0] - b[0]) < EPS or abs(a[1] - b[1]) < EPS


def _line(a, b):
    """(line, lo, hi) for a straight run: the row or column it lies on and the span along it; None if slanted."""
    if abs(a[1] - b[1]) < EPS:
        return ("h", round(a[1], 3)), min(a[0], b[0]), max(a[0], b[0])
    if abs(a[0] - b[0]) < EPS:
        return ("v", round(a[0], 3)), min(a[1], b[1]), max(a[1], b[1])
    return None


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


def library_for(data) -> tuple | None:
    """KiCad's library, if a part in the file comes from it (`symbol:` or `footprint:`); `kicad:` names it."""
    parts = data.get("parts") if isinstance(data, dict) else None
    if isinstance(parts, dict) and any(isinstance(s, dict) and (s.get("symbol") or s.get("footprint"))
                                       for s in parts.values()):
        from .kicad import library_roots
        return tuple(library_roots(data.get("kicad")))
    return None


def needs_placing(data) -> bool:
    """Whether a part in the file has no `at` (or two-lead, no `pins`), so bbgen has to place it."""
    parts = data.get("parts") if isinstance(data, dict) else None
    if not isinstance(parts, dict) or data.get("_placed") is not None:
        return False  # autoplace's own trial builds
    def kind(spec):  # as make_part decides it: a part named from KiCad is a footprint part
        return spec.get("type") or ("kicad" if spec.get("symbol") or spec.get("footprint") else None)
    return any(isinstance(spec, dict) and ((kind(spec) in FOOTPRINT_KINDS and not spec.get("at")) or
                                           (kind(spec) in LEAD_KINDS and spec.get("pins") is None))
               for spec in parts.values())


class Circuit:
    def __init__(self, data: dict, source: str = "circuit.yaml"):
        if not isinstance(data, dict):
            raise CircuitError("the file must be a mapping with title, boards, parts, wires and nets")
        self.source = source
        self.title = str(data.get("title") or Path(source).stem)
        self.notes = [str(n) for n in data.get("notes") or []]
        self.flex = data.get("flex", True) is not False  # may autowire use flexible jumpers?
        self.issues: list[Issue] = []
        self.occupied: dict[Hole, str] = {}
        self._used: dict[tuple, str] = {}          # the same, by drawing position
        self._segments: list[tuple] = []           # (a, b, who) for each wire run and two-lead part
        self._lines: dict[tuple, list] = {}        # the straight ones by the line they lie on: (lo, hi, who)
        self._bodies: list[tuple] = []             # (box, ref) for each footprint part
        self._pin_holes: set[Hole] = set()         # holes a footprint part's pins sit in
        self._covered: dict[tuple, str] = {}       # positions under a two-lead part's body -> its ref
        self._keepouts: set[str] = set()           # the names in _bodies that are keep-outs, not bodies
        self._raised: list[tuple] = []             # (box, ref) for each raised module: flat wires go under it
        self._under: dict[tuple, str] = {}         # positions of the holes under a raised module -> its ref
        self._tall: set[str] = set()               # parts standing tall (a pot, a can, an LED): nothing passes over
        self._cables: list[tuple] = []             # (box, board) from a cable's plug to the board's edge
        if needs_placing(data):
            from . import trace
            from .autoplace import autoplace  # it builds trial circuits of its own
            with trace.placing():
                data = autoplace(data, source)
        self._leg_rank: dict = data.get("_leg_rank") or {}
        self.rail_order: list = data.get("_rail_order") or []  # supplies in the order they take + rails  # autoplace's search: take a leg pair other than the best
        self._load_boards(data.get("boards") or [{"type": "full"}])
        self.library = library_for(data)
        self._load_parts(data.get("parts") or {})
        for ref in data.get("_placed") or []:  # parts autoplace placed, not the file
            if ref in self.parts:
                self.parts[ref].auto = True
        self._load_wires(data.get("wires") or [])
        self._load_nets(data.get("nets") or {})
        self._place()
        if data.get("autowire"):
            from .autowire import autowire  # it builds on the check, which imports this module
            autowire(self)
        self._find_slants()
        self._find_over_parts()
        self._find_under()
        self._find_stacking()

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
            if spec.get("below") is not None:  # snapped on under another, long sides together
                above = self.boards.get(str(spec["below"]))
                if above is None:
                    raise CircuitError(f"boards[{i}]: below {spec['below']!r} names no board before it")
                if any(b.y0 == above.y0 + STACK and b.x0 == above.x0 for b in self.boards.values()):
                    raise CircuitError(f"boards[{i}]: board {above.id} already has a board below it")
                board.x0, board.y0 = above.x0, above.y0 + STACK
            else:
                board.x0 = left + 2
                left = board.right + 3
            self.boards[board.id] = board
        self.default_board = next(iter(self.boards))
        # every height a hole sits at, rails included, on every board
        self._levels = sorted({b.y(r) for b in self.boards.values() for r in (*ROW_Y, *RAILS)})

    def _load_parts(self, specs: dict) -> None:
        self.parts: dict[str, Part] = {}
        for ref, spec in specs.items():
            ref = str(ref)
            if not re.fullmatch(r"[A-Za-z_][\w-]*", ref):
                raise CircuitError(f"parts.{ref}: refs are letters, digits, _ and - (and start with a letter)")
            try:
                self.parts[ref] = make_part(ref, spec, self.library)
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
                slant, flex = None, False
                if isinstance(item, list) and len(item) in (2, 3):
                    a, b, color, via = item[0], item[1], item[2] if len(item) == 3 else None, []
                elif isinstance(item, dict) and "from" in item and "to" in item:
                    a, b, color, via = item["from"], item["to"], item.get("color"), item.get("via") or []
                    try:
                        slant = slant_reason(item, where)
                    except PartError as e:
                        raise CircuitError(str(e)) from None
                    flex = item.get("flex", False)
                    if not isinstance(flex, bool):
                        raise CircuitError(f"{where}: flex is true or false")
                    if flex and via:
                        raise CircuitError(f"{where}: a flexible jumper arches straight over; give it no via")
                else:
                    raise CircuitError(f"{where}: expected [from, to], [from, to, color] or "
                                       "{from, to, color, via, slant, flex}")
                if color is not None and str(color).lower() not in COLORS and not str(color).startswith("#"):
                    raise CircuitError(f"{where}: unknown color {color!r} (use {', '.join(COLORS)} or #rrggbb)")
                self.wires.append(Wire(len(self.wires) + 1, str(group), (str(a), str(b)),
                                       str(color) if color else None, [str(v) for v in via], slant, flex=flex))

    def _load_nets(self, specs: dict) -> None:
        self.nets: dict[str, Net] = {}
        colors: dict[str, str | None] = {}
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
            polarity, rails, guessed = net_polarity(name, spec)
            supplies = sum(1 for n in self.nets.values() if n.polarity == "+")  # each its own colour
            colors[name] = (str(spec["color"]) if spec.get("color") else "black" if polarity == "-" else
                            SUPPLY_COLORS[supplies % len(SUPPLY_COLORS)] if polarity == "+" else None)
            self.nets[name] = Net(name, pins, "", rails, polarity, guessed)
        check_rail_count({n.name: n.polarity for n in self.nets.values()},
                         {n.name for n in self.nets.values() if n.guessed}, list(self.boards.values()))
        # the rest take palette colours that no net has already
        used = {color_hex(c).lower() for c in colors.values() if c}
        palette = cycle([c for c in NET_PALETTE if color_hex(c).lower() not in used] or NET_PALETTE)
        for name, net in self.nets.items():
            net.color = colors[name] or next(palette)

    # ---- placement ----

    def _place(self) -> None:
        for part in self.parts.values():
            if part.kind in FOOTPRINT_KINDS:
                self._place_footprint(part)
        self._layout_offboard()
        from .autoplace import lead_room  # rail ends kept for a supply's flying leads (leg placement, autowire)
        self._lead_room = lead_room(self)
        # Holes written out explicitly are claimed before anything is auto-placed.
        for part in self.parts.values():
            for i, leg in enumerate(part.legs):
                self._reserve(leg, f"{part.ref} leg {i + 1}")
        for w in self.wires:
            for spec in w.spec:
                self._reserve(spec, f"W{w.num}")
        # two-lead parts with their legs written first, so the ones bbgen places see them
        auto = [p for p in self.parts.values() if p.kind in LEAD_KINDS and not p.legs]
        pending = [p for p in self.parts.values() if p.kind in LEAD_KINDS and p not in auto]
        while pending or auto:
            ready = [p for p in pending if not any(self._waits_on_leads(leg) for leg in p.legs)]
            if not ready and auto:
                from .autoplace import place_legs
                place_legs(self, auto)
                auto = []
                continue
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
        if not part.at:
            self.issues.append(Issue("error", "BAD_PLACE", f"{part.ref}: bbgen couldn't place it; give it at:"))
            return
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
            pin.hole = board.hole_at(*pin.xy) or next(
                (h for b in self.boards.values() if b is not board for h in [b.hole_at(*pin.xy)] if h), None)
            if pin.hole is None:
                self.issues.append(Issue("error", "OFF_HOLES", f"{pin} lands in the trench or off the board",
                                         pins=[pin]))
            else:
                self._occupy(pin.hole, str(pin))
                self._pin_holes.add(pin.hole)
        x0, y0, x1, y1 = placed_box(part, body_rect(part))
        # a wire alongside can't lie right against a tall part (a pot, a transistor, a regulator); a DIP's or a
        # module's body hugs its pins, and a wire past its end is fine
        flat = part.kind in ("pico", "dip") or part.source.startswith(("Package_DIP:", "Module:")) \
            or "Package_DIP:" in part.source or "Module:" in part.source
        m = 0.0 if flat else CLEARANCE
        own = {p.hole for p in part.pins}
        if part.raised:  # up on its headers: the holes under it take flat wires, put in before it
            self._raised.append(((x0, y0, x1, y1), part.ref))
            # but its headers are plastic strips along its rows of pins, sitting on the board: nothing crosses one
            rows: dict = {}
            for pin in part.pins:
                if pin.xy:
                    line = round(pin.xy[1], 3) if abs(rotate((1, 0), part.rot)[1]) < 1e-9 else round(pin.xy[0], 3)
                    rows.setdefault(line, []).append(pin.xy)
            for pts in rows.values():
                xs, ys = [x for x, _ in pts], [y for _, y in pts]
                self._bodies.append(((min(xs) - 0.5, min(ys) - 0.3, max(xs) + 0.5, max(ys) + 0.3)
                                     if max(ys) - min(ys) < 1e-9 else
                                     (min(xs) - 0.3, min(ys) - 0.5, max(xs) + 0.3, max(ys) + 0.5), part.ref))
        else:
            self._bodies.append(((x0 - m, y0 - m, x1 + m, y1 + m), part.ref))
            if not flat:
                self._tall.add(part.ref)
        for other in self.boards.values():  # a part across two boards stacked covers holes on both
            for hole in other.holes():
                x, y = other.xy(hole)
                if x0 < x < x1 and y0 < y < y1 and hole not in own:
                    if part.raised:
                        self._under[_key((x, y))] = part.ref
                    else:
                        self._occupy(hole, f"the body of {part.ref}")
        for what, rect in part.keepouts:  # room it needs beyond its body, like a USB cable's plug
            box = placed_box(part, rect)
            self._bodies.append((box, f"{part.ref}'s {what}"))
            self._keepouts.add(f"{part.ref}'s {what}")
            if "cable" in what.lower():  # the cable runs on past the plug, off the board's nearest edge
                if box[2] <= x0:
                    self._cables.append(((board.left, box[1], box[2], box[3]), board))
                elif box[0] >= x1:
                    self._cables.append(((box[0], box[1], board.right, box[3]), board))
                else:
                    self._cables.append((box, board))
            for hole in board.holes():
                x, y = board.xy(hole)
                if box[0] < x < box[2] and box[1] < y < box[3] and hole not in own:
                    self._occupy(hole, f"{part.ref}'s {what}")

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
                ox, oy = below_x + pad_l, max(b.bottom for b in self.boards.values()) + 1.5 + pad_t
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
            a, b = part.pins[0].xy, part.pins[1].xy
            self._add_segment(a, b, part.ref)
            if _aligned(a, b):  # it lies over the holes between its legs
                self._covered.update((k, part.ref) for k in self._between(a, b))
            if part.kind in ROUND_BODY:  # standing up, about two holes across: nothing lies flat under it
                cx, cy, r = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2, ROUND_BODY[part.kind]
                self._bodies.append(((cx - r, cy - r, cx + r, cy + r), part.ref))
                self._tall.add(part.ref)

    def _route(self, w: Wire) -> None:
        sa, sb = w.spec
        who = f"W{w.num}"
        try:
            w.via_xy = [self._via(v) for v in w.via]
            a, b = self._options(sa, self._hint(sb), not w.flex), self._options(sb, self._hint(sa), not w.flex)
            if w.via_xy:  # each end runs to its nearest bend
                pa = self._best(a, [_Option(None, w.via_xy[0])])[0]
                pb = self._best([o for o in b if o.hole is None or o.hole != pa.hole],
                                [_Option(None, w.via_xy[-1])])[0]
            else:
                pa, pb = self._best(a, b, w.flex)
            w.a, w.b = self._take(pa, who, sa), self._take(pb, who, sb)
        except _Unresolved as e:
            self.issues.append(Issue("error", "BAD_WIRE", f"W{w.num} ({sa} -> {sb}): {e}"))
            w.a = w.b = None
            return
        if w.flex:
            return  # it arches over the board: nothing lies under it
        pts = [self.end_xy(w.a), *w.via_xy, self.end_xy(w.b)]
        for p, q in zip(pts, pts[1:]):
            self._add_segment(p, q, who)

    def _best(self, a: list[_Option], b: list[_Option], flex: bool = False) -> tuple[_Option, _Option]:
        """The best pair of places for two ends: straight and clear, then over as few part bodies as
        possible, then nearest their pins, then shortest. Flying leads and flexible jumpers only care
        about the last two."""
        def key(pair):
            p, q = pair
            flying = flex or p.pin is not None or q.pin is not None
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
        along = self._along_who(a, b)
        if along:
            return 3, along
        over = [self._used[k] for k in self._between(a, b) if k in self._used]
        return (1, over) if over else (0, [])

    def _add_segment(self, a, b, who: str) -> None:
        self._segments.append((a, b, who))
        line = _line(a, b)
        if line:
            self._lines.setdefault(line[0], []).append((line[1], line[2], who))

    def _reindex(self) -> None:
        """Rebuild the line index after the segments' names change."""
        segments, self._segments, self._lines = self._segments, [], {}
        for a, b, who in segments:
            self._add_segment(a, b, who)

    def _along_who(self, a, b, exclude: str = "") -> list[str]:
        """What lies along the straight run a-b, sharing some of its length."""
        line = _line(a, b)
        if not line:
            return []
        key, lo, hi = line
        return [who for l2, h2, who in self._lines.get(key, ()) if who != exclude and min(hi, h2) - max(lo, l2) > EPS]

    def _between(self, a, b) -> list[tuple]:
        """Hole positions strictly between a and b on a straight run."""
        if abs(a[1] - b[1]) < EPS:
            lo, hi = sorted((a[0], b[0]))
            return [_key((x, a[1])) for x in range(math.floor(lo + EPS) + 1, math.ceil(hi - EPS))]
        lo, hi = sorted((a[1], b[1]))
        return [_key((a[0], y)) for y in self._levels if lo + EPS < y < hi - EPS]

    def _bodies_crossed(self, a, b) -> list[str]:
        x0, x1, y0, y1 = min(a[0], b[0]), max(a[0], b[0]), min(a[1], b[1]), max(a[1], b[1])
        return [ref for box, ref in self._bodies
                if x1 > box[0] and x0 < box[2] and y1 > box[1] and y0 < box[3] and _crosses(a, b, box)]

    def _take(self, option: _Option, who: str, spec: str = "") -> End:
        if option.pin is not None:
            return End(spec, pin=option.pin)
        if not option.fixed:
            self._occupy(option.hole, who)
        return End(spec, hole=option.hole)

    def cable_holes(self) -> int:
        """Holes on the board under a cable's plug or its way off the board: the room it costs."""
        return sum(1 for box, board in self._cables for h in board.holes()
                   if box[0] < board.xy(h)[0] < box[2] and box[1] < board.xy(h)[1] < box[3])

    # ---- slants ----

    def _find_slants(self) -> None:
        """Warn about wires and two-lead parts that lie slanted where they could run straight."""
        for w in self.wires:
            if w.a and w.b and w.a.hole and w.b.hole and not w.via and not w.slant and not w.flex:
                self._slant(f"W{w.num}", w.spec, (w.a.hole, w.b.hole), w.auto)
        for part in self.parts.values():
            if part.kind in LEAD_KINDS and not part.slant and all(p.hole for p in part.pins):
                self._slant(part.ref, part.legs, tuple(p.hole for p in part.pins), part.auto)

    def _slant(self, name: str, specs, holes: tuple[Hole, Hole], auto: bool = False) -> None:
        if holes[0].board != holes[1].board:
            return
        board = self.boards[holes[0].board]
        if _aligned(board.xy(holes[0]), board.xy(holes[1])):
            return
        route = f"{name} slants ({self.hole_name(holes[0])} → {self.hole_name(holes[1])})"
        accept = ("or place the parts so it can run straight" if auto else
                  f"or accept it with slant: <reason> on {name}")
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
        fix = "Move parts to make room" if auto else f"Move parts to make room, bend it with via:, {accept}"
        self.issues.append(Issue("warning", "DIAGONAL", f"{route}: every straight route is blocked{what}. {fix}",
                                 holes=list(holes)))

    def _find_over_parts(self) -> None:
        """Warn about wires and two-lead parts lying over a part's body: they hide its pins and have to come
        out before it can."""
        for w in self.wires:
            name = f"W{w.num}"
            for end in (w.a, w.b):  # an end under a two-lead part can't be plugged in or pulled out
                if end and end.hole and _key(self.end_xy(end)) in self._covered:
                    ref = self._covered[_key(self.end_xy(end))]
                    self.issues.append(Issue("warning", "OVER_PART", f"{name} ends in {self.hole_name(end.hole)}, "
                                             f"under {ref}: move one of them", holes=[end.hole]))
            if w.flex and w.a and w.b and w.a.hole and w.b.hole:  # in the air, but not where a plug goes
                zones = [z for z in self._bodies_crossed(self.end_xy(w.a), self.end_xy(w.b)) if z in self._keepouts]
                if zones:
                    self.issues.append(Issue("warning", "OVER_PART", f"{name} arches over {' and '.join(zones)}, "
                                             "which needs that room: move one of them", holes=[w.a.hole, w.b.hole]))
            if not (w.a and w.b and w.a.hole and w.b.hole) or w.flex:
                continue  # flying leads and flexible jumpers are in the air
            pts = [self.end_xy(w.a), *w.via_xy, self.end_xy(w.b)]
            ends = f"({self.hole_name(w.a.hole)} → {self.hole_name(w.b.hole)})"
            wires = {f"W{x.num}" for x in self.wires}  # wire on wire: _find_stacking's business
            along = list(dict.fromkeys(who for p, q in zip(pts, pts[1:]) for who in self._along_who(p, q, name)
                                       if who not in wires))
            if along:
                self.issues.append(Issue("warning", "OVER_PART", f"{name} runs along {' and '.join(along)} {ends}, "
                                         "hiding it: move one of them", holes=[w.a.hole, w.b.hole]))
            refs = list(dict.fromkeys(r for p, q in zip(pts, pts[1:]) for r in self._bodies_crossed(p, q)))
            if not refs:
                continue
            route = f"{name} runs over {' and '.join(refs)} {ends}"
            detour = None if w.via else self._detour(w.a.hole, w.b.hole, name)
            fix = (f"bend it around with via: [{', '.join(detour)}]" if detour else
                   "move the bends in its via" if w.via else "move a part to make room")
            self.issues.append(Issue("warning", "OVER_PART", f"{route}: {fix}", holes=[w.a.hole, w.b.hole]))
        for part in self.parts.values():
            if part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
                refs = [r for r in self._bodies_crossed(part.pins[0].xy, part.pins[1].xy) if r != part.ref]
                if refs:
                    self.issues.append(Issue(
                        "warning", "OVER_PART", f"{part.ref} lies over {' and '.join(refs)} "
                        f"({self.hole_name(part.pins[0].hole)} → {self.hole_name(part.pins[1].hole)}): "
                        "put its legs where it clears the part", holes=[p.hole for p in part.pins]))

    def _find_under(self) -> None:
        """Under a raised module there's room for a flat wire and nothing else: not a two-lead part's leg, nor a
        flexible jumper's or a flying lead's plug; and a two-lead part can't lie across one."""
        for part in self.parts.values():
            if part.kind not in LEAD_KINDS or not all(p.xy for p in part.pins):
                continue
            for pin in part.pins:
                if _key(pin.xy) in self._under:
                    self.issues.append(Issue("error", "HOLE_TAKEN", f"{part.ref} leg in {self.hole_name(pin.hole)}, "
                                             f"under {self._under[_key(pin.xy)]}: only a flat wire goes under a raised "
                                             "module", holes=[pin.hole]))
            refs = self._raised_crossed(part.pins[0].xy, part.pins[1].xy)
            if refs:
                self.issues.append(Issue("warning", "OVER_PART", f"{part.ref} lies across {' and '.join(refs)}, "
                                         "which stands over it: put its legs where it clears the part",
                                         holes=[p.hole for p in part.pins]))
        for w in self.wires:
            if not (w.flex or (w.a and w.a.pin) or (w.b and w.b.pin)):
                continue
            for end in (w.a, w.b):
                if end and end.hole and _key(self.end_xy(end)) in self._under:
                    what = "a flexible jumper's" if w.flex else "a flying lead's"
                    self.issues.append(Issue("error", "HOLE_TAKEN", f"W{w.num} ends in {self.hole_name(end.hole)}, "
                                             f"under {self._under[_key(self.end_xy(end))]}: {what} plug won't go "
                                             "under a raised module", holes=[end.hole]))

    def _raised_crossed(self, a, b) -> list[str]:
        """The raised modules a straight run from a to b passes under (or a part would lie across)."""
        x0, x1, y0, y1 = min(a[0], b[0]), max(a[0], b[0]), min(a[1], b[1]), max(a[1], b[1])
        return [ref for box, ref in self._raised
                if x1 > box[0] and x0 < box[2] and y1 > box[1] and y0 < box[3] and _crosses(a, b, box)]

    def under(self, w: Wire) -> list[str]:
        """The raised modules a flat wire lies under or ends under: it goes in before them."""
        if w.flex or not (w.a and w.b and w.a.hole and w.b.hole):
            return []
        keys = self._lies_over(w) | {_key(self.end_xy(e)) for e in (w.a, w.b)}
        refs = {self._under[k] for k in keys if k in self._under}
        pts = [self.end_xy(w.a), *w.via_xy, self.end_xy(w.b)]
        refs |= {r for p, q in zip(pts, pts[1:]) for r in self._raised_crossed(p, q)}
        return sorted(refs)

    def _lies_over(self, w: Wire) -> set:
        """Positions of the holes a wire lying on the board covers: between its ends, and at its bends."""
        if w.flex or not (w.a and w.b and w.a.hole and w.b.hole):
            return set()
        pts = [self.end_xy(w.a), *w.via_xy, self.end_xy(w.b)]
        out = {_key(v) for v in w.via_xy}
        for p, q in zip(pts, pts[1:]):
            if _aligned(p, q):
                out.update(self._between(p, q))
        return out

    def _find_stacking(self) -> None:
        """Wires on wires, in the order they're built (the checklist's: by number, flexible jumpers last). An end
        in a hole that a wire built before it lies over can't be plugged in; a wire lying over one built
        before it, end to end, hides it."""
        rank = {w.num: (w.flex, w.num) for w in self.wires}
        flat = [(w, self._lies_over(w)) for w in self.wires if not w.flex]
        for w in self.wires:
            for end in (w.a, w.b):
                if not (end and end.hole):
                    continue
                k = _key(self.end_xy(end))
                for other, over in flat:
                    if other is not w and k in over and rank[other.num] < rank[w.num]:
                        what = "the jumper" if w.flex else f"W{w.num}"
                        self.issues.append(Issue(
                            "error", "END_COVERED", f"W{w.num} ends in {self.hole_name(end.hole)}, under W{other.num}, "
                            f"which goes in first: {what} can't be plugged in. Move one of them", holes=[end.hole]))
        for i, (w, over) in enumerate(flat):  # nested: a wire lying along one built before it, over both its ends
            for other, over2 in flat[:i]:
                if rank[other.num] < rank[w.num] and {_key(self.end_xy(e)) for e in (other.a, other.b) if e and e.hole} <= over \
                        and other.a and other.a.hole:
                    self.issues.append(Issue(
                        "warning", "NESTED", f"W{w.num} lies over W{other.num}, hiding it, and has to come out before it "
                        f"can: buildable, but hard to trace (nested: flex makes the outer one a flexible jumper)",
                        holes=[w.a.hole, w.b.hole]))

    def _detour(self, a: Hole, b: Hole, who: str = "") -> list[str] | None:
        found = self._detour_cost(a, b, who)
        return found[1] if found else None

    def _detour_cost(self, a: Hole, b: Hole, who: str = "") -> tuple[tuple, list[str]] | None:
        """Bends (one or two holes) that take a wire from a to b around every part body, lying along as few
        other wires and parts and passing as few holes in use or under parts as possible, then shortest;
        with that cost, (trouble, length)."""
        if a.board != b.board:
            return None
        board = self.boards[a.board]
        routes = [[(a.row, b.col)], [(b.row, a.col)]]
        span = range(max(1, min(a.col, b.col) - DETOUR_REACH), min(board.columns, max(a.col, b.col) + DETOUR_REACH) + 1)
        routes += [[(a.row, col), (b.row, col)] for col in span]
        best = None
        for route in routes:
            if not all(board.has(row, col) for row, col in route):
                continue
            pts = [board.xy(a), *(board.xy(Hole(board.id, row, col)) for row, col in route), board.xy(b)]
            segs = list(zip(pts, pts[1:]))
            if any(self._bodies_crossed(p, q) for p, q in segs):
                continue
            along = sum(len(self._along_who(p, q, who)) for p, q in segs)
            over = 10 * along + sum(k in self._used or k in self._covered for p, q in segs for k in self._between(p, q))
            cost = (over, round(sum(math.dist(p, q) for p, q in segs), 6))
            if best is None or cost < best[0]:
                best = (cost, [self.hole_name(Hole(board.id, row, col)) for row, col in route])
        return best

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

    def _options(self, spec: str, hint, flat: bool = False) -> list[_Option]:
        """Every place one end could go: a hole written out, a free hole on a bare rail, a free hole in a
        pin's strip, or an off-board pin. Only a flat wire's end may go under a raised module."""
        parsed = parse_hole(spec)
        if parsed:
            board_id, row, col = parsed
            if col is not None:
                hole = self._hole(spec)  # claimed by _reserve
                return [_Option(hole, self.boards[hole.board].xy(hole), fixed=True)]
            if row not in RAILS:
                raise _Unresolved(f"'{spec}' needs a column number")
            board = self._board(board_id)
            return self._free(board, [h for h in board.holes() if h.row == row], None, f"the {row} rail", flat)
        pin = self._pin(spec, hint)
        if pin.part.kind == "offboard":
            return [_Option(None, pin.xy, pin=pin)]
        if pin.hole is None:
            raise _Unresolved(f"{pin} is not on the board")
        board = self.boards[pin.hole.board]
        return self._free(board, board.strip_holes(board.strip(pin.hole)), pin.xy, f"the strip of {pin}", flat)

    def _free(self, board: Board, holes: list[Hole], near, what: str, flat: bool = False) -> list[_Option]:
        free = [_Option(h, board.xy(h), _dist(board.xy(h), near)) for h in holes if h not in self.occupied
                and (flat or _key(board.xy(h)) not in self._under)]
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
