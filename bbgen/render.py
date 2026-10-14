"""SVG drawing of the boards, the parts on them, the wires and any problems."""

from __future__ import annotations

import math
import re
from xml.sax.saxutils import escape

from .board import OUTLINE_Y, RAIL_PAIR_Y, RAILS, ROW_Y, TRENCH_Y, Board
from .check import Result, end_node, wire_color
from .circuit import Circuit, Wire, color_hex
from .parts import FOOTPRINT_KINDS, LEAD_KINDS, LED_COLORS, Part, body_rect, placed_box, rotate

P = 20.0     # pixels per grid unit (0.1 in)
FONT = "DejaVu Sans, Helvetica, Arial, sans-serif"
HEADER = 3.4  # grid units above the drawing for the title
INK, MUTED, BOARD = "#262626", "#9a917f", "#f6f3ec"

BODY_FILL = {"pico": "#2f6e3d", "dip": "#24467a", "sip": "#3d3d3d", "kicad": "#24467a", "module": "#2f6e3d"}
# fill, outline, thickness and longest body length, in grid units
LEAD_STYLE = {
    "resistor": ("#e6d2a4", "#8b7448", 0.5, 1.9),
    "capacitor": ("#e9c84a", "#94791c", 0.55, 0.8),
    "electrolytic": ("#253f8e", "#101c45", 0.85, 1.3),
    "diode": ("#2a2a2a", "#000000", 0.45, 1.3),
    "led": ("#e8463a", "#8e1d14", 0.8, 0.8),
}

# resistor colour code: digit (or multiplier exponent) -> band colour
BAND = ("#1a1a1a", "#7a4a2a", "#d0281c", "#f07c12", "#f4d21a",
        "#2e9a3a", "#2152c4", "#7d3fb0", "#8c8c8c", "#f9f9f9")
GOLD, SILVER = "#c9a227", "#b8bcc2"
_OHMS = re.compile(r"^(\d*)(?:[.,](\d+))?([rkm]?)$|^(\d+)([rkm])(\d+)$")  # 4.7k, or 4k7


def resistor_bands(value: str) -> list[str] | None:
    """Colour bands for a resistance like 10k, 330R, 4k7, 2.2M or 100: two digits, multiplier and gold
    (5 %), or three digits, multiplier and brown (1 %) when two digits can't say it. None if unreadable."""
    s = value.strip().lower().replace("ω", "").replace("ohms", "").replace("ohm", "").replace(" ", "")
    m = _OHMS.match(s)
    if not m or not (m[1] or m[2] or m[4]):
        return None
    whole, frac, suffix = (m[1], m[2], m[3]) if m[4] is None else (m[4], m[6], m[5])
    ohms = float(f"{whole or 0}.{frac or 0}") * {"k": 1e3, "m": 1e6}.get(suffix, 1)
    if ohms == 0:
        return [BAND[0]]
    for digits, tolerance in ((2, GOLD), (3, BAND[1])):
        exp = math.floor(math.log10(ohms)) - digits + 1
        sig = round(ohms / 10 ** exp)
        if sig == 10 ** digits:  # 99.96 rounds up a decade
            sig, exp = sig // 10, exp + 1
        if abs(sig * 10.0 ** exp - ohms) <= ohms * 1e-9 or digits == 3:
            break
    if not -2 <= exp <= 9:
        return None
    mult = GOLD if exp == -1 else SILVER if exp == -2 else BAND[exp]
    return [BAND[int(d)] for d in str(sig)] + [mult, tolerance]


def _outline(color: str) -> str:
    """Dark outline around a wire, or a light one when the wire itself is dark (black, dark blue...)."""
    if re.fullmatch(r"#[0-9a-fA-F]{6}", color):
        r, g, b = (int(color[i:i + 2], 16) / 255 for i in (1, 3, 5))
        if 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.25:
            return "#c2c2c2"
    return "#1d1d1d"


def _text_box(gx, gy, s, size, anchor="middle", vertical=False):
    """A label's extent in grid units, roughly."""
    w, h = len(s) * size * 0.55 / P, size * 0.8 / P
    lo = {"start": 0.0, "end": -w, "middle": -w / 2}[anchor]
    if vertical:  # reads bottom to top: the length runs up from gy
        return (gx - h / 2, gy - lo - w, gx + h / 2, gy - lo)
    return (gx + lo, gy - h / 2, gx + lo + w, gy + h / 2)


def _seg_hits_box(a, b, box) -> bool:
    """A straight run passing through a box (sampled: runs are short and boxes small)."""
    x0, y0, x1, y1 = box
    n = max(2, int(math.dist(a, b) * 4))
    return any(x0 < a[0] + (b[0] - a[0]) * t / n < x1 and y0 < a[1] + (b[1] - a[1]) * t / n < y1
               for t in range(n + 1))


