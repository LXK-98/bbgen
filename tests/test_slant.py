import unittest

from bbgen.circuit import Circuit, CircuitError
from tests.helpers import build, codes

PICO = {"type": "pico", "at": "h3"}
DIP = {"type": "dip", "pins": 6, "at": "e27", "rot": 180, "labels": ["VIN", "GND", "CE", "NC", "X", "VOUT"]}


def names(c, w):
    return c.hole_name(w.a.hole), c.hole_name(w.b.hole)


def diagonal(r):
    return [i.message for i in r.issues if i.code == "DIAGONAL"]


class StraightTest(unittest.TestCase):
    def test_auto_ends_line_up(self):
        # GP21 is in row c with only a and b free above it; CE is in e25 with a-d free
        c, r = build({"Pico": PICO, "U1": DIP}, [["Pico.GP21", "U1.CE"]])
        self.assertEqual(names(c, c.wires[0]), ("b16", "b25"))
        self.assertEqual(diagonal(r), [])

    def test_parallel_runs_take_different_rows(self):
        c, r = build({"Pico": PICO, "U1": DIP}, [["Pico.GP21", "U1.CE"], ["Pico.GP22", "U1.VIN"]])
        self.assertEqual(names(c, c.wires[1]), ("a14", "a27"))
        self.assertEqual(diagonal(r), [])

    def test_leg_lines_up_with_a_written_hole(self):
        c, r = build({"Pico": PICO, "U1": DIP, "R1": {"type": "resistor", "pins": ["U1.CE", "a30"]}})
        self.assertEqual(c.hole_name(c.parts["R1"].pins[0].hole), "a25")
        self.assertEqual(diagonal(r), [])

class DiagonalTest(unittest.TestCase):
    def test_rail_gap_names_the_part_and_the_gaps(self):
        _, r = build({"U1": DIP}, [["U1.GND", "top-"]])
        [msg] = diagonal(r)
        self.assertIn("column 26 has no top- hole", msg)
        self.assertIn("8, 14, 20, 26", msg)
        self.assertIn("Move U1", msg)

    def test_written_holes_get_a_straight_alternative(self):
        _, r = build({"R1": {"type": "resistor", "pins": ["bot+47", "j48"]}})
        self.assertEqual(diagonal(r), ["R1 slants (bot+47 → j48): bot+48 instead of bot+47 would make it straight"])

    def test_blocked_routes_name_what_blocks_them(self):
        _, r = build({}, [["a10", "a20"], ["b10", "b20"], ["c10", "c20"], ["d10", "d20"], ["e10", "e20"],
                          ["e11", "a19"]])
        [msg] = diagonal(r)
        self.assertTrue(msg.startswith("W6 slants (e11 → a19): every straight route is blocked"), msg)
        self.assertIn("W1", msg)

    def test_trench_crossings_and_flying_leads_are_quiet(self):
        _, r = build({"J1": {"type": "offboard", "pins": 1}}, [["d10", "g12"], ["a5", "J1.1"]])
        self.assertEqual(diagonal(r), [])

    def test_a_reason_silences_it(self):
        _, r = build({"U1": DIP}, [{"from": "U1.GND", "to": "top-", "slant": "U1 can't move"}])
        self.assertEqual(diagonal(r), [])
        _, r = build({"R1": {"type": "resistor", "pins": ["bot+47", "j48"], "slant": "fits better"}})
        self.assertEqual(diagonal(r), [])

    def test_a_reason_must_be_given(self):
        with self.assertRaises(CircuitError):
            Circuit({"wires": [{"from": "a1", "to": "b5", "slant": True}]})

    def test_warnings_dont_fail_the_check(self):
        _, r = build({"U1": DIP}, [["U1.GND", "top-"]])
        self.assertEqual(codes(r), [])


if __name__ == "__main__":
    unittest.main()
