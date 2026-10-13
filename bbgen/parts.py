"""Parts, their pins, and where those pins sit relative to pin 1.

Footprint parts (pico, dip, sip) are placed by the hole of pin 1 plus a rotation.
Two-lead parts (resistor, capacitor, ...) name a hole, rail or part pin for each
leg. Off-board parts are connectors elsewhere, drawn beside the breadboard and
reached with flying leads.

Offsets are in grid units: x along the columns, y down the rows. Unrotated, a DIP
has pin 1 bottom-left with its notch to the left, and a Pico has its USB to the
left with pins 1-20 along the bottom.
"""

from __future__ import annotations

from dataclasses import dataclass, field

PICO_PINS = {
    1: "GP0", 2: "GP1", 3: "GND", 4: "GP2", 5: "GP3", 6: "GP4", 7: "GP5", 8: "GND",
    9: "GP6", 10: "GP7", 11: "GP8", 12: "GP9", 13: "GND", 14: "GP10", 15: "GP11",
    16: "GP12", 17: "GP13", 18: "GND", 19: "GP14", 20: "GP15", 21: "GP16", 22: "GP17",
    23: "GND", 24: "GP18", 25: "GP19", 26: "GP20", 27: "GP21", 28: "GND", 29: "GP22",
    30: "RUN", 31: "GP26", 32: "GP27", 33: "AGND", 34: "GP28", 35: "ADC_VREF",
    36: "3V3", 37: "3V3_EN", 38: "GND", 39: "VSYS", 40: "VBUS",
}
# Joined on the Pico's own board. AGND (33) is left out: wire it if you use it.
PICO_GND = (3, 8, 13, 18, 23, 28, 38)
# Room for a USB cable's plug past the Pico's USB end, relative to pin 1: the "USB Cable" courtyard zone
# of KiCad's Module:RaspberryPi_Pico_Common_THT footprint (x 3.14-14.64 mm, y -3.57 to -23.07 mm).
PICO_USB_KEEPOUT = (-9.08, -5.76, -1.41, -1.24)

LEAD_KINDS = {
    "resistor": ("1", "2"),
    "capacitor": ("1", "2"),
    "electrolytic": ("+", "-"),
    "diode": ("A", "K"),
    "led": ("A", "K"),
}
FOOTPRINT_KINDS = ("pico", "dip", "sip")
KINDS = (*FOOTPRINT_KINDS, *LEAD_KINDS, "offboard")


class PartError(ValueError):
    pass


