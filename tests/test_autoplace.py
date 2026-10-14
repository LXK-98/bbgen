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

    def test_a_wide_dip_straddles_the_trench(self):
        # a 0.6 in adapter: its two rows either side of the trench, a hole free by every pin
        adapter = {"type": "dip", "pins": 8, "width": 6, "labels": ["A", "B", "GND", "D", "E", "F", "G", "VCC"]}
        data = {"parts": {"U1": adapter, "U2": {"type": "dip", "pins": 6, "labels": ["X", "Y", "GND", "P", "Q", "VCC"]}},
                "nets": {"GND": ["U1.GND", "U2.GND"], "VCC": ["U1.VCC", "U2.VCC"], "S1": ["U1.A", "U2.X"],
                         "S2": ["U1.F", "U2.Q"]}}
        c, r = build(data)
        self.assertEqual(codes(r), [])
        rows = {p.hole.row for p in c.parts["U1"].pins}
        self.assertEqual(len(rows), 2)
        self.assertTrue(rows & set("abcde") and rows & set("fghij"))

    def test_a_dip_too_wide_for_the_board_says_so(self):
        data = {"parts": {"U1": {"type": "dip", "pins": 8, "width": 10}}, "nets": {"A": ["U1.1", "U1.8"]}}
        with self.assertRaisesRegex(Exception, "1 in DIP"):
            build(data)

    def test_parts_spread_over_two_boards(self):
        # four 16-pin chips don't fit on one half-size board: they go on both, each on one board only
        chip = {"type": "dip", "pins": 16}
        parts = {f"U{i}": dict(chip) for i in range(1, 5)}
        nets = {"GND": [f"U{i}.8" for i in range(1, 5)], "VCC": [f"U{i}.16" for i in range(1, 5)],
                **{f"S{i}": [f"U{i}.1", f"U{i + 1}.9"] for i in range(1, 4)}}
        c, r = build({"boards": ["half", "half"], "parts": parts, "nets": nets})
        self.assertEqual(codes(r), [])
        for ref in parts:
            self.assertEqual(len({p.hole.board for p in c.parts[ref].pins}), 1, ref)
        self.assertEqual({c.parts[ref].pins[0].hole.board for ref in parts}, {"1", "2"})

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