class _Canvas:
    def __init__(self, x0: float, y0: float):
        self.x0, self.y0, self.out = x0, y0, []
        self.late: list[str] = []  # part labels, drawn over the wires
        self.labels: list[tuple] = []  # (box, text) in grid units for each label over the wires, and wire tags
        self.spots: list[tuple] = []   # wire ends and bends: a label must never hide one
        self.segs: list[tuple] = []    # (a, b, owner) for each wire run: a label had better not lie across one

    def clash(self, box, owner: str = "") -> float:
        """How badly a label there would hide things: wire ends and other labels count most."""
        x0, y0, x1, y1 = box
        n = 3.0 * sum(1 for x, y in self.spots if x0 < x < x1 and y0 < y < y1)
        n += 3.0 * sum(1 for b, _ in self.labels if x0 < b[2] and b[0] < x1 and y0 < b[3] and b[1] < y1)
        n += sum(1 for a, b, who in self.segs if who != owner and _seg_hits_box(a, b, box))
        return n

    def label(self, spots: list, s: str, size: float, owner: str = "", **kw) -> None:
        """A label at the first of spots [(gx, gy, anchor), ...] that hides least."""
        best = min(range(len(spots)), key=lambda i: (self.clash(_text_box(*spots[i][:2], s, size, spots[i][2]), owner), i))
        gx, gy, anchor = spots[best]
        self.text(gx, gy, s, size, anchor, late=True, **kw)

    def X(self, gx: float) -> float:
        return (gx - self.x0) * P

    def Y(self, gy: float) -> float:
        return (gy - self.y0) * P

    def add(self, svg: str) -> None:
        self.out.append(svg)

    def text(self, gx, gy, s, size=7.0, anchor="middle", fill=INK, weight="normal", vertical=False,
             halo=None, late=False) -> None:
        """Text centred across its height on (gx, gy); vertical text reads bottom to top."""
        if late:
            self.labels.append((_text_box(gx, gy, s, size, anchor, vertical), s))
        x, y = self.X(gx), self.Y(gy)
        tr = ""
        if vertical:
            x += 0.35 * size
            tr = f' transform="rotate(-90 {x:.1f} {y:.1f})"'
        else:
            y += 0.35 * size
        extra = (f' stroke="{halo}" stroke-width="2.6" stroke-linejoin="round" paint-order="stroke"'
                 if halo else "")
        (self.late if late else self.out).append(
            f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" text-anchor="{anchor}" fill="{fill}" '
            f'font-weight="{weight}"{tr}{extra}>{escape(s)}</text>')

    def rect(self, x0, y0, x1, y1, fill, stroke="none", width=1.0, rx=0.0, extra="") -> None:
        self.add(f'<rect x="{self.X(x0):.1f}" y="{self.Y(y0):.1f}" width="{(x1 - x0) * P:.1f}" '
                 f'height="{(y1 - y0) * P:.1f}" rx="{rx * P:.1f}" fill="{fill}" stroke="{stroke}" '
                 f'stroke-width="{width}"{extra}/>')

    def circle(self, gx, gy, r, fill, stroke="none", width=1.0) -> None:
        self.add(f'<circle cx="{self.X(gx):.1f}" cy="{self.Y(gy):.1f}" r="{r * P:.1f}" fill="{fill}" '
                 f'stroke="{stroke}" stroke-width="{width}"/>')

    def line(self, ax, ay, bx, by, stroke, width) -> None:
        self.add(f'<line x1="{self.X(ax):.1f}" y1="{self.Y(ay):.1f}" x2="{self.X(bx):.1f}" '
                 f'y2="{self.Y(by):.1f}" stroke="{stroke}" stroke-width="{width * P:.1f}" '
                 f'stroke-linecap="round"/>')


