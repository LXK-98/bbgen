"""Record bbgen's behaviour as the tests' expected results: after a change to placement or wiring that you mean.

    python3 benchmarks/record.py             record everything again
    python3 benchmarks/record.py 13 16       only the benchmarks whose name holds one of these

`cargo test` holds bbgen to what's recorded in benchmarks/golden/ (first recorded from the Python bbgen this
replaced). A change that moves a wire fails it, by design. Check the change with benchmarks/run.py first
(better or worse, circuit by circuit), then record it here, and commit the records with the change:

- golden/<name>.jsonl.gz: each benchmark's build event by event (`bbgen dev build --dump`), from the first
  anneal to the check; tests/golden.rs and tests/trials.rs read them
- golden/placed/<name>.yaml: each benchmark as placement left it (as JSON, which YAML reads)
- golden/expected/: what `bbgen build` prints, writes and exits with for the placed files, the examples and
  golden/cases/ (the checklist and report in full, the drawing's SHA-256); tests/outputs.rs reads them
- golden/index.json: each benchmark's layout hash and score

The KiCad library parts come from is part of the record: after updating KiCad, run this and look at what
changed.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
GOLDEN = HERE / "golden"
BIN = ROOT / "target" / "release" / "bbgen"


def record(path: Path) -> dict:
    """Build a benchmark with its events dumped; write its record and its placed file."""
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "events.jsonl"
        done = subprocess.run([str(BIN), "dev", "build", str(path), "--dump", str(out)], capture_output=True, text=True)
        if not out.exists():
            raise SystemExit(f"{path.name}: {done.stderr.strip()}")
        events = [json.loads(line) for line in out.read_text().splitlines() if line.strip()]
    with gzip.open(GOLDEN / f"{path.stem}.jsonl.gz", "wt", encoding="utf-8") as f:
        for e in events:
            f.write(json.dumps(e, separators=(",", ":")) + "\n")
    placed = next(e["data"] for e in events if e["kind"] == "placed")
    (GOLDEN / "placed" / f"{path.stem}.yaml").write_text(json.dumps(placed, indent=1, ensure_ascii=False) + "\n")
    check = events[-1]
    return {"hash": check["hash"], "score": check["score"], "events": len(events)}


def expected(group: str, path: Path) -> None:
    """What `bbgen build` prints and writes for one file."""
    exp = GOLDEN / "expected" / group
    exp.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        done = subprocess.run([str(BIN), "build", str(path), "-o", tmp, "--no-png"], capture_output=True, text=True)
        (exp / f"{path.stem}.out").write_text((done.stdout + done.stderr).replace(tmp, "OUT") + f"exit {done.returncode}\n")
        md, svg = Path(tmp) / f"{path.stem}.md", Path(tmp) / f"{path.stem}.svg"
        for f, keep in ((exp / f"{path.stem}.md", md), (exp / f"{path.stem}.svg.sha256", svg)):
            f.unlink(missing_ok=True)
        if md.exists():
            (exp / f"{path.stem}.md").write_text(md.read_text())
        if svg.exists():
            (exp / f"{path.stem}.svg.sha256").write_text(hashlib.sha256(svg.read_bytes()).hexdigest() + "\n")


def main(argv: list[str]) -> int:
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT, check=True)
    (GOLDEN / "placed").mkdir(parents=True, exist_ok=True)
    paths = sorted(p for p in HERE.glob("[0-9]*.yaml") if not argv or any(a in p.stem for a in argv))
    index_file = GOLDEN / "index.json"
    index = json.loads(index_file.read_text()) if index_file.exists() else {}
    for p in paths:
        index[p.stem] = record(p)
        print(f"{p.stem}: {index[p.stem]}", flush=True)
    done = subprocess.run([str(BIN), "dev", "kicad-roots"], capture_output=True, text=True)
    index["_kicad"] = [line for line in done.stdout.splitlines() if line]
    index_file.write_text(json.dumps(dict(sorted(index.items())), indent=1) + "\n")
    groups = {"placed": sorted((GOLDEN / "placed").glob("*.yaml")), "examples": sorted((ROOT / "examples").glob("*.yaml")),
              "cases": sorted((GOLDEN / "cases").glob("*.yaml"))}
    for group, files in groups.items():
        for f in files:
            expected(group, f)
        print(f"expected outputs: {group} ({len(files)})")
    print("now: cargo test --release, and commit benchmarks/golden/ with the change")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
