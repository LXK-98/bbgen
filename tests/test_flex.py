import unittest

from tests.helpers import build, codes
from tests.test_autowire import DIP, auto, ends

from bbgen.checklist import checklist
from bbgen.circuit import Circuit, CircuitError
from bbgen.render import render

U2 = dict(DIP, at="e10")
# wires along rows a-d and g-j between columns 14 and 24: no straight way from U2 to U1 is left
BLOCKED = [[f"{r}14", f"{r}24"] for r in "abcdghij"]


class FlexTest(unittest.TestCase):
    def test_autowire_arches_over_what_it_cannot_lie_past(self):
        c, r = auto({"U1": DIP, "U2": U2}, {"EN": ["U1.EN", "U2.EN"]}, BLOCKED)
        [w] = [w for w in c.wires if w.auto]
        self.assertTrue(w.flex)
        self.assertEqual(ends(c, w), ("d25", "d8"))
        self.assertEqual(codes(r) + codes(r, "warning"), [])

    def test_flex_false_keeps_every_wire_on_the_board(self):
        c, r = auto({"U1": DIP, "U2": U2}, {"EN": ["U1.EN", "U2.EN"]}, BLOCKED, flex=False)
        self.assertFalse(any(w.flex for w in c.wires))
        self.assertEqual(codes(r, "warning"), ["DIAGONAL"])

    def test_a_straight_wire_still_wins(self):
        c, r = auto({"U1": DIP, "U2": U2}, {"EN": ["U1.EN", "U2.EN"]})
        [w] = c.wires
        self.assertFalse(w.flex)
        self.assertEqual(codes(r, "warning"), [])

    def test_a_flexible_jumper_in_the_file(self):
        parts = {"U1": DIP, "U2": U2}
        wires = [{"from": "U2.EN", "to": "U1.EN", "flex": True}, *BLOCKED]
        c, r = build(parts, wires, {"EN": ["U1.EN", "U2.EN"]})
        w = c.wires[0]
        self.assertTrue(w.flex)
        self.assertEqual(codes(r) + codes(r, "warning"), [])  # it may slant: it's off the board
        self.assertFalse(any(who == "W1" for _, _, who in c._segments))  # and lies along nothing
        md = checklist(c, r)
        flex_part = md.split("## Flexible jumpers")[1]
        self.assertIn("**W1**", flex_part)
        self.assertIn("10 cm flexible jumper", flex_part)
        self.assertIn("flexible jumper (plug in last)", render(c, r))

    def test_no_bends_in_a_flexible_jumper(self):
        with self.assertRaises(CircuitError) as e:
            Circuit({"wires": [{"from": "a1", "to": "a5", "flex": True, "via": ["c3"]}]})
        self.assertIn("no via", str(e.exception))


if __name__ == "__main__":
    unittest.main()


class TallPartTest(unittest.TestCase):
    def test_no_wire_lies_across_a_can(self):
        # a can standing in row c, cols 20-21: a straight wire along row c from 15 to 25 would pass under it
        can = {"type": "electrolytic", "pins": ["c20", "c21"]}
        c, r = auto({"C1": can, "R1": {"type": "resistor", "pins": ["d13", "d15"]},
                     "R2": {"type": "resistor", "pins": ["d25", "d27"]}},
                    {"X": ["R1.2", "R2.1"], "A": ["C1.1"], "B": ["C1.2"], "P": ["R1.1"], "Q": ["R2.2"]})
        self.assertEqual(codes(r), [])
        for w in c.wires:  # every wire lying on the board keeps clear of it
            if not w.flex and w.a.hole and w.b.hole:
                self.assertNotIn("C1", c._bodies_crossed(c.end_xy(w.a), c.end_xy(w.b)), w.num)
        self.assertIn("C1", c._bodies_crossed((15, 5), (25, 5)))  # (which a run along row c would cross)