def _board(cv: _Canvas, c: Circuit, result: Result, b: Board) -> None:
    top = b.y0  # every height on the board is from its own top (a board stacked under another is lower)
    cv.rect(b.left, b.top, b.right, b.bottom, BOARD, "#d5cdbb", 1.5, rx=0.5)
    last = b.x0 + b.columns - 1
    cv.rect(b.x0 - 0.9, top + TRENCH_Y[0], last + 0.9, top + TRENCH_Y[1], "#e7e1d3", rx=0.2)
    if len(c.boards) > 1:
        cv.text(b.left + 0.4, b.top + 0.5, f"board {b.id}", 6.5, "start", MUTED, "bold")
    for rail in RAILS:
        y = top + b.rail_y(rail)
        upper = b.rail_y(rail) == RAIL_PAIR_Y[rail[:3]][0]
        color = "#d33a2c" if rail[3] == "+" else "#2a5db0"
        xa, xb = b.x0 + b.rail_cols[0] - 1.7, b.x0 + b.rail_cols[-1] - 0.3
        cv.line(xa, y + (-0.55 if upper else 0.55), xb, y + (-0.55 if upper else 0.55), color, 0.06)
        sign = "+" if rail[3] == "+" else "−"
        cv.text(xa - 0.4, y, sign, 9, fill=color, weight="bold")
        cv.text(xb + 0.4, y, sign, 9, fill=color, weight="bold")
        halves = {b.strip(h) for h in b.holes() if h.row == rail}
        lx, rx = min(b.x0 - 1.2, xa - 1.0), max(last + 1.2, xb + 1.0)
        for strip in sorted(halves):
            nets = result.nets_at(("strip", *strip))
            if len(nets) == 1:
                at_right = strip[2] == 1
                cv.text(rx if at_right else lx, y, nets[0], 6, "start" if at_right else "end", color, "bold")
                if not b.split_rails:
                    cv.text(rx, y, nets[0], 6, "start", color, "bold")
    for hole in b.holes():
        x, y = b.xy(hole)
        cv.rect(x - 0.17, y - 0.17, x + 0.17, y + 0.17, "#4b4b4b", rx=0.06)
    for row in ROW_Y:
        cv.text(b.x0 - 1.1, b.y(row), row, 6.5, fill=MUTED)
        cv.text(last + 1.1, b.y(row), row, 6.5, fill=MUTED)
    for col in [1, *range(5, b.columns + 1, 5)]:
        cv.text(b.x0 + col - 1, top + 2.05, str(col), 5.5, fill=MUTED)
        cv.text(b.x0 + col - 1, top + 14.95, str(col), 5.5, fill=MUTED)


def _keepouts(cv: _Canvas, c: Circuit, part: Part) -> None:
    """Room a part needs beyond its body (a USB cable's plug), hatched, as far as it lies on a board."""
    if not part.keepouts or part.pins[0].xy is None or part.pins[0].hole is None:
        return
    board = c.boards[part.pins[0].hole.board]
    for what, rect in part.keepouts:
        x0, y0, x1, y1 = placed_box(part, rect)
        x0, x1 = max(x0, board.left + 0.3), min(x1, board.right - 0.3)
        y0, y1 = max(y0, board.top + 0.3), min(y1, board.bottom - 0.3)
        if x1 - x0 < 0.3 or y1 - y0 < 0.3:
            continue
        cv.rect(x0, y0, x1, y1, "url(#keepout)", "#b0a58c", 1, rx=0.15, extra=' stroke-dasharray="4 3"')
        cv.text((x0 + x1) / 2, (y0 + y1) / 2, what, 6, fill="#7d7258", weight="bold", halo=BOARD, late=True)


def _footprint(cv: _Canvas, part: Part) -> None:
    if part.pins[0].xy is None:
        return
    x0, y0, x1, y1 = placed_box(part, body_rect(part))
    big = part.kind == "pico" or (part.kind == "kicad" and min(x1 - x0, y1 - y0) >= 6)  # a module, like a Pico
    fill = BODY_FILL["module" if big and part.kind == "kicad" else part.kind]
    cv.rect(x0, y0, x1, y1, part.color or fill, "#111", 1, rx=0.15)
    if part.kind == "pico":
        cv.rect(*placed_box(part, (-1.05, -4.5, 0.75, -2.5)), "#c9c9c9", "#6f6f6f", 1, rx=0.08)
    inside = part.kind in ("pico", "dip") or (part.kind == "kicad" and len({p.offset[1] for p in part.pins}) > 1)
    if inside:  # the pin-1 dot
        dot = rotate((-0.36, 0.36), part.rot)
        cv.circle(part.pins[0].xy[0] + dot[0], part.pins[0].xy[1] + dot[1], 0.09, "#ffffff")
    for pin in part.pins:
        x, y = pin.xy
        if pin.num == 1:
            cv.rect(x - 0.19, y - 0.19, x + 0.19, y + 0.19, "#d9b44a", "#8a6d1c", 0.6)
        else:
            cv.circle(x, y, 0.19, "#d9b44a", "#8a6d1c", 0.6)
        dx, dy = rotate(pin.label_dir, part.rot)
        fill = "#f1f1f1" if inside else INK
        if dx == 0:
            cv.text(x, y + dy * 0.38, pin.name, 5.4, "start" if dy < 0 else "end", fill, vertical=True,
                    halo=None if inside else "#ffffff")
        else:
            cv.text(x + dx * 0.38, y, pin.name, 5.4, "start" if dx > 0 else "end", fill,
                    halo=None if inside else "#ffffff")
    label = f"{part.ref} {part.title}".strip()
    if big:
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        cv.text(cx, cy - 0.4, part.ref, 9, fill="#ffffff", weight="bold")
        cv.text(cx, cy + 0.45, part.title, 6.5, fill="#dfe9df")
    else:
        above, below = ((x0 + x1) / 2, y0 - 0.5, "middle"), ((x0 + x1) / 2, y1 + 0.55, "middle")
        spots = [below, above] if part.kind == "sip" else [above, below]
        # if both hide something: flush with either end of the body, then a row further out
        for y in (spots[0][1], spots[1][1]):
            spots += [(x0, y, "start"), (x1, y, "end")]
        for x, y, _ in spots[:2]:
            spots.append((x, y + (0.5 if y > (y0 + y1) / 2 else -0.5), "middle"))
        cv.label(spots, label, 6.5, part.ref, weight="bold", halo="#ffffff")


