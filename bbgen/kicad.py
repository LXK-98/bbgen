"""Reading parts from KiCad's libraries: a symbol's pins, its footprint's pads, outline and keep-outs.

Only what bbgen needs is read, and only the symbol or footprint asked for: the library's symbol
files run to megabytes. Results are cached, since placement builds a circuit many times over.
The library is looked for where KiCad installs it (system packages, Flatpak, macOS, Windows) and
where its environment variables point; $BBGEN_KICAD or a circuit's `kicad:` names it outright.
"""

from __future__ import annotations

import glob
import os
import re
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

MM = 2.54  # one hole pitch

_TOKEN = re.compile(r'\(|\)|"(?:\\.|[^"\\])*"|[^\s()"]+')


class KicadError(ValueError):
    pass


@dataclass
class SymbolPin:
    number: str
    name: str        # "" when KiCad leaves it unnamed ("~")
    kind: str        # KiCad's electrical type: power_in, input, passive, ...


@dataclass
class Footprint:
    name: str
    pads: dict[str, tuple[float, float]]           # through-hole pad number -> (x, y) in mm
    smd: bool                                        # it has surface-mount pads only
    outline: tuple[float, float, float, float] | None  # fabrication (else courtyard) bounding box, mm
    keepouts: list[tuple[str, tuple[float, float, float, float]]] = field(default_factory=list)


@dataclass
class Symbol:
    name: str
    pins: list[SymbolPin]
    footprint: str   # "Lib:Name", or "" when the symbol names none
    description: str


# ---- finding the library ----

def library_roots(extra: str | None = None) -> list[Path]:
    """Folders holding KiCad's `symbols/` and `footprints/`, most specific first."""
    return list(_roots(extra, os.environ.get("BBGEN_KICAD"), tuple(sorted(os.environ.items()))))


@lru_cache(maxsize=8)
def _roots(extra, env_kicad, _environ) -> tuple[Path, ...]:
    found: list[Path] = []
    for base in (extra, env_kicad):
        if base:
            found.append(Path(base).expanduser())
    for var in sorted(os.environ):  # KICAD9_SYMBOL_DIR, KICAD10_FOOTPRINT_DIR, ...
        if re.fullmatch(r"KICAD\d*_(SYMBOL|FOOTPRINT)_DIR", var):
            found.append(Path(os.environ[var]).parent)
    patterns = [
        "/var/lib/flatpak/runtime/org.kicad.kicad.Library/*/stable/*/files",
        os.path.expanduser("~/.local/share/flatpak/runtime/org.kicad.kicad.Library/*/stable/*/files"),
        "/usr/share/kicad", "/usr/local/share/kicad",
        "/Applications/KiCad/KiCad.app/Contents/SharedSupport",
        "C:/Program Files/KiCad/*/share/kicad",
    ]
    for pattern in patterns:
        found += [Path(p) for p in sorted(glob.glob(pattern), reverse=True)]
    roots = []
    for root in found:
        if (root / "symbols").is_dir() or (root / "footprints").is_dir():
            root = root.resolve()  # Flatpak's "active" is a link to one of the others
            if root not in roots:
                roots.append(root)
    return tuple(roots)


def clean_name(name: str) -> str:
    """A KiCad pin name without its markup: ~{RST} (an overbar) is /RST, V_{IN} is VIN."""
    name = re.sub(r"~\{([^}]*)\}", r"/\1", name)
    return re.sub(r"[_^]\{([^}]*)\}", r"\1", name).strip()


def _find(roots: list[Path], kind: str, lib: str) -> Path:
    suffix = {"symbols": ".kicad_sym", "footprints": ".pretty"}[kind]
    for root in roots:
        path = root / kind / f"{lib}{suffix}"
        if path.exists():
            return path
    where = ", ".join(str(r) for r in roots) or "nowhere: no KiCad library found (set BBGEN_KICAD)"
    raise KicadError(f"no KiCad {kind[:-1]} library {lib!r} (looked in {where})")


def _split(ref: str, what: str) -> tuple[str, str]:
    lib, sep, name = ref.partition(":")
    if not sep or not lib or not name:
        raise KicadError(f"a KiCad {what} is named Library:Name, like Timer:NE555P, not {ref!r}")
    return lib, name


# ---- S-expressions ----

def _parse(text: str, start: int = 0):
    """The one S-expression starting at text[start] (an opening parenthesis), as nested lists."""
    stack: list[list] = []
    for m in _TOKEN.finditer(text, start):
        tok = m.group()
        if tok == "(":
            stack.append([])
        elif tok == ")":
            done = stack.pop()
            if not stack:
                return done
            stack[-1].append(done)
        else:
            if tok.startswith('"'):
                tok = tok[1:-1].replace('\\"', '"').replace("\\\\", "\\")
            stack[-1].append(tok)
    raise KicadError("unbalanced parentheses in a KiCad file")


def _children(node: list, head: str):
    return [c for c in node if isinstance(c, list) and c and c[0] == head]


def _child(node: list, head: str):
    found = _children(node, head)
    return found[0] if found else None


# ---- symbols ----

@lru_cache(maxsize=None)
def _symbol_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _symbol_node(text: str, name: str):
    m = re.search(r'^\t?\(symbol "' + re.escape(name) + r'"', text, re.M)
    return _parse(text, text.index("(", m.start())) if m else None


