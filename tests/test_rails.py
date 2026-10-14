import unittest

from tests.helpers import codes
from tests.test_autowire import auto, ends

from bbgen.checklist import checklist
from bbgen.circuit import Circuit, CircuitError
from bbgen.measure import rail_links_inside

SUPPLY = {"type": "offboard", "labels": ["A", "B", "C"]}


def rails_of(c, net):
    return {e.hole.row for w in c.wires if w.group == net for e in (w.a, w.b) if e.hole and e.hole.is_rail}


class PolarityTest(unittest.TestCase):
    def test_two_supplies_get_a_plus_rail_each(self):
        c, r = auto({"J1": SUPPLY, "C1": {"type": "capacitor", "pins": ["a10", "a12"]},
                     "C2": {"type": "capacitor", "pins": ["j10", "j12"]}},
                    {"5V": {"pins": ["J1.1", "C1.1"], "rail": "+"}, "3V3": {"pins": ["J1.2", "C2.1"], "rail": "+"},
                     "GND": {"pins": ["J1.3", "C1.2", "C2.2"], "rail": "-"}})
        self.assertEqual(codes(r), [])
        self.assertEqual((rails_of(c, "5V"), rails_of(c, "3V3")), ({"top+"}, {"bot+"}))

    def test_too_many_supplies_for_the_rails(self):
        nets = {n: {"pins": [f"J1.{i}"], "rail": "+"} for i, n in enumerate(("5V", "3V3", "12V"), 1)}
        with self.assertRaises(CircuitError) as e:
            Circuit({"parts": {"J1": SUPPLY}, "nets": nets})
        self.assertIn("3 positive supplies (5V, 3V3, 12V)", str(e.exception))
        self.assertIn("split_rails", str(e.exception))
        # split rails have four + strips: enough
        Circuit({"boards": [{"type": "full", "split_rails": True}], "parts": {"J1": SUPPLY}, "nets": nets})

    def test_a_guess_is_named_in_the_error_and_the_checklist(self):
        nets = {"VIN": ["J1.1"], "VCC": ["J1.2"], "5V": ["J1.3"]}
        with self.assertRaises(CircuitError) as e:
            Circuit({"parts": {"J1": SUPPLY}, "nets": nets})
        self.assertIn("rail: none", str(e.exception))
        nets["VIN"] = {"pins": ["J1.1"], "rail": "none"}
        c, r = auto({"J1": SUPPLY}, nets)
        md = checklist(c, r)
        self.assertIn("ASSUMED: `VCC` is a supply", md)
        self.assertNotIn("`VIN`", md)

    def test_a_ground_on_a_plus_rail(self):
        _, r = auto({"C1": {"type": "capacitor", "pins": ["top+10", "a10"]}},
                    {"GND": {"pins": ["C1.1"], "rail": "-"}, "X": ["C1.2"]})
        self.assertIn("BAD_RAIL", codes(r))

    def test_rails_are_linked_at_their_ends(self):
        # ground pins on both sides of the trench, under a DIP: each drops to its own side's rail, and the two
        # rails are joined at an end of the board, not in the middle of the circuit
        dip = {"type": "dip", "pins": 8, "at": "e30", "rot": 180,
               "labels": ["G1", "X", "Y", "Z", "G2", "P", "Q", "R"]}
        c, r = auto({"U1": dip, "J1": SUPPLY}, {"GND": {"pins": ["U1.G1", "U1.G2", "J1.1"], "rail": "-"}})
        self.assertEqual(codes(r), [])
        self.assertEqual(rail_links_inside(c), 0)


class LedColorTest(unittest.TestCase):
    def test_colours(self):
        from tests.helpers import build
        from bbgen.render import render
        c, r = build({"D1": {"type": "led", "color": "green", "pins": ["a10", "a12"]}})
        self.assertIn("#3fc35a", render(c, r))  # its lens
        with self.assertRaises(CircuitError):
            Circuit({"parts": {"D1": {"type": "led", "color": "chartreuse"}}})


if __name__ == "__main__":
    unittest.main()