def _lead(cv: _Canvas, part: Part) -> None:
    if any(p.xy is None for p in part.pins):
        return
    (ax, ay), (bx, by) = part.pins[0].xy, part.pins[1].xy
    length = math.hypot(bx - ax, by - ay) or 1e-9
    ux, uy = (bx - ax) / length, (by - ay) / length
    nx, ny = -uy, ux
    if ny > 0 or (ny == 0 and nx > 0):
        nx, ny = -nx, -ny  # the side facing up (or left)
    if part.kind == "electrolytic":
        _can(cv, part, (ax, ay), (bx, by), (ux, uy), (nx, ny), length)
        return
    if part.kind == "led":
        _led(cv, part, (ax, ay), (bx, by), (ux, uy), (nx, ny), length)
        return
    fill, stroke, thick, longest = LEAD_STYLE[part.kind]
    if length < 1.8:  # standing up beside its legs, half a pitch over: between rows, not on the next holes
        cx, cy = (ax + bx) / 2 + nx * 0.5, (ay + by) / 2 + ny * 0.5
        blen = min(longest, 0.9)
    else:
        cx, cy = (ax + bx) / 2, (ay + by) / 2
        blen = min(longest, length - 0.6)
    e1 = (cx - ux * blen / 2, cy - uy * blen / 2)
    e2 = (cx + ux * blen / 2, cy + uy * blen / 2)
    cv.line(ax, ay, *e1, "#8d8d8d", 0.1)
    cv.line(bx, by, *e2, "#8d8d8d", 0.1)
    angle = math.degrees(math.atan2(uy, ux))
    w, h = blen * P, thick * P
    cv.add(f'<g transform="translate({cv.X(cx):.1f} {cv.Y(cy):.1f}) rotate({angle:.1f})">')
    cv.add(f'<rect x="{-w / 2:.1f}" y="{-h / 2:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{h / 3:.1f}" '
           f'fill="{part.color or fill}" stroke="{stroke}" stroke-width="1"/>')
    bands = resistor_bands(part.value) if part.kind == "resistor" else None
    if bands:  # read left to right (or top to bottom), the tolerance band set apart at the end
        span = w - 2 * h / 3  # clear of the rounded ends
        bw = min(span / (len(bands) + 2), 3.4)
        at = [-span / 2 + (i + 0.5) * span / (len(bands) + 1) for i in range(len(bands) - 1)] + [span / 2 - bw / 2]
        if ux < -1e-6 or (abs(ux) <= 1e-6 and uy < 0):  # leg 1 is on the right or at the bottom
            at = [-x for x in at]
        for x, band in zip(at, bands):
            cv.add(f'<rect x="{x - bw / 2:.1f}" y="{-h / 2 + 0.5:.1f}" width="{bw:.1f}" height="{h - 1:.1f}" '
                   f'fill="{band}" stroke="#00000040" stroke-width="0.5"/>')
    cv.add("</g>")
    if part.kind in ("diode", "led"):  # cathode band, toward leg 2
        k = (e2[0] - ux * 0.18, e2[1] - uy * 0.18)
        cv.line(k[0] - nx * thick / 2, k[1] - ny * thick / 2, k[0] + nx * thick / 2, k[1] + ny * thick / 2,
                "#d9d9d9", 0.1)
    for x, y in ((ax, ay), (bx, by)):
        cv.circle(x, y, 0.13, "#9a9a9a", "#555", 0.6)
    _part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), thick / 2 + 0.5, blen / 2)


def _part_label(cv: _Canvas, part: Part, centre, u, n, off: float, half: float) -> None:
    """A two-lead part's label: beside it on the upper side if that hides nothing, else the other side, else
    past one of its ends."""
    (cx, cy), (ux, uy), (nx, ny) = centre, u, n
    end = half + 0.35
    spots = [(cx + nx * off, cy + ny * off, "middle"), (cx - nx * off, cy - ny * off, "middle")]
    for k in (1, -1):
        x, y = cx + ux * end * k, cy + uy * end * k
        anchor = "middle" if abs(ux) < 0.5 else ("start" if ux * k > 0 else "end")
        spots.append((x, y + (0.35 if abs(ux) < 0.5 and uy * k > 0 else -0.35 if abs(ux) < 0.5 else 0), anchor))
    cv.label(spots, f"{part.ref} {part.value}".strip(), 6.2, part.ref, weight="bold", halo="#ffffff")


