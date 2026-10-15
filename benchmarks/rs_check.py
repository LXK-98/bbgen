"""The Rust port against the golden corpus: the same events build for build, and how much faster.

    python3 benchmarks/rs_check.py           every placed benchmark: parity, then Rust's time per build
    python3 benchmarks/rs_check.py 13 16     only circuits whose file name holds one of these
    python3 benchmarks/rs_check.py --python  ... and time the Python build of each too (slow)
    python3 benchmarks/rs_check.py --full    the benchmarks themselves, placement and all, against the whole
                                             record; and the time for each against Python's (hotspots.json)

Builds rust/ first (cargo build --release). Each placed file (golden/placed/) is built by bbgen-rs with its
events dumped, and compared with the golden record (golden.py --diff); then built again many times for its
time per build. Exits 1 if any circuit's events differ.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
BIN = ROOT / "rust" / "target" / "release" / "bbgen-rs"
sys.path.insert(0, str(HERE))


def python_ms(path: Path) -> float:
    import yaml
    sys.path.insert(0, str(ROOT))
    from bbgen.autoplace import _score
    from bbgen.check import check
    from bbgen.circuit import Circuit
    data = yaml.safe_load(path.read_text())
    c = Circuit(data, str(path))
    _score(c, check(c))
    n, t0 = 0, time.perf_counter()
    while time.perf_counter() - t0 < 2 or n < 3:
        c = Circuit(data, str(path))
        _score(c, check(c))
        n += 1
    return (time.perf_counter() - t0) / n * 1000


def full(argv: list[str]) -> int:
    """Place, wire and check each benchmark in Rust, diff the whole record, and time it."""
    import contextlib
    import io
    import json
    from golden import diff
    hot = HERE / "out" / "hotspots.json"
    py = json.loads(hot.read_text()) if hot.exists() else {}
    paths = sorted(p for p in HERE.glob("[0-9]*.yaml") if not argv or any(a in p.stem for a in argv))
    bad = 0
    total_rs = total_py = 0.0
    print(f"{'circuit':26} {'parity':>8} {'Rust s':>8} {'Python s (1 core)':>18} {'speed-up':>9}")
    with tempfile.TemporaryDirectory() as tmp:
        for p in paths:
            out = Path(tmp) / f"{p.stem}.jsonl"
            t0 = time.perf_counter()
            subprocess.run([str(BIN), "build", str(p), "--dump", str(out)], capture_output=True)
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                same = out.exists() and diff(HERE / "golden" / f"{p.stem}.jsonl.gz", out) == 0
            # timed again without the record
            t0 = time.perf_counter()
            subprocess.run([str(BIN), "build", str(p)], capture_output=True)
            rs = time.perf_counter() - t0
            ps = py.get(p.stem, {}).get("serial", {}).get("seconds")
            bad += not same
            total_rs += rs
            total_py += ps or 0
            print(f"{p.stem:26} {'same' if same else 'DIFFERS':>8} {rs:8.2f} {ps or 0:18.1f} {(ps or 0) / rs:8.0f}×", flush=True)
            if not same:
                print("   " + buf.getvalue().strip().replace("\n", "\n   ")[:2000])
    print(f"{'all':26} {len(paths) - bad}/{len(paths):<6} {total_rs:8.2f} {total_py:18.1f} {total_py / total_rs:8.0f}×")
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    with_python = "--python" in argv
    argv = [a for a in argv if a != "--python"]
    if "--full" in argv:
        subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT / "rust", check=True)
        return full([a for a in argv if a != "--full"])
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT / "rust", check=True)
    from golden import diff
    import contextlib
    import io
    paths = sorted(p for p in (HERE / "golden" / "placed").glob("*.yaml") if not argv or any(a in p.stem for a in argv))
    bad = 0
    total_rs = total_py = 0.0
    print(f"{'circuit':26} {'parity':>8} {'Rust ms':>9}" + (f" {'Python ms':>10} {'speed-up':>9}" if with_python else ""))
    with tempfile.TemporaryDirectory() as tmp:
        for p in paths:
            out = Path(tmp) / f"{p.stem}.jsonl"
            subprocess.run([str(BIN), "build", str(p), "--dump", str(out)], capture_output=True)
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                same = diff(HERE / "golden" / f"{p.stem}.jsonl.gz", out) == 0
            bad += not same
            # enough builds for about a second
            first = subprocess.run([str(BIN), "build", str(p), "--repeat", "3"], capture_output=True, text=True).stdout
            ms = float(first.strip().splitlines()[-1].split()[0])
            n = max(3, int(1000 / max(ms, 0.01)))
            res = subprocess.run([str(BIN), "build", str(p), "--repeat", str(n)], capture_output=True, text=True).stdout
            ms = float(res.strip().splitlines()[-1].split()[0])
            total_rs += ms
            line = f"{p.stem:26} {'same' if same else 'DIFFERS':>8} {ms:9.2f}"
            if with_python:
                py = python_ms(p)
                total_py += py
                line += f" {py:10.1f} {py / ms:8.0f}×"
            print(line, flush=True)
            if not same:
                print("   " + buf.getvalue().strip().replace("\n", "\n   ")[:1500])
    print(f"{'all':26} {len(paths) - bad}/{len(paths):<6} {total_rs:9.2f}" +
          (f" {total_py:10.1f} {total_py / total_rs:8.0f}×" if with_python else ""))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
