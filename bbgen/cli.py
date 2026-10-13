"""bbgen command line: build (drawing + checklist + check), check, and find parts in KiCad's library."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

import yaml

from . import __version__
from .check import check
from .checklist import checklist
from .circuit import Circuit, CircuitError
from .render import render


def _report(result) -> None:
    for issue in result.issues:
        print(f"{issue.level:<8}{issue.code:<13}{issue.message}")
    e, w = len(result.errors), len(result.warnings)
    print(f"check: {e} error{'s' * (e != 1)}, {w} warning{'s' * (w != 1)}")


def _png(svg: Path) -> Path | None:
    png = svg.with_suffix(".png")
    if shutil.which("magick"):
        cmd = ["magick", "-density", "144", "-background", "white", str(svg), str(png)]
    elif shutil.which("rsvg-convert"):
        cmd = ["rsvg-convert", "--zoom", "1.5", "-b", "white", "-o", str(png), str(svg)]
    else:
        print("bbgen: no PNG written (needs ImageMagick or rsvg-convert)", file=sys.stderr)
        return None
    subprocess.run(cmd, check=True)
    return png


def _find(words: list[str], where: str | None) -> int:
    from .kicad import library_roots, search
    roots = library_roots(where)
    if not roots:
        print("bbgen: no KiCad library found; install KiCad or point BBGEN_KICAD at it", file=sys.stderr)
        return 2
    hits = search(words, roots)
    for ref, fp, about in hits:
        print(f"{ref:<44} {fp or '(no footprint: give one)':<44} {about[:70]}")
    if not hits:
        print("nothing found; try fewer or shorter words")
    return 0 if hits else 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="bbgen", description="Breadboard layouts from YAML: a drawing, "
                                                          "a build checklist and a wiring check.")
    ap.add_argument("--version", action="version", version=f"bbgen {__version__}")
    sub = ap.add_subparsers(dest="cmd", required=True)
    build = sub.add_parser("build", help="write <name>.svg/.png/.md next to the YAML and check the wiring")
    build.add_argument("circuit", type=Path)
    build.add_argument("-o", "--out", type=Path, help="output directory (default: the YAML's directory)")
    build.add_argument("--no-png", action="store_true", help="skip the PNG")
    build.add_argument("--steps", action="store_true", help="also draw one image per wire group, highlighting it")
    chk = sub.add_parser("check", help="check the wiring only")
    chk.add_argument("circuit", type=Path)
    find = sub.add_parser("find", help="search KiCad's library for a part's symbol: bbgen find ne555")
    find.add_argument("words", nargs="+")
    find.add_argument("--kicad", help="the library's folder (default: wherever KiCad is installed)")
    args = ap.parse_args(argv)

    if args.cmd == "find":
        return _find(args.words, args.kicad)

    try:
        c = Circuit.load(args.circuit)
    except (CircuitError, yaml.YAMLError, OSError) as e:
        print(f"bbgen: {e}", file=sys.stderr)
        return 2
    result = check(c)
    _report(result)

    if args.cmd == "build":
        out = args.out or args.circuit.parent
        out.mkdir(parents=True, exist_ok=True)
        stem = args.circuit.stem
        views = [(out / f"{stem}.svg", None)]
        if args.steps:
            for i, group in enumerate(c.groups, 1):
                slug = re.sub(r"[^\w-]+", "-", group).strip("-").lower() or "group"
                views.append((out / f"{stem}.step{i}-{slug}.svg", group))
        for svg, highlight in views:
            svg.write_text(render(c, result, highlight), encoding="utf-8")
            written = [svg] + ([] if args.no_png else [p for p in [_png(svg)] if p])
            print("wrote " + ", ".join(str(p) for p in written))
        md = out / f"{stem}.md"
        md.write_text(checklist(c, result), encoding="utf-8")
        print(f"wrote {md}")
    return 1 if result.errors else 0
