import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

from bbgen.cli import main

EXAMPLES = Path(__file__).parent.parent / "examples"
EXAMPLE = EXAMPLES / "pico-blink.yaml"


def run(*args):
    out = StringIO()
    with redirect_stdout(out):
        code = main([str(a) for a in args])
    return code, out.getvalue()


class CliTest(unittest.TestCase):
    def test_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            code, out = run("build", EXAMPLE, "-o", tmp, "--no-png", "--steps")
            self.assertEqual(code, 0, out)
            names = sorted(p.name for p in Path(tmp).iterdir())
            self.assertEqual(names, ["pico-blink.md", "pico-blink.step1-ground.svg", "pico-blink.svg"])
            svg = (Path(tmp) / "pico-blink.svg").read_text()
            self.assertTrue(svg.startswith("<svg") and "wiring check passed" in svg)

    def test_examples_pass(self):
        for example in sorted(EXAMPLES.glob("*.yaml")):
            with self.subTest(example.name):
                code, out = run("check", example)
                self.assertEqual(code, 0, out)
                self.assertIn("check: 0 errors, 0 warnings", out)

    def test_errors_exit_1_and_bad_files_exit_2(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad = Path(tmp) / "bad.yaml"
            bad.write_text("parts:\n  R1: {type: resistor, pins: [a1, a5]}\nnets:\n  X: [R1.1, R1.2]\n")
            self.assertEqual(run("check", bad)[0], 1)
            bad.write_text("parts:\n  R1: {type: banana}\n")
            self.assertEqual(run("check", bad)[0], 2)


if __name__ == "__main__":
    unittest.main()