def _shade(hex_color: str, k: float) -> str:
    """A colour darkened (k < 1) or lightened toward white (k > 1)."""
    r, g, b = (int(hex_color[i:i + 2], 16) for i in (1, 3, 5))
    if k <= 1:
        r, g, b = (int(v * k) for v in (r, g, b))
    else:
        r, g, b = (int(v + (255 - v) * (k - 1)) for v in (r, g, b))
    return f"#{r:02x}{g:02x}{b:02x}"


LEG = "#c9cdd2"       # a part's tinned leg
LEG_EDGE = "#4d5257"  # and its outline, so it shows on the board and on a wire


def _legs(cv: _Canvas, a, b, mid) -> None:
    """The legs of a part seen from above, from its holes in under its body (drawn over them), so each meets
    the body wherever its edge is: an LED's flat is cut back from the round."""
    cx, cy = mid
    for x, y in (a, b):
        cv.line(x, y, cx, cy, LEG_EDGE, 0.17)
        cv.line(x, y, cx, cy, LEG, 0.09)


def _leg_ends(cv: _Canvas, a, b) -> None:
    """Where the legs go into the board: plain to see, so it's clear which strip each is in."""
    for x, y in (a, b):
        cv.circle(x, y, 0.14, LEG, LEG_EDGE, 1)


def _led(cv: _Canvas, part: Part, a, b, u, n, length: float) -> None:
    """An LED seen from above: its round lens inside the wider flange, with the flat cut into the flange on
    the cathode side (leg 2)."""
    (ax, ay), (bx, by), (ux, uy), (nx, ny) = a, b, u, n
    color = part.color or "red"
    lens, flange = LED_COLORS.get(color) or (_shade(color, 1.25), _shade(color, 0.7))
    cx, cy, r = (ax + bx) / 2, (ay + by) / 2, 0.75
    _legs(cv, a, b, (cx, cy))
    d = r * 0.78  # the flat: a chord across the flange, toward the cathode
    h = math.sqrt(r * r - d * d)
    p1 = (cx + ux * d + nx * h, cy + uy * d + ny * h)
    p2 = (cx + ux * d - nx * h, cy + uy * d - ny * h)
    sweep = 1 if ux * ny - uy * nx > 0 else 0  # the long way round, away from the flat, whichever way it faces
    cv.add(f'<path d="M {cv.X(p1[0]):.1f} {cv.Y(p1[1]):.1f} A {r * P:.1f} {r * P:.1f} 0 1 {sweep} {cv.X(p2[0]):.1f} '
           f'{cv.Y(p2[1]):.1f} Z" fill="{flange}" stroke="{_shade(flange, 0.6)}" stroke-width="1"/>')
    cv.circle(cx, cy, r * 0.72, lens, _shade(flange, 0.8), 0.8)
    cv.circle(cx - r * 0.25, cy - r * 0.25, r * 0.18, "#ffffff", "none", 0)  # the lens's shine
    _leg_ends(cv, a, b)
    _part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), r + 0.45, r)


def _can(cv: _Canvas, part: Part, a, b, u, n, length: float) -> None:
    """A radial electrolytic seen from above: black sleeve, silver top with its vent cross, and the
    white stripe on the side of the - leg (leg 2)."""
    (ax, ay), (bx, by), (ux, uy), (nx, ny) = a, b, u, n
    cx, cy, r = (ax + bx) / 2, (ay + by) / 2, 0.75
    _legs(cv, a, b, (cx, cy))
    cv.circle(cx, cy, r, part.color or "#1c1c1c", "#000000", 1)
    ang, spread = math.atan2(uy, ux), math.radians(40)
    x1, y1 = cx + r * math.cos(ang - spread), cy + r * math.sin(ang - spread)
    x2, y2 = cx + r * math.cos(ang + spread), cy + r * math.sin(ang + spread)
    cv.add(f'<path d="M {cv.X(cx):.1f} {cv.Y(cy):.1f} L {cv.X(x1):.1f} {cv.Y(y1):.1f} '
           f'A {r * P:.1f} {r * P:.1f} 0 0 1 {cv.X(x2):.1f} {cv.Y(y2):.1f} Z" fill="#ececec"/>')
    cv.circle(cx, cy, r * 0.7, "#c9cdd2", "#8e959c", 0.8)
    cv.text(cx + ux * r * 0.86, cy + uy * r * 0.86, "\u2212", 6, fill="#1c1c1c", weight="bold")
    arm = r * 0.42
    for dx, dy in ((1, 1), (1, -1)):
        k = arm / math.sqrt(2)
        cv.line(cx - dx * k, cy - dy * k, cx + dx * k, cy + dy * k, "#8e959c", 0.05)
    _leg_ends(cv, a, b)
    _part_label(cv, part, (cx, cy), (ux, uy), (nx, ny), r + 0.45, r)


