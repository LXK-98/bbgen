"""Every trial layout placement built, built again by the Rust port: the same score?

    python3 benchmarks/rs_trials.py          every benchmark
    python3 benchmarks/rs_trials.py 05 13    only these

The golden record holds each trial's layout (where each part went, which leg pairs were asked for) and its
score. This rebuilds each trial's file as autoplace made it (the benchmark file, autowire on, the parts it
placed, the layout written in) and has bbgen-rs score it. A trial that scores differently is a difference in
building, not in searching: leg placement, wiring, the check or the score.
"""

from __future__ import annotations

import copy
import gzip
import json
import subprocess
import sys
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
BIN = HERE.parent / "rust" / "target" / "release" / "bbgen-rs"


def trial_data(data: dict, layout: dict) -> dict:
    out = copy.deepcopy(data)
    out.setdefault("autowire", True)
    out["_placed"] = list(layout["parts"])
    if layout["rail_order"]:
        out["_rail_order"] = layout["rail_order"]
    for ref, where in layout["parts"].items():
        spec = dict(out["parts"][ref])
        if isinstance(where, list) and len(where) == 2 and isinstance(where[1], int):
            spec["at"], spec["rot"] = where
        elif where is None:
            spec.pop("pins", None)
        else:
            spec["pins"] = where
        out["parts"][ref] = spec
    out["_leg_rank"] = layout["leg_rank"]
    return out


def main(argv: list[str]) -> int:
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=HERE.parent / "rust", check=True)
    paths = sorted(p for p in HERE.glob("[0-9]*.yaml") if not argv or any(a in p.stem for a in argv))
    bad_total = 0
    for p in paths:
        data = yaml.safe_load(p.read_text())
        with gzip.open(HERE / "golden" / f"{p.stem}.jsonl.gz", "rt") as f:
            trials = [e for e in map(json.loads, f) if e["kind"] == "trial"]
        lines = "".join(json.dumps({"data": trial_data(data, t["layout"]), "source": str(p)}) + "\n" for t in trials)
        out = subprocess.run([str(BIN), "score-lines"], input=lines, capture_output=True, text=True)
        got = out.stdout.splitlines()
        bad = [(i, t["score"], g) for i, (t, g) in enumerate(zip(trials, got)) if g.split()[0] != repr(float(t["score"]))]
        if len(got) != len(trials):
            bad.append((len(got), "?", out.stderr.strip()[-300:]))
        bad_total += bool(bad)
        print(f"{p.stem:26} {len(trials) - len(bad):4}/{len(trials)} trials score the same", flush=True)
        for i, want, g in bad[:3]:
            print(f"    trial {i}: Python {want!r}, Rust {g}")
    return 1 if bad_total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