@lru_cache(maxsize=None)
def symbol(ref: str, roots: tuple[Path, ...]) -> Symbol:
    lib, name = _split(ref, "symbol")
    text = _symbol_text(_find(list(roots), "symbols", lib))
    node = _symbol_node(text, name)
    if node is None:
        raise KicadError(f"no symbol {name!r} in KiCad's {lib} library")
    props = {p[1]: p[2] for p in _children(node, "property") if len(p) > 2}
    base = node
    seen = {name}
    while _child(base, "extends"):  # a derived symbol: its pins are its base's
        parent = _child(base, "extends")[1]
        if parent in seen:
            raise KicadError(f"symbol {name!r} extends itself")
        seen.add(parent)
        base = _symbol_node(text, parent)
        if base is None:
            raise KicadError(f"symbol {name!r} extends {parent!r}, which isn't in {lib}")
        props = {**{p[1]: p[2] for p in _children(base, "property") if len(p) > 2}, **props}
    pins: dict[str, SymbolPin] = {}
    for unit in [base, *_children(base, "symbol")]:
        for pin in _children(unit, "pin"):
            number, label = _child(pin, "number"), _child(pin, "name")
            if number is None:
                continue
            label = label[1] if label else ""
            pins.setdefault(number[1], SymbolPin(number[1], "" if label == "~" else label, pin[1]))
    return Symbol(name, list(pins.values()), props.get("Footprint", ""), props.get("Description", ""))


# ---- footprints ----

def _box(points) -> tuple[float, float, float, float] | None:
    if not points:
        return None
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    return min(xs), min(ys), max(xs), max(ys)


def _layer_points(node: list, layer: str) -> list[tuple[float, float]]:
    points = []
    for item in node:
        if not (isinstance(item, list) and item and item[0].startswith("fp_")):
            continue
        lay = _child(item, "layer")
        if not lay or lay[1] != layer:
            continue
        for key in ("start", "end", "center", "mid"):
            p = _child(item, key)
            if p:
                points.append((float(p[1]), float(p[2])))
        pts = _child(item, "pts")
        if pts:
            points += [(float(xy[1]), float(xy[2])) for xy in _children(pts, "xy")]
    return points


@lru_cache(maxsize=None)
def footprint(ref: str, roots: tuple[Path, ...]) -> Footprint:
    lib, name = _split(ref, "footprint")
    path = _find(list(roots), "footprints", lib) / f"{name}.kicad_mod"
    if not path.exists():
        raise KicadError(f"no footprint {name!r} in KiCad's {lib} library")
    text = path.read_text(encoding="utf-8")
    node = _parse(text, text.index("("))
    pads: dict[str, tuple[float, float]] = {}
    smd = 0
    for pad in _children(node, "pad"):
        number, kind = pad[1], pad[2]
        at = _child(pad, "at")
        if kind == "smd":
            smd += 1
        elif kind == "thru_hole" and number and at:
            pads.setdefault(number, (float(at[1]), float(at[2])))
    outline = _box(_layer_points(node, "F.Fab")) or _box(_layer_points(node, "F.CrtYd"))  # the body, not its clearance
    keepouts = []
    for zone in _children(node, "zone"):
        layers = [_child(zone, "layer")] + _children(zone, "layers")
        named = _child(zone, "name")
        if named and any(lay and "CrtYd" in " ".join(lay[1:]) for lay in layers if lay):
            polygon = _child(zone, "polygon")
            pts = _child(polygon, "pts") if polygon else None
            box = _box([(float(xy[1]), float(xy[2])) for xy in _children(pts, "xy")]) if pts else None
            if box:
                keepouts.append((named[1], box))
    return Footprint(name, pads, smd > 0 and not pads, outline, keepouts)


# ---- searching ----

_SYMBOL_START = re.compile(r'^\t\(symbol "([^"]+)"', re.M)
_PROPERTY = re.compile(r'\(property "(Description|ki_keywords|Footprint)" "((?:\\.|[^"\\])*)"')


def search(words: list[str], roots: list[Path], limit: int = 25) -> list[tuple[str, str, str]]:
    """(Library:Name, footprint, description) for symbols whose name, description or keywords hold every
    word, names that hold them first."""
    words = [w.lower() for w in words if w]
    seen: set[str] = set()
    hits = []
    for root in roots:
        for path in sorted((root / "symbols").glob("*.kicad_sym")):
            lib = path.stem
            text = path.read_text(encoding="utf-8")
            starts = list(_SYMBOL_START.finditer(text))
            for i, m in enumerate(starts):
                ref = f"{lib}:{m.group(1)}"
                if ref in seen:
                    continue
                head = text[m.start():starts[i + 1].start() if i + 1 < len(starts) else len(text)][:6000]
                props = {k: v for k, v in _PROPERTY.findall(head)}
                about = f"{ref} {props.get('Description', '')} {props.get('ki_keywords', '')}".lower()
                if all(w in about for w in words):
                    seen.add(ref)
                    in_name = sum(w in ref.lower() for w in words)
                    hits.append((-in_name, ref, props.get("Footprint", ""), props.get("Description", "")))
    hits.sort()
    return [(ref, fp, about) for _, ref, fp, about in hits[:limit]]
