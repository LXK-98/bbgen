"""The quick benchmark circuits, held to the expectations in their files (the slow ones: benchmarks/run.py)."""

import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("bench", ROOT / "benchmarks" / "run.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

QUICK = ["01-led", "02-pico-button", "18-traffic-light", "19-two-supplies"]


class BenchmarkTest(unittest.TestCase):
    def test_quick_circuits_meet_their_expectations(self):
        for name in QUICK:
            path = ROOT / "benchmarks" / f"{name}.yaml"
            with self.subTest(name):
                result = bench.measure(path, png=False)
                self.assertEqual(result["status"], "ok")
                self.assertEqual(bench.problems(name, result, bench.expectations(path), None), [])


if __name__ == "__main__":
    unittest.main()
