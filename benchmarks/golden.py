"""The golden corpus: what this code does with each benchmark, stage by stage, for checking a port against.

    python3 benchmarks/golden.py             record every benchmark into benchmarks/golden/
    python3 benchmarks/golden.py 13 16       only circuits whose file name holds one of these
    python3 benchmarks/golden.py --diff A B  the first event where two records part ways

For each circuit, in a process of its own with a fixed hash seed (bbgen/dump.py says what's recorded):

- golden/<name>.jsonl.gz: the record of the full build, one event a line, ending with the check's issues,
  the layout's score and its hash (every part's holes and every wire);
- golden/placed/<name>.yaml: the file as placement left it, everything placed and no wires. Building it
  wires the same layout without placing anything: what a port that can't place yet is checked on.

golden/index.json holds the KiCad library the record was made with (its parts are read from there, so
another library version may give other layouts), and each circuit's hash and score, and whether building the placed file gave the same
layout and the same wiring events as the full build's last build did (it must: if not, the placed file
leaves something out).
"""

from __future__ import annotations

import gzip
import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOLDEN = HERE / "golden"


def record(path: Path) -> dict:
    import yaml
    from bbgen import dump
    from bbgen.autoplace import _score
    from bbgen.check import check
    from bbgen.circuit import Circuit
    sys.path.insert(0, str(HERE))
    from hotspots import layout_hash

    def build(p: Path) -> tuple[list, str, float]:
        with dump.recording() as events:
            c = Circuit.load(p)
            r = check(c)
            score, digest = _score(c, r), layout_hash(c)
            dump.event("check", issues=[[i.level, i.code, i.message] for i in r.issues], score=score, hash=digest)
        return events, digest, score

    events, digest, score = build(path)
    with gzip.open(GOLDEN / f"{path.stem}.jsonl.gz", "wt", encoding="utf-8") as f:
        for e in events:
            f.write(json.dumps(e, separators=(",", ":")) + "\n")
    placed = next((e["data"] for e in events if e["kind"] == "placed"), None)
    if placed is None:  # nothing to place: the file as it is
        placed = yaml.safe_load(path.read_text(encoding="utf-8"))
    out = GOLDEN / "placed" / f"{path.stem}.yaml"
    out.write_text(yaml.safe_dump(placed, sort_keys=False, allow_unicode=True), encoding="utf-8")
    again, digest2, _ = build(out)
    # the full build's last build: from the placed file on (placement's own events come before it)
    start = next((i + 1 for i, e in enumerate(events) if e["kind"] == "placed"), 0)
    return {"hash": digest, "score": score, "events": len(events), "placed_hash": digest2 == digest,
            "placed_events": again == events[start:]}


def load(path: Path) -> list[dict]:
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def diff(a: Path, b: Path) -> int:
    ea, eb = load(a), load(b)
    for i, (x, y) in enumerate(zip(ea, eb)):
        if x != y:
            print(f"event {i + 1} differs ({x.get('kind')} / {y.get('kind')})")
            if i:
                print(f"  after: {json.dumps(ea[i - 1])[:300]}")
            for name, e in ((a.name, x), (b.name, y)):
                print(f"  {name}: {json.dumps(e)[:1000]}")
            return 1
    if len(ea) != len(eb):
        print(f"the same for {min(len(ea), len(eb))} events, then {a.name} has {len(ea)} and {b.name} {len(eb)}")
        return 1
    print(f"the same: {len(ea)} events")
    return 0


def run(path: Path) -> dict:
    env = {**os.environ, "PYTHONHASHSEED": "0", "PYTHONPATH": str(HERE.parent)}
    done = subprocess.run([sys.executable, __file__, "--one", str(path)], capture_output=True, text=True, env=env)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return {"error": done.stderr.strip().splitlines()[-1] if done.stderr.strip() else "?"}


def main(argv: list[str]) -> int:
    if argv[:1] == ["--one"]:
        print(json.dumps(record(Path(argv[1]))))
        return 0
    if argv[:1] == ["--diff"]:
        return diff(Path(argv[1]), Path(argv[2]))
    (GOLDEN / "placed").mkdir(parents=True, exist_ok=True)
    paths = sorted(p for p in HERE.glob("*.yaml") if not argv or any(a in p.stem for a in argv))
    index_file = GOLDEN / "index.json"
    index = json.loads(index_file.read_text()) if index_file.exists() else {}
    baseline = json.loads((HERE / "baseline.json").read_text())
    order = sorted(paths, key=lambda p: -float(baseline.get(p.stem, {}).get("seconds", 0)))  # the longest first
    bad = 0
    with ThreadPoolExecutor(8) as ex:
        for p, r in zip(order, ex.map(run, order)):
            index[p.stem] = r
            ok = r.get("placed_hash") and r.get("placed_events")
            bad += not ok
            print(f"{p.stem}: {r}", flush=True)
    from bbgen.kicad import library_roots  # 20 of the 21 take parts from it: a new KiCad library may change them
    index["_kicad"] = [str(r) for r in library_roots()]
    index_file.write_text(json.dumps(dict(sorted(index.items())), indent=1) + "\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
