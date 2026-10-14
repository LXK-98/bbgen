"""Run bbgen on every benchmark circuit and measure what it made.

    python3 benchmarks/run.py              build them all, print the table, compare with baseline.json
    python3 benchmarks/run.py 05 m1        only circuits whose file name holds one of these
    python3 benchmarks/run.py --save       ... and make this run the new baseline
    python3 benchmarks/run.py --no-png     skip the PNGs (faster)
    python3 benchmarks/run.py --check      ... and exit 1 if a circuit misses its expectations or got worse

Each circuit is built in a process of its own with a fixed hash seed, as a user would run bbgen.
Drawings and checklists go to benchmarks/out/; the table to benchmarks/results.md. A circuit file's
`expect:` gives the most each measure may be (`expect: {slanted: 0, flex: 2}`). --save keeps the
drawings as benchmarks/out/baseline/; after that, a circuit whose measures change gets its drawing
before and after in benchmarks/out/diff/, one above the other.
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
    ("errors", "err", True), ("warnings", "warn", True), ("slanted", "slant", True), ("bent", "bent", True),
    ("crossings", "cross", True), ("flex", "flex", True), ("rail_links_inside", "links", True),
    ("label_clashes", "labels", True), ("board_wire", "wire", True), ("flying", "lead", True),
    ("columns", "cols", True), ("seconds", "s", True),
]
# what --check holds against the baseline: any of these going up is a regression
GUARDED = ["errors", "warnings", "slanted", "bent", "crossings", "flex", "rail_links_inside", "label_clashes"]


def measure(path: Path, png: bool) -> dict:
    """Build one circuit and measure it (runs in its own process)."""
    sys.path.insert(0, str(HERE.parent))
    from bbgen.autoplace import _score
    from bbgen.check import check
    from bbgen.checklist import checklist
    from bbgen.circuit import Circuit
    from bbgen import measure as m
    from bbgen.cli import _png
    from bbgen.render import draw

    start = time.perf_counter()
    try:
        c = Circuit.load(path)
        r = check(c)
    except Exception as e:  # a circuit bbgen can't build is a result too
        return {"status": f"{type(e).__name__}: {e}", "seconds": round(time.perf_counter() - start, 2)}
    seconds = time.perf_counter() - start

    board = 0.0
    for w in c.wires:
        if w.a and w.b and w.a.hole and w.b.hole and not w.flex:
            pts = [c.end_xy(w.a), *w.via_xy, c.end_xy(w.b)]
            board += sum(math.dist(p, q) for p, q in zip(pts, pts[1:]))
    svg, canvas = draw(c, r)
    cols = [h.col for h in c.occupied if not h.is_rail and not c.occupied[h].endswith(("cable", "Cable", "Keep Out"))]
    codes: dict[str, int] = {}
    for issue in r.issues:
        codes[issue.code] = codes.get(issue.code, 0) + 1

    OUT.mkdir(exist_ok=True)
    out = OUT / f"{path.stem}.svg"
    out.write_text(svg, encoding="utf-8")
    (OUT / f"{path.stem}.md").write_text(checklist(c, r), encoding="utf-8")
    if png:
        _png(out)
    return {
        "status": "ok", "seconds": round(seconds, 2), "parts": len(c.parts), "wires": len(c.wires),
        "errors": len(r.errors), "warnings": len(r.warnings), "codes": codes,
        "slanted": m.slanted(c), "bent": m.bent(c), "crossings": m.crossings(c),
        "flex": sum(1 for w in c.wires if w.flex), "rail_links_inside": m.rail_links_inside(c),
        "label_clashes": m.label_clashes(c, canvas), "board_wire": round(board, 1),
        "flying": round(m.flying_length(c), 1),
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
                 f"{sum(r.get('flex', 0) for r in ok)} flexible jumpers, "
                 f"{sum(r['seconds'] for r in ok):.1f} s in all")
    return "\n".join(lines)


# what every circuit is held to unless its file says otherwise: bbgen's wiring never bends or slants a wire
# lying on the board (it uses a flexible jumper instead), and links rails only at their ends
DEFAULT_EXPECT = {"errors": 0, "bent": 0, "slanted": 0, "rail_links_inside": 0}


def expectations(path: Path) -> dict:
    import yaml
    return {**DEFAULT_EXPECT, **((yaml.safe_load(path.read_text()) or {}).get("expect") or {})}


def problems(name: str, result: dict, expect: dict, before: dict | None) -> list[str]:
    """What --check fails a circuit for: an expectation missed, or a guarded measure worse than the baseline."""
    if result.get("status") != "ok":
        return [f"{name}: doesn't build ({result.get('status')})"] if (before or {}).get("status") == "ok" else []
    out = [f"{name}: {key} is {result.get(key)}, expected at most {most}"
           for key, most in expect.items() if result.get(key, 0) > most]
    if before and before.get("status") == "ok":
        out += [f"{name}: {key} went from {before[key]} to {result[key]}"
                for key in GUARDED if key in before and result.get(key, 0) > before[key]]
    return out


def diff_images(results: dict, baseline: dict) -> None:
    """Each circuit whose measures changed: its baseline drawing above its drawing now."""
    import shutil
    import subprocess
    (OUT / "diff").mkdir(parents=True, exist_ok=True)
    for name, res in results.items():
        old, new = OUT / "baseline" / f"{name}.png", OUT / f"{name}.png"
        before = baseline.get(name) or {}
        changed = any(res.get(k) != before.get(k) for k, _, _ in COLUMNS if k != "seconds")
        if changed and old.exists() and new.exists() and shutil.which("magick"):
            subprocess.run(["magick", str(old), str(new), "-background", "white", "-gravity", "west",
                            "-append", str(OUT / "diff" / f"{name}.png")], check=True)


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
    if png and not save:
        diff_images(results, baseline)
    failed = []
    for path in files:
        failed += problems(path.stem, results[path.stem], expectations(path), baseline.get(path.stem))
    if failed:
        print("\n" + "\n".join(f"FAIL {f}" for f in failed))
    if save:
        BASELINE.write_text(json.dumps({**baseline, **results}, indent=1, sort_keys=True) + "\n")
        if png:
            import shutil
            (OUT / "baseline").mkdir(parents=True, exist_ok=True)
            for name in results:
                if (OUT / f"{name}.png").exists():
                    shutil.copy(OUT / f"{name}.png", OUT / "baseline" / f"{name}.png")
        print(f"saved {len(results)} results as the baseline", file=sys.stderr)
    return 1 if failed and "--check" in argv else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