def _offboard(cv: _Canvas, part: Part) -> None:
    x0, y0, x1, y1 = part.box
    cv.rect(x0, y0, x1, y1, "#2b2b2b", "#111", 1, rx=0.12)
    cv.text((x0 + x1) / 2, y0 - 0.7, part.title, 7, weight="bold")
    for pin in part.pins:
        x, y = pin.xy
        cv.rect(x - 0.18, y - 0.18, x + 0.18, y + 0.18, "#d9b44a", "#8a6d1c", 0.6)
        dx, dy = pin.label_dir
        warn = pin.name.endswith("!")
        fill, weight = ("#c62828", "bold") if warn else (INK, "normal")
        # the number goes next to the pin: first when the text starts at the pin
        if pin.name == str(pin.num):
            text = pin.name
        else:
            text = f"{pin.num} {pin.name}" if dx > 0 or (dx == 0 and dy < 0) else f"{pin.name} {pin.num}"
        # over the flying leads, which often cross these labels on their way in
        if dx == 0:  # clear of the housing, which reaches 0.6 past the pins
            cv.text(x, y + dy * 0.85, text, 6, "end" if dy > 0 else "start", fill, weight, vertical=True,
                    halo="#ffffff", late=True)
        else:
            cv.text(x + dx * 0.85, y, text, 6, "start" if dx > 0 else "end", fill, weight, halo="#ffffff", late=True)


def _wire_path(c: Circuit, w: Wire):
    pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
    if len(pts) > 2:
        d = "M " + " L ".join(f"{x:.2f} {y:.2f}" for x, y in pts)
        i = len(pts) // 2
        mid = ((pts[i - 1][0] + pts[i][0]) / 2, (pts[i - 1][1] + pts[i][1]) / 2)
        return d, pts, mid
    (ax, ay), (bx, by) = pts
    length = math.hypot(bx - ax, by - ay)
    # jumpers on the board lie straight; flying leads to off-board pins curve, and flexible jumpers
    # arch more, so even a short one reads as off the board
    off_board = w.a.pin is not None or w.b.pin is not None
    bend = 0.12 * length if off_board and length > 2 else min(max(0.18 * length, 0.8), 2.0) if w.flex else 0.0
    bend *= 1 if w.num % 2 else -1
    nx, ny = (-(by - ay) / length, (bx - ax) / length) if length else (0, 0)
    qx, qy = (ax + bx) / 2 + nx * bend, (ay + by) / 2 + ny * bend
    mid = (0.25 * ax + 0.5 * qx + 0.25 * bx, 0.25 * ay + 0.5 * qy + 0.25 * by)
    return (ax, ay, qx, qy, bx, by), pts, mid


def _wire_d(cv: _Canvas, c: Circuit, w: Wire):
    """The wire's SVG path, its points and the middle of its run (for its label)."""
    path, pts, mid = _wire_path(c, w)
    if isinstance(path, tuple):
        ax, ay, qx, qy, bx, by = path
        return (f"M {cv.X(ax):.1f} {cv.Y(ay):.1f} Q {cv.X(qx):.1f} {cv.Y(qy):.1f} "
                f"{cv.X(bx):.1f} {cv.Y(by):.1f}"), pts, mid
    return "M " + " L ".join(f"{cv.X(x):.1f} {cv.Y(y):.1f}" for x, y in pts), pts, mid


FOCUS = "#ff8c00"


def _focus(cv: _Canvas, c: Circuit, focus) -> None:
    """Ring what a step changed: parts by ref, wires by W<num>."""
    boxes = {ref: box for box, ref in c._bodies}
    for name in focus:
        part = c.parts.get(name)
        if part is not None:
            if name in boxes:
                x0, y0, x1, y1 = boxes[name]
            elif part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
                xs, ys = [p.xy[0] for p in part.pins], [p.xy[1] for p in part.pins]
                x0, y0, x1, y1 = min(xs), min(ys), max(xs), max(ys)
            elif part.extent:
                x0, y0, x1, y1 = part.extent
            else:
                continue
            cv.rect(x0 - 0.6, y0 - 0.6, x1 + 0.6, y1 + 0.6, "none", FOCUS, 3, rx=0.4)
            continue
        for w in c.wires:
            if f"W{w.num}" == name and w.a and w.b:
                d, _, _ = _wire_d(cv, c, w)
                cv.add(f'<path d="{d}" fill="none" stroke="{FOCUS}" stroke-opacity="0.45" '
                       f'stroke-width="{0.8 * P:.1f}" stroke-linecap="round" stroke-linejoin="round"/>')


