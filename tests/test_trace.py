import re
import shutil
import tempfile
import unittest
from pathlib import Path

from bbgen import trace
from bbgen.circuit import Circuit
from bbgen.cli import main

FILE = {"parts": {"U1": {"type": "dip", "pins": 8, "labels": ["GND", "TRIG", "OUT", "RST", "CV", "THR", "DIS", "VCC"]},
                  "R1": {"type": "resistor"}, "D1": {"type": "led"}},
        "nets": {"VCC": ["U1.VCC", "U1.RST"], "GND": ["U1.GND", "D1.K"], "OUT": ["U1.OUT", "R1.1"],
                 "LED": ["R1.2", "D1.A"]}}


class TraceTest(unittest.TestCase):
    def test_nothing_is_recorded_unless_asked(self):
        self.assertFalse(trace.active())
        Circuit(FILE, "t.yaml")  # and nothing breaks

    def test_placing_and_wiring_make_frames(self):
        with trace.recording() as rec:
            c = Circuit(FILE, "t.yaml")
        captions = [svg for svg, _ in rec.frames]
        self.assertTrue(any("starting layout 1 of" in s for s in captions))
        self.assertTrue(any("parts placed: wiring next" in s for s in captions))
        wiring = [s for s in captions if re.search(r"wiring \w+: ", s)]
        self.assertEqual(len(wiring), len(c.wires))  # one frame per wire, none from placement's trial builds

    def test_every_try_records_more(self):
        with trace.recording() as kept:
            Circuit(FILE, "t.yaml")
        with trace.recording(every=True) as every:
            Circuit(FILE, "t.yaml")
        self.assertGreater(len(every.frames), len(kept.frames))

    @unittest.skipUnless(shutil.which("ffmpeg") and shutil.which("magick"), "needs ffmpeg and ImageMagick")
    def test_bbgen_build_writes_a_video(self):
        import yaml
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.yaml"
            path.write_text(yaml.safe_dump(FILE))
            self.assertEqual(main(["build", str(path), "--no-png", "--video", str(Path(tmp) / "t.mp4")]), 0)
            self.assertGreater((Path(tmp) / "t.mp4").stat().st_size, 1000)


if __name__ == "__main__":
    unittest.main()
