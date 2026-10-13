import unittest

from tests.helpers import codes

from bbgen.check import check
from bbgen.circuit import Circuit

SMALL = {
    "parts": {
        "Pico": {"type": "pico"},
        "U1": {"type": "dip", "pins": 6, "labels": ["VIN", "GND", "EN", "X", "Y", "VOUT"]},
        "R1": {"type": "resistor", "value": "10k"},
        "C1": {"type": "capacitor", "value": "1uF"},
    },
    "nets": {
        "VSYS": ["Pico.VSYS", "U1.VIN", "C1.1"],
        "GND": ["Pico.GND", "U1.GND", "R1.2", "C1.2"],
        "EN": ["Pico.GP21", "U1.EN", "R1.1"],
    },
}


def build(data):
    c = Circuit(data, "test.yaml")
    return c, check(c)


class AutoplaceTest(unittest.TestCase):
    def test_everything_gets_placed_and_wired(self):
        c, r = build(SMALL)
        self.assertEqual(codes(r), [])
        self.assertTrue(all(p.pins[0].hole for p in c.parts.values()))
        self.assertTrue(c.wires and all(w.auto for w in c.wires))

    def test_the_same_file_gets_the_same_layout(self):
        first, second = build(SMALL)[0], build(SMALL)[0]
        def holes(c):
            return [(p.ref, [c.hole_name(q.hole) for q in p.pins]) for p in c.parts.values()]
        self.assertEqual(holes(first), holes(second))

    def test_parts_given_a_place_keep_it(self):
        data = {**SMALL, "parts": {**SMALL["parts"], "Pico": {"type": "pico", "at": "h3"}}}
        c, r = build(data)
        self.assertEqual(c.hole_name(c.parts["Pico"].pins[0].hole), "h3")
        self.assertEqual(codes(r), [])

    def test_a_leg_in_no_net_needs_its_pins(self):
        data = {**SMALL, "parts": {**SMALL["parts"], "R2": {"type": "resistor"}}}
        _, r = build(data)
        self.assertIn("BAD_LEG", codes(r))

    def test_near_keeps_a_part_by_another(self):
        data = {**SMALL, "parts": {**SMALL["parts"], "C1": {"type": "capacitor", "near": "U1"}}}
        c, _ = build(data)
        u1 = [p.hole.col for p in c.parts["U1"].pins]
        c1 = [p.hole.col for p in c.parts["C1"].pins]
        self.assertLessEqual(min(abs(a - b) for a in u1 for b in c1), 3)

    def test_a_supply_by_its_name_runs_on_a_rail(self):
        data = {**SMALL, "nets": {**SMALL["nets"], "3V3": SMALL["nets"]["VSYS"]}}
        del data["nets"]["VSYS"]
        c, r = build(data)
        self.assertEqual(codes(r), [])
        self.assertTrue(any(e.hole and e.hole.row.endswith("+") for w in c.wires if w.group == "3V3"
                            for e in (w.a, w.b)))

    def test_the_layout_does_not_depend_on_hash_order(self):
        # Python orders sets by hash, which changes from run to run; seed 6 once moved the KiCad 555
        import os
        import subprocess
        import sys
        from bbgen.kicad import library_roots
        examples = ["examples/555-blinker.yaml"] + (["examples/555-kicad.yaml"] if library_roots() else [])
        script = ("import sys; sys.path.insert(0, '.'); from bbgen.circuit import Circuit; "
                  "c = Circuit.load(sys.argv[1]); "
                  "print([[c.hole_name(p.hole) if p.hole else None for p in part.pins] for part in c.parts.values()])")
        for example in examples:
            with self.subTest(example):
                outs = {subprocess.run([sys.executable, "-c", script, example], capture_output=True, text=True,
                                       check=True, env={**os.environ, "PYTHONHASHSEED": seed}).stdout
                        for seed in ("0", "6")}
                self.assertEqual(len(outs), 1, outs)

if __name__ == "__main__":
    unittest.main()
