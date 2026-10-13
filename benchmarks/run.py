"""Run bbgen on every benchmark circuit and measure what it made.

    python3 benchmarks/run.py              build them all, print the table, compare with baseline.json
    python3 benchmarks/run.py 05 m1        only circuits whose file name holds one of these
    python3 benchmarks/run.py --save       ... and make this run the new baseline
    python3 benchmarks/run.py --no-png     skip the PNGs (faster)

Each circuit is built in a process of its own with a fixed hash seed, as a user would run bbgen.
Drawings and checklists go to benchmarks/out/; the table to benchmarks/results.md.
"""

from __future__ import annotations

import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "out"
BASELINE = HERE / "baseline.json"

# what the table shows: (key, heading, lower is better)
COLUMNS = [
    ("errors", "err", True), ("warnings", "warn", True), ("slanted", "slant", True), ("over", "over", True),
    ("board_wire", "wire", True), ("flying", "lead", True), ("columns", "cols", True), ("seconds", "s", True),
]


def measure(path: Path, png: bool) -> dict:
    """Build one circuit and measure it (runs in its own process)."""
    sys.path.insert(0, str(HERE.parent))
    from bbgen.autoplace import _score
    from bbgen.check import check
    from bbgen.checklist import checklist
    from bbgen.circuit import Circuit, _aligned
    from bbgen.cli import _png
    from bbgen.parts import LEAD_KINDS
    from bbgen.render import render

    start = time.perf_counter()
    try:
        c = Circuit.load(path)
        r = check(c)
    except Exception as e:  # a circuit bbgen can't build is a result too
        return {"status": f"{type(e).__name__}: {e}", "seconds": round(time.perf_counter() - start, 2)}
    seconds = time.perf_counter() - start

    board, flying, slanted, over = 0.0, 0.0, 0, 0
    for w in c.wires:
        if not (w.a and w.b):
            continue
        if w.a.hole and w.b.hole:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            segs = list(zip(pts, pts[1:]))
            board += sum(math.dist(p, q) for p, q in segs)
            slanted += any(not _aligned(p, q) for p, q in segs)
            over += sum(len(c._bodies_crossed(p, q)) for p, q in segs)
        else:
            flying += math.dist(c.end_xy(w.a), c.end_xy(w.b))
    for part in c.parts.values():
        if part.kind in LEAD_KINDS and all(p.xy for p in part.pins):
            slanted += not _aligned(part.pins[0].xy, part.pins[1].xy)
    cols = [h.col for h in c.occupied if not h.is_rail and not c.occupied[h].endswith(("cable", "Cable", "Keep Out"))]
    codes: dict[str, int] = {}
    for issue in r.issues:
        codes[issue.code] = codes.get(issue.code, 0) + 1

    OUT.mkdir(exist_ok=True)
    svg = OUT / f"{path.stem}.svg"
    svg.write_text(render(c, r), encoding="utf-8")
    (OUT / f"{path.stem}.md").write_text(checklist(c, r), encoding="utf-8")
    if png:
        _png(svg)
    return {
        "status": "ok", "seconds": round(seconds, 2), "parts": len(c.parts), "wires": len(c.wires),
        "errors": len(r.errors), "warnings": len(r.warnings), "codes": codes,
        "slanted": slanted, "over": over, "board_wire": round(board, 1), "flying": round(flying, 1),
        "columns": (max(cols) - min(cols) + 1) if cols else 0,
        "keepout_holes": sum(1 for who in c.occupied.values() if who.endswith(("cable", "Cable", "Keep Out"))),
        "score": round(_score(c, r), 1),
    }


def run(path: Path, png: bool) -> dict:
    env = {**os.environ, "PYTHONHASHSEED": "0"}
    args = [sys.executable, str(Path(__file__).resolve()), "--one", str(path)] + ([] if png else ["--no-png"])
    done = subprocess.run(args, capture_output=True, text=True, env=env)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return {"status": f"crashed: {done.stderr.strip().splitlines()[-1] if done.stderr.strip() else '?'}"}


def _cell(result: dict, key: str, before: dict | None) -> str:
    value = result.get(key)
    if value is None:
        return "-"
    text = f"{value:g}" if isinstance(value, float) else str(value)
    old = (before or {}).get(key)
    if isinstance(old, (int, float)) and old != value:
        lower_better = dict((k, b) for k, _, b in COLUMNS)[key]
        if key == "seconds" and abs(value - old) < max(0.5, 0.25 * old):
            return text  # timing noise
        better = (value < old) == lower_better
        text += f" ({'+' if value > old else ''}{value - old:g}{' ✓' if better else ' ✗'})"
    return text


def table(results: dict, baseline: dict) -> str:
    head = "| circuit | " + " | ".join(h for _, h, _ in COLUMNS) + " | notes |"
    lines = [head, "|" + "---|" * (len(COLUMNS) + 2)]
    for name, res in results.items():
        before = baseline.get(name)
        if res.get("status") != "ok":
            note = res.get("status", "?")
            if before and before.get("status") == "ok":
                note += " ✗ (built before)"
            lines.append(f"| {name} | " + " | ".join("-" for _ in COLUMNS) + f" | {note[:110]} |")
            continue
        cells = [_cell(res, k, before) for k, _, _ in COLUMNS]
        codes = ", ".join(f"{k} {v}" for k, v in sorted(res.get("codes", {}).items()))
        if before and before.get("status") != "ok":
            codes = "builds now ✓; " + codes
        lines.append(f"| {name} | " + " | ".join(cells) + f" | {codes} |")
    ok = [r for r in results.values() if r.get("status") == "ok"]
    lines.append("")
    lines.append(f"{len(ok)}/{len(results)} built; {sum(r['errors'] for r in ok)} errors, "
                 f"{sum(r['warnings'] for r in ok)} warnings, {sum(r['slanted'] for r in ok)} slanted, "
                 f"{sum(r['seconds'] for r in ok):.1f} s in all")
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    if argv[:1] == ["--one"]:
        print(json.dumps(measure(Path(argv[1]), "--no-png" not in argv)))
        return 0
    png = "--no-png" not in argv
    save = "--save" in argv
    picks = [a for a in argv if not a.startswith("--")]
    files = [p for p in sorted(HERE.glob("*.yaml")) if not picks or any(k in p.stem for k in picks)]
    baseline = json.loads(BASELINE.read_text()) if BASELINE.exists() else {}
    results = {}
    for path in files:
        print(f"{path.stem} ...", end=" ", flush=True, file=sys.stderr)
        results[path.stem] = run(path, png)
        print(results[path.stem].get("status"), file=sys.stderr)
    text = table(results, baseline)
    print(text)
    if not picks:
        (HERE / "results.md").write_text("# Benchmark results\n\nCompared with baseline.json where it differs: "
                                         "✓ better, ✗ worse.\n\n" + text + "\n", encoding="utf-8")
    if save:
        BASELINE.write_text(json.dumps({**baseline, **results}, indent=1, sort_keys=True) + "\n")
        print(f"saved {len(results)} results as the baseline", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
