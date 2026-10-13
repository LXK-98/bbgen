import unittest

from bbgen.checklist import checklist
from bbgen.render import render
from tests.helpers import build, codes

PICO = {"type": "pico", "at": "h22"}  # USB to the left: its cable needs e13-e20 and f13-f20


class KeepoutTest(unittest.TestCase):
    def test_the_usb_cable_reserves_holes(self):
        c, _ = build({"Pico": PICO})
        held = sorted(c.hole_name(h) for h, who in c.occupied.items() if who == "Pico's USB cable")
        self.assertEqual(held, [f"{r}{n}" for r in "ef" for n in range(13, 21)])

    def test_nothing_goes_in_it(self):
        _, r = build({"Pico": PICO}, [["e18", "top-"]])
        self.assertEqual(codes(r), ["HOLE_TAKEN"])
        self.assertIn("Pico's USB cable", r.issues[0].message)

    def test_nothing_runs_over_it(self):
        _, r = build({"Pico": PICO}, [["d15", "g15"]])
        msg = next(i.message for i in r.issues if i.code == "OVER_PART")
        self.assertIn("runs over Pico's USB cable", msg)

    def test_drawn_and_in_the_checklist(self):
        c, r = build({"Pico": PICO})
        self.assertIn("url(#keepout)", render(c, r))
        self.assertIn("keep `e13`-`e20` and `f13`-`f20` clear for its USB cable", checklist(c, r))

    def test_off_the_board_it_costs_nothing(self):
        c, r = build({"Pico": {"type": "pico", "at": "h3"}})
        held = [h for h, who in c.occupied.items() if who == "Pico's USB cable"]
        self.assertEqual(sorted(c.hole_name(h) for h in held), ["e1", "f1"])
        self.assertEqual(codes(r), [])


if __name__ == "__main__":
    unittest.main()