def rotate(v: tuple[float, float], rot: int) -> tuple[float, float]:
    """Rotate an offset clockwise on screen by rot degrees (a multiple of 90)."""
    x, y = v
    for _ in range(rot // 90 % 4):
        x, y = -y, x
    return x, y


@dataclass(eq=False)
class Pin:
    part: Part
    num: int
    name: str
    offset: tuple[int, int] = (0, 0)
    label_dir: tuple[int, int] = (0, -1)
    hole: object = None  # board.Hole once on the board
    xy: tuple[float, float] | None = None

    def __str__(self) -> str:
        ref = f"{self.part.ref}.{self.num}"
        return ref if self.name == str(self.num) else f"{ref} {self.name}"


@dataclass(eq=False)
class Part:
    ref: str
    kind: str
    title: str = ""
    value: str = ""
    color: str | None = None
    pins: list[Pin] = field(default_factory=list)
    buses: list[tuple[Pin, ...]] = field(default_factory=list)
    at: str | None = None    # footprint parts: hole of pin 1, as written
    rot: int = 0
    width: int = 0           # dip: row spacing
    legs: list[str] = field(default_factory=list)  # two-lead parts: where each leg goes
    slant: str | None = None  # two-lead parts: why it may lie slanted
    near: str | None = None   # a part to keep it close to, when bbgen places it
    auto: bool = False        # placed by bbgen, not the file
    keepouts: list = field(default_factory=list)  # (what, rect relative to pin 1): room it needs that isn't body
    cols: int = 1            # off-board layout
    numbering: str = "row"
    place: str = "right"
    box: tuple | None = None     # off-board: housing outline
    extent: tuple | None = None  # off-board: outline including labels and title

    def find(self, token: str) -> list[Pin]:
        token = token.strip()
        if token.isdigit():
            return [p for p in self.pins if p.num == int(token)]
        exact = [p for p in self.pins if p.name == token]
        return exact or [p for p in self.pins if p.name.lower() == token.lower()]

    def bus_of(self, pin: Pin) -> tuple[Pin, ...] | None:
        return next((bus for bus in self.buses if pin in bus), None)


def body_rect(part: Part) -> tuple[float, float, float, float] | None:
    """The part's outline relative to pin 1, unrotated. Holes under it can't be used."""
    n = len(part.pins)
    if part.kind == "pico":  # 21 x 51 mm
        return (-0.54, -7.64, 19.54, 0.64)
    if part.kind == "dip":
        return (-0.5, -part.width - 0.5, n // 2 - 0.5, 0.5)
    if part.kind == "sip":
        return (-0.45, -0.45, n - 0.55, 0.45)
    return None


def placed_box(part: Part, rect) -> tuple[float, float, float, float]:
    """A rect given relative to pin 1 in the part's own frame, rotated and placed."""
    ax, ay = part.pins[0].xy
    corners = [rotate((x, y), part.rot) for x in (rect[0], rect[2]) for y in (rect[1], rect[3])]
    xs, ys = [ax + x for x, _ in corners], [ay + y for _, y in corners]
    return min(xs), min(ys), max(xs), max(ys)


def _count(spec: dict, ref: str) -> int:
    n = spec.get("pins")
    if n is None and spec.get("type") == "offboard":
        labels = spec.get("labels") or []
        n = len(labels) if isinstance(labels, list) else max(map(int, labels), default=0)
    if not isinstance(n, int) or n < 1:
        raise PartError(f"parts.{ref}: needs pins: <count>")
    return n


def _labels(spec: dict, n: int, ref: str) -> list[str]:
    raw = spec.get("labels") or {}
    if isinstance(raw, list):
        if len(raw) != n:
            raise PartError(f"parts.{ref}: {len(raw)} labels for {n} pins")
        names = {i + 1: v for i, v in enumerate(raw)}
    elif isinstance(raw, dict):
        names = {int(k): v for k, v in raw.items()}
        if any(not 1 <= k <= n for k in names):
            raise PartError(f"parts.{ref}: label for a pin outside 1..{n}")
    else:
        raise PartError(f"parts.{ref}: labels must be a list or a {{pin: name}} mapping")
    return [str(names[k]) if names.get(k) not in (None, "") else str(k) for k in range(1, n + 1)]


def slant_reason(spec: dict, where: str) -> str | None:
    """The reason given for a slanted wire or part (`slant: <reason>`), which silences DIAGONAL."""
    reason = spec.get("slant")
    if reason is None:
        return None
    if not isinstance(reason, str) or not reason.strip():
        raise PartError(f"{where}: slant takes the reason it can't be straight, like slant: crosses the trench")
    return reason.strip()


def make_part(ref: str, spec: dict) -> Part:
    if not isinstance(spec, dict):
        raise PartError(f"parts.{ref}: expected a mapping")
    kind = spec.get("type")
    if kind not in KINDS:
        raise PartError(f"parts.{ref}: unknown type {kind!r} (expected one of {', '.join(KINDS)})")
    part = Part(ref, kind, str(spec.get("title") or ""), str(spec.get("value") or ""), spec.get("color"))

    part.near = str(spec["near"]) if spec.get("near") else None
    if kind in FOOTPRINT_KINDS:
        part.at = spec.get("at")  # None: bbgen places it
        part.rot = int(spec.get("rot", 0))
        if part.rot not in (0, 90, 180, 270):
            raise PartError(f"parts.{ref}: rot must be 0, 90, 180 or 270")

    if kind == "pico":
        part.title = part.title or "Raspberry Pi Pico"
        for n, name in PICO_PINS.items():
            bottom = n <= 20
            part.pins.append(Pin(part, n, name, (n - 1, 0) if bottom else (40 - n, -7),
                                 (0, -1) if bottom else (0, 1)))
        part.buses.append(tuple(part.pins[n - 1] for n in PICO_GND))
        part.keepouts.append(("USB cable", PICO_USB_KEEPOUT))
    elif kind == "dip":
        n = _count(spec, ref)
        if n % 2:
            raise PartError(f"parts.{ref}: a dip needs an even pin count")
        part.width = int(spec.get("width", 3))
        names = _labels(spec, n, ref)
        for k in range(1, n + 1):
            if k <= n // 2:
                part.pins.append(Pin(part, k, names[k - 1], (k - 1, 0), (0, -1)))
            else:
                part.pins.append(Pin(part, k, names[k - 1], (n - k, -part.width), (0, 1)))
    elif kind == "sip":
        n = _count(spec, ref)
        names = _labels(spec, n, ref)
        part.pins = [Pin(part, k, names[k - 1], (k - 1, 0), (0, -1)) for k in range(1, n + 1)]
    elif kind in LEAD_KINDS:
        legs = spec.get("pins")
        if legs is not None and not (isinstance(legs, list) and len(legs) == 2):
            raise PartError(f"parts.{ref}: pins is [<leg 1>, <leg 2>] (holes, rails or part pins), or leave it out "
                            "and bbgen places the legs from the nets")
        part.legs = [str(x) for x in legs or []]  # none: bbgen places them
        part.slant = slant_reason(spec, f"parts.{ref}")
        names = spec.get("labels") or LEAD_KINDS[kind]
        part.pins = [Pin(part, k + 1, str(names[k])) for k in range(2)]
    else:  # offboard
        n = _count(spec, ref)
        names = _labels(spec, n, ref)
        part.title = part.title or ref
        part.cols = int(spec.get("cols", 1))
        part.numbering = str(spec.get("numbering", "row"))
        part.place = str(spec.get("place", "right"))
        if part.numbering not in ("row", "column") or part.place not in ("right", "below"):
            raise PartError(f"parts.{ref}: numbering is row|column and place is right|below")
        part.pins = [Pin(part, k, names[k - 1]) for k in range(1, n + 1)]
    return part
