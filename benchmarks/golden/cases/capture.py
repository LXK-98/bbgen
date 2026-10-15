"""Run the unit tests, writing every circuit a test builds (not placement's own trials) to a YAML file here:
the cases benchmarks/rs_outputs.py builds with both implementations.

    python3 benchmarks/golden/cases/capture.py      (from the repository's root)
"""
from pathlib import Path
OUT = Path(__file__).resolve().parent
import sys, unittest, yaml, itertools
sys.path.insert(0, ".")
import bbgen.circuit as circuit
counter = itertools.count(1)
seen = set()
orig = circuit.Circuit.__init__
depth = [0]
def init(self, data, source="circuit.yaml"):
    if depth[0] == 0 and isinstance(data, dict):
        try:
            text = yaml.safe_dump(data, sort_keys=False, allow_unicode=True)
        except Exception:
            text = None
        if text and text not in seen:
            seen.add(text)
            open(f"{OUT}/t{next(counter):03d}.yaml", "w").write(text)
    depth[0] += 1
    try:
        orig(self, data, source)
    finally:
        depth[0] -= 1
circuit.Circuit.__init__ = init
suite = unittest.defaultTestLoader.discover("tests")
unittest.TextTestRunner(verbosity=0).run(suite)
