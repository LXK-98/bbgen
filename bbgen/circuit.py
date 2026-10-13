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

from .board import OUTLINE_Y, RAILS, Board, Hole, parse_hole
from .parts import FOOTPRINT_KINDS, LEAD_KINDS, Part, PartError, Pin, body_rect, make_part, placed_box, rotate

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
    a: End | None = None
    b: End | None = None
    via_xy: list = field(default_factory=list)


@dataclass
class Net:
    name: str
    pins: list[Pin]
    color: str


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
        self._load_boards(data.get("boards") or [{"type": "full"}])
        self._load_parts(data.get("parts") or {})
        self._load_wires(data.get("wires") or [])
        self._place()
        self._load_nets(data.get("nets") or {})

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
                if isinstance(item, list) and len(item) in (2, 3):
                    a, b, color, via = item[0], item[1], item[2] if len(item) == 3 else None, []
                elif isinstance(item, dict) and "from" in item and "to" in item:
                    a, b, color, via = item["from"], item["to"], item.get("color"), item.get("via") or []
                else:
                    raise CircuitError(f"{where}: expected [from, to], [from, to, color] or {{from, to, color, via}}")
                if color is not None and str(color).lower() not in COLORS and not str(color).startswith("#"):
                    raise CircuitError(f"{where}: unknown color {color!r} (use {', '.join(COLORS)} or #rrggbb)")
                self.wires.append(Wire(len(self.wires) + 1, str(group), (str(a), str(b)),
                                       str(color) if color else None, [str(v) for v in via]))

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
        x0, y0, x1, y1 = placed_box(part, body_rect(part))
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
        for i, (pin, spec) in enumerate(zip(part.pins, part.legs)):
            other = part.pins[1 - i]
            hint = other.xy if other.xy else self._hint(part.legs[1 - i])
            try:
                end = self._resolve(spec, hint, f"{part.ref} leg {i + 1}")
            except _Unresolved as e:
                self.issues.append(Issue("error", "BAD_LEG", f"{part.ref} leg {i + 1} ({spec}): {e}"))
                continue
            if end.hole is None:
                self.issues.append(Issue("error", "BAD_LEG", f"{part.ref} leg {i + 1} ({spec}): a leg goes in a "
                                                            "breadboard hole, not an off-board pin"))
                continue
            pin.hole = end.hole
            pin.xy = self.boards[end.hole.board].xy(end.hole)

    def _route(self, w: Wire) -> None:
        sa, sb = w.spec
        try:
            w.a = self._resolve(sa, self._hint(sb), f"W{w.num}")
            w.b = self._resolve(sb, self.end_xy(w.a), f"W{w.num}")
            w.via_xy = [self._via(v) for v in w.via]
        except _Unresolved as e:
            self.issues.append(Issue("error", "BAD_WIRE", f"W{w.num} ({sa} -> {sb}): {e}"))
            w.a = w.b = None

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

    def _resolve(self, spec: str, hint, who: str) -> End:
        parsed = parse_hole(spec)
        if parsed:
            board_id, row, col = parsed
            if col is not None:
                return End(spec, hole=self._hole(spec))  # claimed by _reserve
            if row not in RAILS:
                raise _Unresolved(f"'{spec}' needs a column number")
            board = self._board(board_id)
            holes = [h for h in board.holes() if h.row == row]
            return End(spec, hole=self._alloc(board, holes, None, hint, who, f"the {row} rail"))
        pin = self._pin(spec, hint)
        if pin.part.kind == "offboard":
            return End(spec, pin=pin)
        if pin.hole is None:
            raise _Unresolved(f"{pin} is not on the board")
        board = self.boards[pin.hole.board]
        holes = board.strip_holes(board.strip(pin.hole))
        return End(spec, hole=self._alloc(board, holes, pin.xy, hint, who, f"the strip of {pin}"))

    def _alloc(self, board: Board, holes: list[Hole], near, hint, who: str, what: str) -> Hole:
        """Take the free hole nearest `near` (the pin), then nearest `hint` (the other end)."""
        free = [h for h in holes if h not in self.occupied]
        if not free:
            raise _Unresolved(f"no free hole left in {what}")

        def key(h: Hole):
            xy = board.xy(h)
            return (round(_dist(xy, near), 6), round(_dist(xy, hint), 6), xy[0], xy[1])

        hole = min(free, key=key)
        self._occupy(hole, who)
        return hole

    def _via(self, spec: str) -> tuple[float, float]:
        hole = self._hole(spec)
        return self.boards[hole.board].xy(hole)

    def end_xy(self, end: End) -> tuple[float, float]:
        if end.pin is not None:
            return end.pin.xy
        return self.boards[end.hole.board].xy(end.hole)

    def hole_name(self, hole: Hole) -> str:
        return self.boards[hole.board].name(hole, self.default_board)