def _along(c: Circuit, w: Wire, t: float):
    """The point a fraction t along a wire as drawn (its curve, for a flying lead or jumper)."""
    path, pts, _ = _wire_path(c, w)
    if isinstance(path, tuple):
        ax, ay, qx, qy, bx, by = path
        return ((1 - t) ** 2 * ax + 2 * (1 - t) * t * qx + t * t * bx, (1 - t) ** 2 * ay + 2 * (1 - t) * t * qy + t * t * by)
    lengths = [math.dist(p, q) for p, q in zip(pts, pts[1:])]
    goal = t * sum(lengths)
    for (p, q), d in zip(zip(pts, pts[1:]), lengths):
        if goal <= d or d == lengths[-1]:
            k = goal / d if d else 0.0
            return p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k
        goal -= d
    return pts[-1]


def _wires(cv: _Canvas, c: Circuit, result: Result, highlight: str | None) -> None:
    drawn = []
    # flexible jumpers over the wires lying on the board, as they're plugged in last
    for w in sorted((w for w in c.wires if w.a and w.b), key=lambda w: (highlight is not None and w.group == highlight,
                                                                        w.flex)):
        dim = highlight is not None and w.group != highlight
        color = color_hex(wire_color(c, result, w))
        d, pts, mid = _wire_d(cv, c, w)
        cv.add(f'<g opacity="{0.14 if dim else 1}">')
        for stroke, width in ((_outline(color), 0.34), (color, 0.24)):
            cv.add(f'<path d="{d}" fill="none" stroke="{stroke}" stroke-width="{width * P:.1f}" '
                   f'stroke-linecap="round" stroke-linejoin="round"/>')
        for x, y in (pts[0], pts[-1]):
            if w.flex:  # its plug's housing
                cv.rect(x - 0.2, y - 0.2, x + 0.2, y + 0.2, "#2b2b2b", color, 1.6, rx=0.05)
            else:
                cv.circle(x, y, 0.15, color, _outline(color), 1)
        cv.add("</g>")
        if not dim:
            drawn.append((w, mid, color))
    for w, (x, y), color in drawn:
        label = f"W{w.num}"
        half = (len(label) * 5.2 * 0.62 + 4) / 2 / P
        spots = [(x, y)] + [_along(c, w, t) for t in (0.3, 0.7, 0.2, 0.8, 0.4, 0.6, 0.1, 0.9)]
        # last resort: just beside the middle, off to either side of the wire
        a, b = _along(c, w, 0.45), _along(c, w, 0.55)
        side = (0.0, 0.5) if abs(b[0] - a[0]) >= abs(b[1] - a[1]) else (half + 0.3, 0.0)
        spots += [(x + side[0], y + side[1]), (x - side[0], y - side[1])]

        def clash(p):
            return cv.clash((p[0] - half, p[1] - 0.24, p[0] + half, p[1] + 0.24), label)
        # beside the wire only to get clear of something: it reads as the wire's tag less well
        x, y = min(spots, key=lambda p: (clash(p) + 0.5 * (spots.index(p) > 8), spots.index(p)))
        cv.rect(x - half, y - 0.24, x + half, y + 0.24, "#ffffff", color, 1, rx=0.1, extra=' opacity="0.92"')
        cv.labels.append(((x - half, y - 0.24, x + half, y + 0.24), label))
        cv.text(x, y, label, 5.2, fill=INK, weight="bold")


def _marks(cv: _Canvas, c: Circuit, result: Result) -> None:
    for issue in result.issues:
        color = "#e01b1b" if issue.level == "error" else "#f08a00"
        spots = [p.xy for p in issue.pins if p.xy] + [c.boards[h.board].xy(h) for h in issue.holes if h]
        for x, y in spots:
            cv.circle(x, y, 0.5, "none", color, 2.2)


def render(c: Circuit, result: Result, highlight: str | None = None, caption: str | None = None,
           focus=()) -> str:
    """The drawing; highlight dims every wire group but that one. caption and focus are for the frames of
    a video of bbgen at work: a line saying what the step did, and the parts and wires it changed."""
    return draw(c, result, highlight, caption, focus)[0]


