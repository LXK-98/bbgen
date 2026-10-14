"""Breadboard geometry, in grid units of 0.1 in (one hole pitch).

Each column's a-e holes form one strip and its f-j holes another, with the trench
between e and f. The four rails (top+, top-, bot+, bot-) run along the board in
groups of five; each rail is one strip unless the board splits it in the middle.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

ROW_Y = {"a": 3, "b": 4, "c": 5, "d": 6, "e": 7, "f": 10, "g": 11, "h": 12, "i": 13, "j": 14}
RAILS = ("top+", "top-", "bot+", "bot-")
RAIL_PAIR_Y = {"top": (0, 1), "bot": (16, 17)}
OUTLINE_Y = (-1.2, 18.2)
TRENCH_Y = (7.6, 9.4)
# a board stacked under another (`below:`) sits this many holes lower, on the same 0.1 in grid: the boards'
# long sides snapped together, its row a 1.0 in below the upper board's row i
STACK = 20
# type: (columns, first rail column, rail groups of five)
PRESETS = {"full": (63, 3, 10), "half": (30, 2, 5)}

_HOLE_RE = re.compile(r"^(?:(?P<board>[^:.\s]+):)?(?P<row>top[+-]|bot[+-]|[a-j])(?P<col>\d+)?$")


@dataclass(frozen=True)
class Hole:
    board: str
    row: str
    col: int

    @property
    def is_rail(self) -> bool:
        return self.row in RAILS


def parse_hole(text: str) -> tuple[str | None, str, int | None] | None:
    """'a12', '2:a12', 'top+12' or a bare rail 'top+' -> (board, row, col); None if not a hole."""
    m = _HOLE_RE.match(text.strip())
    if not m:
        return None
    return m["board"], m["row"], int(m["col"]) if m["col"] else None


class Board:
    def __init__(self, id, kind: str = "full", rails: dict | None = None, split_rails: bool = False):
        if kind not in PRESETS:
            raise ValueError(f"unknown board type {kind!r} (expected {' or '.join(PRESETS)})")
        self.id, self.kind, self.split_rails = str(id), kind, bool(split_rails)
        self.columns, first, groups = PRESETS[kind]
        self.rail_cols = [first + 6 * g + k for g in range(groups) for k in range(5)]
        self._rail_set = set(self.rail_cols)
        self._split_after = self.rail_cols[len(self.rail_cols) // 2 - 1]
        rails = rails or {}
        self.rail_order = {"top": str(rails.get("top", "+-")),
                           "bot": str(rails.get("bottom", rails.get("bot", "+-")))}
        for side, order in self.rail_order.items():
            if sorted(order) != ["+", "-"]:
                raise ValueError(f"rails.{side} must be '+-' or '-+' (top to bottom), not {order!r}")
        self.x0 = 0.0  # x of column 1, set when boards are laid out side by side
        self.y0 = 0.0  # how far below the first boards' row it sits (STACK for one stacked under another)

    @property
    def left(self) -> float:
        return self.x0 - 2

    @property
    def right(self) -> float:
        return self.x0 + self.columns + 1

    @property
    def top(self) -> float:
        return self.y0 + OUTLINE_Y[0]

    @property
    def bottom(self) -> float:
        return self.y0 + OUTLINE_Y[1]

    def rail_y(self, rail: str) -> int:
        side, pol = rail[:3], rail[3]
        return RAIL_PAIR_Y[side][self.rail_order[side].index(pol)]

    def y(self, row: str) -> float:
        """Where a row of holes is, down the drawing (rail_y and ROW_Y are a board's own, from its top)."""
        try:
            return self._y[row]
        except (AttributeError, KeyError):
            self._y = {**{r: self.y0 + y for r, y in ROW_Y.items()}, **{r: self.y0 + self.rail_y(r) for r in RAILS}}
            return self._y[row]

    def has(self, row: str, col: int) -> bool:
        if row in ROW_Y:
            return 1 <= col <= self.columns
        return row in RAILS and col in self._rail_set

    def xy(self, hole: Hole) -> tuple[float, float]:
        return self.x0 + hole.col - 1, float(self.y(hole.row))

    def hole_at(self, x: float, y: float) -> Hole | None:
        col = x - self.x0 + 1
        if abs(col - round(col)) > 1e-6:
            return None
        rows = [r for r in (*ROW_Y, *RAILS) if abs(self.y(r) - y) < 1e-6]
        if rows and self.has(rows[0], round(col)):
            return Hole(self.id, rows[0], round(col))
        return None

    def strip(self, hole: Hole) -> tuple:
        """The holes joined under the board, as a hashable key."""
        if hole.row in ROW_Y:
            return (self.id, "a-e" if hole.row in "abcde" else "f-j", hole.col)
        return (self.id, hole.row, int(self.split_rails and hole.col > self._split_after))

    def strip_holes(self, strip: tuple) -> list[Hole]:
        """The holes of a strip, in order (a fresh list: the board's own is cached, as its holes never change)."""
        try:
            return list(self._strip_holes[strip])
        except AttributeError:
            self._strip_holes = {}
        except KeyError:
            pass
        _, kind, n = strip
        if kind in ("a-e", "f-j"):
            holes = [Hole(self.id, r, n) for r in ("abcde" if kind == "a-e" else "fghij")]
        else:
            holes = [h for h in (Hole(self.id, kind, c) for c in self.rail_cols) if self.strip(h) == strip]
        self._strip_holes[strip] = tuple(holes)
        return holes

    def holes(self):
        for row in ROW_Y:
            for col in range(1, self.columns + 1):
                yield Hole(self.id, row, col)
        for rail in RAILS:
            for col in self.rail_cols:
                yield Hole(self.id, rail, col)

    def name(self, hole: Hole, default_board: str) -> str:
        prefix = "" if hole.board == default_board else f"{hole.board}:"
        return f"{prefix}{hole.row}{hole.col}"
