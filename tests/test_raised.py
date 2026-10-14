import unittest

from tests.helpers import codes

from bbgen.check import check
from bbgen.checklist import checklist
from bbgen.circuit import Circuit

# a 1.0 in module in rows b and j: the strips of its pins in row j are under it, f to i
MODULE = {"type": "dip", "pins": 8, "width": 10, "at": "j20", "raised": True,
          "labels": ["VCC", "A", "B", "GND", "X", "Y", "Z", "W"]}


def build(data):
    c = Circuit(data, "test.yaml")
    return c, check(c)


class RaisedTest(unittest.TestCase):
    def test_holes_under_it_are_free_for_flat_wires(self):
        c, r = build({"parts": {"U1": MODULE}, "wires": [["U1.A", "f30"]], "nets": {"A": ["U1.A"]}})
        self.assertNotIn("HOLE_TAKEN", codes(r))
        self.assertEqual(c.under(c.wires[0]), ["U1"])

    def test_without_raised_they_are_taken(self):
        c, r = build({"parts": {"U1": {**MODULE, "raised": False}}, "wires": [["U1.A", "f30"]], "nets": {"A": ["U1.A"]}})
        self.assertIn("BAD_WIRE", codes(r))  # no free hole left in its strip

    def test_no_leg_and_no_plug_goes_under_it(self):
        c, r = build({"parts": {"U1": MODULE, "R1": {"type": "resistor", "pins": ["g21", "g30"]}},
                      "wires": [{"from": "h22", "to": "a40", "flex": True}], "nets": {}})
        messages = [i.message for i in r.errors if i.code == "HOLE_TAKEN"]
        self.assertTrue(any("R1 leg" in m for m in messages), messages)
        self.assertTrue(any("flexible jumper" in m for m in messages), messages)

    def test_a_wire_never_crosses_its_headers(self):
        # VCC (pin 1, j20) to the bottom rail: not straight down across the row of pins, but out past its end
        c, r = build({"parts": {"U1": MODULE}, "nets": {"VCC": {"pins": ["U1.VCC"], "rail": "+"},
                                                        "GND": {"pins": ["U1.GND"], "rail": "-"}}, "autowire": True})
        self.assertEqual(codes(r) + codes(r, "warning"), [])
        for w in c.wires:
            if not w.flex:
                self.assertFalse(c._bodies_crossed(c.end_xy(w.a), c.end_xy(w.b)), c.hole_name(w.a.hole))

    def test_the_checklist_puts_it_in_after_the_wires_under_it(self):
        c, r = build({"parts": {"U1": MODULE}, "wires": [["U1.A", "f30"]], "nets": {"A": ["U1.A"]}})
        md = checklist(c, r)
        self.assertLess(md.index("**W1**"), md.index("**U1**"))
        self.assertIn("runs under U1", md)

    def test_placement_puts_a_wide_raised_module_on_one_board(self):
        module = {k: v for k, v in MODULE.items() if k != "at"}
        nets = {"V": {"pins": ["U1.VCC", "U1.W"], "rail": "+"}, "G": {"pins": ["U1.GND", "U1.X"], "rail": "-"},
                "S": ["U1.A", "U1.Y"]}
        c, r = build({"parts": {"U1": module}, "nets": nets})
        self.assertEqual(codes(r), [])


if __name__ == "__main__":
    unittest.main()
