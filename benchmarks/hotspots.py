"""Where bbgen's time goes, circuit by circuit: the numbers a port to Rust is judged by.

    python3 benchmarks/hotspots.py            every benchmark
    python3 benchmarks/hotspots.py 13 16      only circuits whose file name holds one of these
    python3 benchmarks/hotspots.py -j 4       this many circuits side by side in the serial passes (default 8)

Three passes, each circuit in a process of its own with a fixed hash seed:

- serial (BBGEN_JOBS=1), plain: wall time, the number of full trial builds placement made, the drawing,
  checklist and check of the finished circuit timed alone, and a hash of the layout (every part's holes
  and every wire), which a change that should leave layouts alone must not change;
- serial, under cProfile: each stage's share of the time (cProfile slows Python down, so shares only);
- parallel (the default worker count), one circuit at a time so the workers have the machine: wall time,
  and the trial builds a worker started that the search then had no use for.

The table goes to benchmarks/hotspots.md, the raw numbers to benchmarks/out/hotspots.json.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "out"

# (file, function) -> column: the stages a build goes through (cumulative time)
STAGES = [
    (("autoplace.py", "_evaluate"), "trial builds"),
    (("autowire.py", "autowire"), "autowire"),
    (("autowire.py", "_relay"), "_relay"),
    (("autowire.py", "_reach"), "_reach"),
    (("autowire.py", "_rails"), "_rails"),
    (("autoplace.py", "place_legs"), "place_legs"),
    (("autoplace.py", "anneal"), "anneal"),
    (("check.py", "check"), "check"),
]
# Python's own costs that Rust doesn't have: rounding positions into dict keys, building and hashing Holes
OVERHEAD = [("~", "<built-in method builtins.round>"), ("circuit.py", "_key"),
            ("<string>", "__hash__"), ("<string>", "__init__"), ("<string>", "__eq__")]


def layout_hash(c) -> str:
    parts = sorted((p.ref, tuple(c.hole_name(x.hole) if x.hole else "-" for x in p.pins)) for p in c.parts.values())
    wires = sorted((w.group, c.hole_name(w.a.hole) if w.a.hole else str(w.a.pin),
                    c.hole_name(w.b.hole) if w.b.hole else str(w.b.pin), w.flex) for w in c.wires)
    return hashlib.md5(repr((parts, wires)).encode()).hexdigest()[:10]


def serial(path: Path) -> dict:
    from bbgen import autoplace
    from bbgen.check import check
    from bbgen.checklist import checklist
    from bbgen.circuit import Circuit
    from bbgen.render import draw

    builds = 0
    evaluate = autoplace._evaluate

    def counted(data, source):
        nonlocal builds
        builds += 1
        return evaluate(data, source)
    autoplace._evaluate = counted
    start = time.perf_counter()
    c = Circuit.load(path)
    seconds = time.perf_counter() - start
    start = time.perf_counter()
    r = check(c)
    draw(c, r)
    checklist(c, r)
    outputs = time.perf_counter() - start
    return {"seconds": seconds, "builds": builds, "outputs": outputs, "hash": layout_hash(c)}


def profiled(path: Path) -> dict:
    import cProfile
    import pstats
    from bbgen.circuit import Circuit

    prof = cProfile.Profile()
    prof.enable()
    Circuit.load(path)
    prof.disable()
    stats = pstats.Stats(prof).stats  # (file, line, function) -> (cc, nc, tottime, cumtime, callers)
    total = sum(tt for _, _, tt, _, _ in stats.values())

    def find(file, func):
        return [v for (f, _, fn), v in stats.items() if fn == func and (f == file or f.endswith("/" + file))]
    shares = {}
    for (file, func), name in STAGES:
        # a function that calls itself counts its time once: take the outermost call's cumulative time
        shares[name] = max((v[3] for v in find(file, func)), default=0.0) / total
    shares["object overhead"] = sum(v[2] for file, func in OVERHEAD for v in find(file, func)) / total
    modules: dict[str, float] = {}
    for (f, _, _), (_, _, tt, _, _) in stats.items():
        name = Path(f).name if "/bbgen/" in f else "Python built-ins"
        modules[name] = modules.get(name, 0.0) + tt / total
    return {"shares": shares, "modules": modules}


def parallel(path: Path) -> dict:
    from bbgen import autoplace
    from bbgen.circuit import Circuit

    futures = []
    rebuilds = 0
    make_pool, evaluate = autoplace._pool, autoplace._evaluate

    class Counting:
        def __init__(self, pool):
            self.pool = pool

        def submit(self, *args):
            futures.append(self.pool.submit(*args))
            return futures[-1]

    def counted(data, source):  # in this process: a trial built again because a worker's circuit isn't sent back
        nonlocal rebuilds
        rebuilds += 1
        return evaluate(data, source)
    autoplace._pool = lambda: Counting(make_pool())
    autoplace._evaluate = counted
    start = time.perf_counter()
    Circuit.load(path)
    seconds = time.perf_counter() - start
    return {"seconds": seconds, "jobs": autoplace.JOBS, "submitted": len(futures),
            "started": sum(1 for f in futures if not f.cancelled()), "rebuilds": rebuilds}


def run(mode: str, path: Path) -> dict:
    env = {**os.environ, "PYTHONHASHSEED": "0", "PYTHONPATH": str(HERE.parent)}
    if mode != "parallel":
        env["BBGEN_JOBS"] = "1"
    done = subprocess.run([sys.executable, __file__, "--one", mode, str(path)], capture_output=True, text=True, env=env)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return {"error": done.stderr.strip().splitlines()[-1] if done.stderr.strip() else "?"}


def pct(x: float) -> str:
    return f"{100 * x:.0f}"


def table(results: dict) -> str:
    lines = ["# Where the time goes", "",
             "Written by `benchmarks/hotspots.py`. Serial: one process (`BBGEN_JOBS=1`). Shares are of the "
             "serial build under cProfile, in percent; the stages nest (autowire is part of the trial builds, "
             "`_relay` part of autowire), so they don't add up to 100. *Overhead* is rounding positions into "
             "keys and building and hashing `Hole`s. *Outputs*: check, drawing and checklist of the finished "
             "circuit. *Wasted*: trial builds a worker started that the search didn't use.", "",
             "| circuit | serial s | builds | ms/build | outputs ms | " + " | ".join(n for _, n in STAGES[1:]) +
             " | overhead | parallel s | speed-up | wasted | hash |",
             "|---|" + "---|" * (len(STAGES) + 8)]
    tot = {"serial": 0.0, "parallel": 0.0, "builds": 0, "started": 0}
    for name, r in results.items():
        s, p, par = r.get("serial", {}), r.get("profile", {}), r.get("parallel", {})
        if "seconds" not in s:
            lines.append(f"| {name} | {s.get('error') or p.get('error') or '?'} |")
            continue
        sh = p.get("shares", {})
        per = 1000 * s["seconds"] / max(1, s["builds"] + 1)  # the finished circuit is a build too
        if par.get("seconds"):
            wasted = par["started"] - s["builds"] if par["submitted"] else 0
            tail = f"{par['seconds']:.1f} | {s['seconds'] / par['seconds']:.1f}× | {wasted}"
        else:
            tail = "- | - | -"
        lines.append(f"| {name} | {s['seconds']:.1f} | {s['builds']} | {per:.0f} | {1000 * s['outputs']:.0f} | "
                     + " | ".join(pct(sh.get(n, 0)) for _, n in STAGES[1:])
                     + f" | {pct(sh.get('object overhead', 0))} | {tail} | `{s['hash']}` |")
        tot["serial"] += s["seconds"]
        tot["parallel"] += par.get("seconds", 0)
        tot["builds"] += s["builds"]
        tot["started"] += par.get("started", 0)
    lines += ["", f"In all: {tot['serial']:.0f} s serial, {tot['parallel']:.0f} s parallel, {tot['builds']} trial "
              f"builds needed, {tot['started']} started by workers."]
    # where the time is by module, over all circuits weighted by serial time
    mods: dict[str, float] = {}
    for r in results.values():
        w = r.get("serial", {}).get("seconds", 0)
        for m, share in r.get("profile", {}).get("modules", {}).items():
            mods[m] = mods.get(m, 0.0) + share * w
    if mods:
        total = sum(mods.values())
        lines += ["", "Own time by module, all circuits (weighted by serial time):", "", "| module | % |", "|---|---|"]
        lines += [f"| {m} | {100 * t / total:.1f} |" for m, t in sorted(mods.items(), key=lambda t: -t[1])]
    return "\n".join(lines) + "\n"


def main(argv: list[str]) -> int:
    if argv[:1] == ["--one"]:
        mode, path = argv[1], Path(argv[2])
        print(json.dumps({"serial": serial, "profile": profiled, "parallel": parallel}[mode](path)))
        return 0
    jobs = 8
    if "-j" in argv:
        i = argv.index("-j")
        jobs = int(argv[i + 1])
        del argv[i:i + 2]
    paths = sorted(p for p in HERE.glob("*.yaml") if not argv or any(a in p.stem for a in argv))
    results: dict = {p.stem: {} for p in paths}
    # the longest first, so a slow one doesn't start last
    order = sorted(paths, key=lambda p: -float(json.loads((HERE / "baseline.json").read_text())
                                               .get(p.stem, {}).get("seconds", 0)))
    for mode in ("serial", "profile"):
        with ThreadPoolExecutor(jobs) as ex:
            for p, r in zip(order, ex.map(lambda p: run(mode, p), order)):
                results[p.stem][mode] = r
                print(f"{mode:8} {p.stem}: {r.get('seconds', r.get('error', ''))}", flush=True)
    for p in paths:
        results[p.stem]["parallel"] = r = run("parallel", p)
        print(f"parallel {p.stem}: {r.get('seconds', r.get('error', ''))}", flush=True)
    OUT.mkdir(exist_ok=True)
    (OUT / "hotspots.json").write_text(json.dumps(results, indent=1))
    (HERE / "hotspots.md").write_text(table(results))
    print(f"wrote {HERE / 'hotspots.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
