"""bbgen and bbgen-rs on the same files: the same drawing, checklist, report and exit status, byte for byte?

    python3 benchmarks/rs_outputs.py                 the placed benchmarks, the examples, and the circuits the
                                                     unit tests build (golden/cases/, captured by capture.py)
    python3 benchmarks/rs_outputs.py FILE.yaml ...   these files
    python3 benchmarks/rs_outputs.py --steps         ... with a drawing per wire group too

Each file is built by `python3 -m bbgen build -o DIR --no-png` and `bbgen-rs build -o DIR --no-png` into
two folders; then every file written, what was printed and the exit status are compared. The first lines
that differ are shown. Exits 1 if anything differs.
"""

from __future__ import annotations

import difflib
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
BIN = ROOT / "rust" / "target" / "release" / "bbgen-rs"


def run(cmd: list[str]) -> tuple[int, str]:
    env = {**os.environ, "PYTHONHASHSEED": "0", "PYTHONPATH": str(ROOT)}
    done = subprocess.run(cmd, capture_output=True, text=True, env=env, cwd=ROOT)
    return done.returncode, done.stdout + done.stderr


def main(argv: list[str]) -> int:
    steps = ["--steps"] if "--steps" in argv else []
    argv = [a for a in argv if a != "--steps"]
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT / "rust", check=True)
    files = [Path(a) for a in argv] or (sorted((HERE / "golden" / "placed").glob("*.yaml")) + sorted((ROOT / "examples").glob("*.yaml"))
                                        + sorted((HERE / "golden" / "cases").glob("*.yaml")))
    bad = 0
    with tempfile.TemporaryDirectory() as tmp:
        for f in files:
            py_dir, rs_dir = Path(tmp) / f"py-{f.stem}", Path(tmp) / f"rs-{f.stem}"
            py = run([sys.executable, "-m", "bbgen", "build", str(f), "-o", str(py_dir), "--no-png", *steps])
            rs = run([str(BIN), "build", str(f), "-o", str(rs_dir), "--no-png", *steps])
            problems = []
            if py[0] != rs[0]:
                problems.append(f"exit status: Python {py[0]}, Rust {rs[0]}")
            out_py, out_rs = py[1].replace(str(py_dir), "OUT"), rs[1].replace(str(rs_dir), "OUT")
            if out_py != out_rs:
                problems.append("printed:\n" + "".join(list(difflib.unified_diff(out_py.splitlines(True), out_rs.splitlines(True), "python", "rust", n=0))[2:12]))
            names = sorted({p.name for d in (py_dir, rs_dir) if d.exists() for p in d.iterdir()})
            for name in names:
                a, b = py_dir / name, rs_dir / name
                if not a.exists() or not b.exists():
                    problems.append(f"{name}: only written by {'Rust' if b.exists() else 'Python'}")
                    continue
                ta, tb = a.read_text(), b.read_text()
                if ta != tb:
                    diff = list(difflib.unified_diff(ta.splitlines(True), tb.splitlines(True), "python", "rust", n=0))
                    changed = sum(1 for d in diff if d.startswith(("+", "-")) and not d.startswith(("+++", "---"))) // 2
                    problems.append(f"{name}: about {changed} lines differ\n" + "".join(d[:300] + ("\n" if len(d) > 300 else "") for d in diff[2:10]))
            bad += bool(problems)
            print(f"{f.stem:28} {'same' if not problems else 'DIFFERS'}", flush=True)
            for p in problems:
                print("    " + p.replace("\n", "\n    ").rstrip())
    print(f"{len(files) - bad}/{len(files)} the same")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
