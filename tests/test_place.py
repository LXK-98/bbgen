import unittest

from tests.helpers import build, codes, hole


class PlacementTest(unittest.TestCase):
    def test_pico(self):
        c, r = build({"Pico": {"type": "pico", "at": "h3"}})
        self.assertEqual([hole(c, "Pico", n) for n in (1, 20, 21, 39, 40)], ["h3", "h22", "c22", "c4", "c3"])
        self.assertEqual(codes(r), [])

    def test_pico_body_blocks_the_holes_under_it(self):
        c, r = build({"Pico": {"type": "pico", "at": "h3"}}, [["e10", "top+"]])
        self.assertEqual(codes(r), ["HOLE_TAKEN"])
        c, r = build({"Pico": {"type": "pico", "at": "h3"}}, [["Pico.GP0", "top+"], ["Pico.GP0", "top+"],
                                                                 ["Pico.GP0", "top+"]])
        self.assertIn("no free hole", next(i.message for i in r.issues if i.code == "BAD_WIRE"))

    def test_dip_rotations(self):
        c, _ = build({"U1": {"type": "dip", "pins": 6, "at": "e27", "rot": 180}})
        self.assertEqual([hole(c, "U1", n) for n in range(1, 7)], ["e27", "e26", "e25", "f25", "f26", "f27"])
        c, _ = build({"U1": {"type": "dip", "pins": 8, "at": "f10"}})
        self.assertEqual([hole(c, "U1", n) for n in (1, 4, 5, 8)], ["f10", "f13", "e13", "e10"])
        c, _ = build({"U1": {"type": "dip", "pins": 6, "at": "a10", "rot": 90}})
        self.assertEqual([hole(c, "U1", n) for n in range(1, 7)], ["a10", "b10", "c10", "c13", "b13", "a13"])

    def test_pin_in_the_trench(self):
        _, r = build({"U1": {"type": "dip", "pins": 4, "width": 2, "at": "f10"}})
        self.assertEqual(codes(r), ["OFF_HOLES", "OFF_HOLES"])

    def test_auto_holes_prefer_the_pin_then_the_other_end(self):
        c, _ = build({"Pico": {"type": "pico", "at": "h3"}}, [["Pico.VSYS", "top+"], ["Pico.3", "bot-"]])
        names = [(c.hole_name(w.a.hole), c.hole_name(w.b.hole)) for w in c.wires]
        self.assertEqual(names, [("b4", "top+4"), ("i5", "bot-5")])

    def test_leads_and_offboard(self):
        c, r = build({"J1": {"type": "offboard", "cols": 2, "labels": {1: "A", 2: "B"}},
                      "R1": {"type": "resistor", "pins": ["a5", "a9"]}},
                     [["b5", "J1.A"], ["b9", "J1.2"]],
                     {"N1": ["R1.1", "J1.1"], "N2": ["R1.2", "J1.B"]})
        self.assertEqual(codes(r), [])
        self.assertEqual(c.wires[0].b.pin.name, "A")


if __name__ == "__main__":
    unittest.main()
