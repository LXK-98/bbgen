"""SVG drawing of the boards, the parts on them, the wires and any problems."""

from __future__ import annotations

import math
import re
from xml.sax.saxutils import escape

from .board import OUTLINE_Y, RAIL_PAIR_Y, RAILS, ROW_Y, TRENCH_Y, Board
from .check import Result, end_node, wire_color
from .circuit import Circuit, Wire, color_hex
from .parts import FOOTPRINT_KINDS, LEAD_KINDS, Part, body_rect, placed_box, rotate

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


class _Canvas:
    def __init__(self, x0: float, y0: float):
        self.x0, self.y0, self.out = x0, y0, []
        self.late: list[str] = []  # part labels, drawn over the wires

    def X(self, gx: float) -> float:
        return (gx - self.x0) * P

    def Y(self, gy: float) -> float:
        return (gy - self.y0) * P

    def add(self, svg: str) -> None:
        self.out.append(svg)

    def text(self, gx, gy, s, size=7.0, anchor="middle", fill=INK, weight="normal", vertical=False,
             halo=None, late=False) -> None:
        """Text centred across its height on (gx, gy); vertical text reads bottom to top."""
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
    cv.rect(b.left, OUTLINE_Y[0], b.right, OUTLINE_Y[1], BOARD, "#d5cdbb", 1.5, rx=0.5)
    last = b.x0 + b.columns - 1
    cv.rect(b.x0 - 0.9, TRENCH_Y[0], last + 0.9, TRENCH_Y[1], "#e7e1d3", rx=0.2)
    if len(c.boards) > 1:
        cv.text(b.left + 0.4, OUTLINE_Y[0] + 0.5, f"board {b.id}", 6.5, "start", MUTED, "bold")
    for rail in RAILS:
        y = b.rail_y(rail)
        upper = y == RAIL_PAIR_Y[rail[:3]][0]
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
    for row, y in ROW_Y.items():
        cv.text(b.x0 - 1.1, y, row, 6.5, fill=MUTED)
        cv.text(last + 1.1, y, row, 6.5, fill=MUTED)
    for col in [1, *range(5, b.columns + 1, 5)]:
        cv.text(b.x0 + col - 1, 2.05, str(col), 5.5, fill=MUTED)
        cv.text(b.x0 + col - 1, 14.95, str(col), 5.5, fill=MUTED)


def _keepouts(cv: _Canvas, c: Circuit, part: Part) -> None:
    """Room a part needs beyond its body (a USB cable's plug), hatched, as far as it lies on a board."""
    if not part.keepouts or part.pins[0].xy is None or part.pins[0].hole is None:
        return
    board = c.boards[part.pins[0].hole.board]
    for what, rect in part.keepouts:
        x0, y0, x1, y1 = placed_box(part, rect)
        x0, x1 = max(x0, board.left + 0.3), min(x1, board.right - 0.3)
        y0, y1 = max(y0, OUTLINE_Y[0] + 0.3), min(y1, OUTLINE_Y[1] - 0.3)
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
    elif part.kind == "sip":
        cv.text((x0 + x1) / 2, y1 + 0.55, label, 6.5, weight="bold", halo="#ffffff", late=True)
    else:
        cv.text((x0 + x1) / 2, y0 - 0.5, label, 6.5, weight="bold", halo="#ffffff", late=True)


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
    off = thick / 2 + 0.5
    cv.text(cx + nx * off, cy + ny * off, f"{part.ref} {part.value}".strip(), 6.2, weight="bold", halo="#ffffff",
            late=True)


def _can(cv: _Canvas, part: Part, a, b, u, n, length: float) -> None:
    """A radial electrolytic seen from above: black sleeve, silver top with its vent cross, and the
    white stripe on the side of the - leg (leg 2)."""
    (ax, ay), (bx, by), (ux, uy), (nx, ny) = a, b, u, n
    cx, cy, r = (ax + bx) / 2, (ay + by) / 2, 0.75
    if length > 2 * r:  # legs reach past the can
        cv.line(ax, ay, cx - ux * r, cy - uy * r, "#8d8d8d", 0.1)
        cv.line(bx, by, cx + ux * r, cy + uy * r, "#8d8d8d", 0.1)
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
    for x, y in (a, b):  # where the legs go, under the can
        cv.circle(x, y, 0.12, "none", "#6b6b6b", 1)
    off = r + 0.45
    cv.text(cx + nx * off, cy + ny * off, f"{part.ref} {part.value}".strip(), 6.2, weight="bold", halo="#ffffff",
            late=True)


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
        cv.rect(x - half, y - 0.24, x + half, y + 0.24, "#ffffff", color, 1, rx=0.1, extra=' opacity="0.92"')
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
    xs = [x for b in c.boards.values() for x in (b.left, b.right)]
    ys = list(OUTLINE_Y)
    for part in c.parts.values():
        if part.extent:
            xs += [part.extent[0], part.extent[2]]
            ys += [part.extent[1], part.extent[3]]
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

    for board in c.boards.values():
        _board(cv, c, result, board)
    for part in c.parts.values():
        _keepouts(cv, c, part)
    for part in c.parts.values():
        if part.kind in FOOTPRINT_KINDS:
            _footprint(cv, part)
        elif part.kind == "offboard":
            _offboard(cv, part)
    for part in c.parts.values():
        if part.kind in LEAD_KINDS:
            _lead(cv, part)
    _wires(cv, c, result, highlight)
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
            f'<rect width="100%" height="100%" fill="#ffffff"/>\n' + "\n".join(cv.out) + "\n</svg>\n")
