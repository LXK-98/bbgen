import unittest

from tests.helpers import codes

from bbgen.check import check
from bbgen.circuit import Circuit

PICO = {"type": "pico", "at": "h3"}
DIP = {"type": "dip", "pins": 6, "at": "e27", "rot": 180, "labels": ["VIN", "GND", "EN", "X", "Y", "VOUT"]}


def auto(parts, nets, wires=(), **extra):
    c = Circuit({"parts": parts, "nets": nets, "wires": list(wires), "autowire": True, **extra}, "test.yaml")
    return c, check(c)


def ends(c, w):
    return tuple(c.hole_name(e.hole) if e.hole else str(e.pin) for e in (w.a, w.b))


class AutowireTest(unittest.TestCase):
    def test_nets_get_connected(self):
        c, r = auto({"Pico": PICO, "U1": DIP},
                    {"VSYS": ["Pico.VSYS", "U1.VIN"], "GND": ["Pico.GND", "U1.GND"], "EN": ["Pico.GP21", "U1.EN"]})
        self.assertEqual(codes(r), [])
        self.assertTrue(all(w.auto for w in c.wires))
        # the signal runs straight from the Pico's strip to the part's
        [en] = [w for w in c.wires if w.group == "EN"]
        self.assertEqual(ends(c, en), ("b16", "b25"))

    def test_a_supply_takes_a_plus_rail(self):
        c, r = auto({"Pico": PICO, "U1": DIP}, {"VSYS": ["Pico.VSYS", "U1.VIN"]})
        self.assertEqual(codes(r), [])
        # short drops, from the row next to the rail
        self.assertEqual({frozenset(ends(c, w)) for w in c.wires},
                         {frozenset({"a4", "top+4"}), frozenset({"a27", "top+27"})})

    def test_a_lone_ground_takes_the_minus_rails(self):
        c, r = auto({"Pico": PICO, "U1": DIP}, {"GND": ["Pico.GND", "U1.GND"]})
        self.assertEqual(codes(r), [])
        rails = {e.hole.row for w in c.wires for e in (w.a, w.b) if e.hole and e.hole.is_rail}
        self.assertTrue(rails <= {"top-", "bot-"} and rails)

    def test_rail_names_a_rail(self):
        c, r = auto({"U1": DIP}, {"VCC": {"pins": ["U1.VIN", "U1.VOUT"], "rail": "bot+"}})
        self.assertEqual(codes(r), [])
        self.assertTrue(any("bot+" in end for w in c.wires for end in ends(c, w)))

    def test_a_rail_can_carry_one_net(self):
        _, r = auto({"C1": {"type": "capacitor", "pins": ["top+10", "top-10"]}},
                    {"A": ["C1.1"], "B": {"pins": ["C1.2"], "rail": "top+"}})
        self.assertIn("BAD_RAIL", codes(r))

    def test_offboard_pins_are_leaves(self):
        c, r = auto({"Pico": PICO, "J1": {"type": "offboard", "cols": 2, "labels": {1: "GND", 2: "GND", 3: "GND"}}},
                    {"GND": ["Pico.GND", "J1.1", "J1.2", "J1.3"]})
        self.assertEqual(codes(r), [])
        for n in (1, 2, 3):
            leads = [w for w in c.wires if any(e.pin is not None and e.pin.num == n for e in (w.a, w.b))]
            self.assertEqual(len(leads), 1, f"J1.{n}")
            self.assertTrue(all(e.pin is None for w in leads for e in (w.a, w.b) if e.pin is None or e.pin.num != n))

    def test_wires_in_the_file_are_kept(self):
        c, r = auto({"Pico": PICO, "U1": DIP}, {"EN": ["Pico.GP21", "U1.EN"]}, [["Pico.GP21", "a25"]])
        self.assertEqual(codes(r), [])
        self.assertEqual([w.auto for w in c.wires], [False])

    def test_a_wire_across_a_part_is_bent_around_it(self):
        opamp = {"type": "dip", "pins": 6, "at": "e42", "rot": 180, "labels": ["OUT", "V-", "IN+", "IN-", "X", "V+"]}
        c, r = auto({"U1": opamp}, {"FB": ["U1.OUT", "U1.IN-"]})
        self.assertEqual(codes(r), [])
        [w] = c.wires
        self.assertTrue(w.via)
        self.assertNotIn("OVER_PART", codes(r, "warning"))

    def test_numbered_rails_first(self):
        c, _ = auto({"Pico": PICO, "U1": DIP},
                    {"EN": ["Pico.GP21", "U1.EN"], "GND": ["Pico.GND", "U1.GND"]})
        self.assertEqual(c.wires[0].group, "GND")
        self.assertEqual([w.num for w in c.wires], list(range(1, len(c.wires) + 1)))

    def test_ends_stay_out_from_under_two_lead_parts(self):
        # R1 stands from b25 up to the rail, over a25; c25 and d25 are still free for the wire
        c, r = auto({"Pico": PICO, "U1": DIP, "R1": {"type": "resistor", "pins": ["b25", "top-25"]}},
                    {"EN": ["Pico.GP21", "U1.EN", "R1.1"], "GND": ["R1.2"]})
        [en] = [w for w in c.wires if w.group == "EN"]
        self.assertNotIn("a25", ends(c, en))
        self.assertNotIn("OVER_PART", codes(r, "warning"))

    def test_flying_leads_prefer_a_rail(self):
        u1 = {"type": "dip", "pins": 6, "at": "e58", "rot": 180, "labels": ["A", "GND", "B", "C", "D", "E"]}
        c, r = auto({"U1": u1, "J1": {"type": "offboard", "labels": ["GND"]}}, {"GND": ["U1.GND", "J1.1"]})
        [lead] = [w for w in c.wires if w.b.pin is not None or w.a.pin is not None]
        board_end = lead.a if lead.a.hole else lead.b
        self.assertTrue(board_end.hole.is_rail, ends(c, lead))


if __name__ == "__main__":
    unittest.main()