def draw(c: Circuit, result: Result, highlight: str | None = None, caption: str | None = None, focus=()):
    """The drawing's SVG, and the canvas it was drawn on (whose labels the measures look at)."""
    xs = [x for b in c.boards.values() for x in (b.left, b.right)]
    ys = [y for b in c.boards.values() for y in (b.top, b.bottom)]
    for part in c.parts.values():
        if part.extent:
            xs += [part.extent[0], part.extent[2]]
            ys += [part.extent[1], part.extent[3]]
    for box, _ in [*c._bodies, *c._raised]:  # a module hanging over the board's edge, or its keep-out
        xs += [box[0], box[2]]
        ys += [box[1], box[3]]
    minx, maxx, miny, maxy = min(xs) - 1, max(xs) + 1, min(ys) - HEADER, max(ys) + 1
    cv = _Canvas(minx, miny)
    legend_y = maxy + 0.6
    # the legend, wrapped to the drawing's width: (x, line, kind, text) for each entry
    entries = [("net", net.name) for net in c.nets.values()]
    if any(w.flex for w in c.wires):
        entries.append(("flex", "flexible jumper (plug in last)"))
    legend, x, line = [], minx + 0.8, 0
    for kind, text in entries:
        size = 1.4 + len(text) * 0.24 + 1.2
        if x + size > maxx and x > minx + 0.8:
            x, line = minx + 0.8, line + 1
        legend.append((x, line, kind, text))
        x += size
    width, height = (maxx - minx) * P, (legend_y + 1.6 + 0.8 * line - miny) * P

    errors, warnings = len(result.errors), len(result.warnings)
    status = ("wiring check passed" if not errors else f"{errors} error{'s' * (errors != 1)}") + (
        f", {warnings} warning{'s' * (warnings != 1)}" if warnings else "")
    sub = f"{len(c.parts)} parts · {sum(1 for w in c.wires)} wires · {status}"
    if highlight:
        sub += f" · step: {highlight}"
    cv.text(minx + 0.8, miny + 1.1, c.title, 15, "start", weight="bold")
    cv.text(minx + 0.8, miny + 2.3, sub, 8, "start", "#c62828" if errors else "#555")
    if caption:
        cv.text(maxx - 0.8, miny + 1.1, caption, 11, "end", "#b35c00", weight="bold")

    for w in c.wires:
        if w.a and w.b:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            cv.spots += [pts[0], pts[-1], *w.via_xy]
            cv.segs += [(p, q, f"W{w.num}") for p, q in zip(pts, pts[1:])]
    for board in c.boards.values():
        _board(cv, c, result, board)
    for part in c.parts.values():
        _keepouts(cv, c, part)
    for part in c.parts.values():
        if part.kind in FOOTPRINT_KINDS and not part.raised:
            _footprint(cv, part)
        elif part.kind == "offboard":
            _offboard(cv, part)
    for part in c.parts.values():
        if part.kind in LEAD_KINDS:
            _lead(cv, part)
    _wires(cv, c, result, highlight)
    for part in c.parts.values():  # up on its headers, over the wires laid under it: see-through, to show them
        if part.kind in FOOTPRINT_KINDS and part.raised:
            cv.add('<g opacity="0.78">')
            _footprint(cv, part)
            cv.add("</g>")
    _focus(cv, c, focus)
    cv.out += cv.late
    _marks(cv, c, result)

    colors = {net.name: color_hex(net.color) for net in c.nets.values()}
    for x, line, kind, text in legend:
        y = legend_y + 0.8 * line
        if kind == "net":
            swatch = colors[text]
            cv.rect(x, y - 0.25, x + 0.9, y + 0.25, swatch, _outline(swatch), 1, rx=0.1)
        else:  # an arch between two plugs: what the square ends mean
            cv.add(f'<path d="M {cv.X(x + 0.2):.1f} {cv.Y(y):.1f} Q {cv.X(x + 0.6):.1f} {cv.Y(y - 0.7):.1f} '
                   f'{cv.X(x + 1.0):.1f} {cv.Y(y):.1f}" fill="none" stroke="#777777" stroke-width="{0.2 * P:.1f}"/>')
            for gx in (x + 0.2, x + 1.0):
                cv.rect(gx - 0.2, y - 0.2, gx + 0.2, y + 0.2, "#2b2b2b", "#777777", 1.6, rx=0.05)
        cv.text(x + 1.4, y, text, 6.5, "start")

    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{height:.0f}" '
            f'viewBox="0 0 {width:.0f} {height:.0f}" font-family="{FONT}">\n'
            f'<defs><pattern id="keepout" width="8" height="8" patternUnits="userSpaceOnUse" '
            f'patternTransform="rotate(45)"><rect width="8" height="8" fill="#efe9dc"/>'
            f'<line x1="0" y1="0" x2="0" y2="8" stroke="#d3c9b1" stroke-width="3"/></pattern></defs>\n'
            f'<rect width="100%" height="100%" fill="#ffffff"/>\n' + "\n".join(cv.out) + "\n</svg>\n"), cv
