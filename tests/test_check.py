import unittest

from tests.helpers import build, codes

PICO = {"type": "pico", "at": "h3"}


def blink(**changes):
    parts = {"Pico": PICO, "R1": {"type": "resistor", "pins": ["Pico.GP15", "j27"]},
             "D1": {"type": "led", "pins": ["R1.2", "bot-"]}}
    wires = [["Pico.3", "bot-"]]
    nets = {"GND": ["Pico.GND", "D1.K"], "DRIVE": ["Pico.GP15", "R1.1"], "LED": ["R1.2", "D1.A"]}
    parts.update(changes.get("parts", {}))
    nets.update(changes.get("nets", {}))
    return build(parts, changes.get("wires", wires), nets)


class CheckTest(unittest.TestCase):
    def test_good(self):
        _, r = blink()
        self.assertEqual(r.issues, [])

    def test_open(self):
        _, r = blink(wires=[])
        self.assertEqual(codes(r), ["OPEN"])

    def test_short(self):
        # the resistor's far leg lands in the Pico GP16 strip
        _, r = blink(parts={"R1": {"type": "resistor", "pins": ["Pico.GP15", "b22"]},
                            "D1": {"type": "led", "pins": ["a22", "top-"]}},
                     wires=[["Pico.38", "top-"]], nets={"GP16": ["Pico.GP16"]})
        self.assertIn("SHORT", codes(r))

    def test_stray(self):
        # GP16 shares the strip with R1's far leg but is in no net
        _, r = blink(parts={"R1": {"type": "resistor", "pins": ["Pico.GP15", "b22"]},
                            "D1": {"type": "led", "pins": ["a22", "top-"]}},
                     wires=[["Pico.38", "top-"]])
        self.assertEqual(codes(r), ["STRAY"])

    def test_shorted_part(self):
        _, r = blink(parts={"C1": {"type": "capacitor", "pins": ["a40", "b40"]}},
                     nets={"X": ["C1.1"], "Y": ["C1.2"]})
        self.assertIn("SHORTED_PART", codes(r))
        self.assertIn("SHORT", codes(r))

    def test_two_nets_and_bad_refs(self):
        _, r = blink(nets={"X": ["Pico.GP15"], "Y": ["Nope.1", "Pico.GP99"]})
        self.assertEqual(codes(r), ["BAD_NET", "BAD_NET", "TWO_NETS"])

    def test_dangling_and_no_effect(self):
        _, r = blink(wires=[["Pico.3", "bot-"], ["a40", "a50"], ["a45", "b45"]])
        self.assertEqual(codes(r, "warning"), ["DANGLING", "DANGLING", "NO_EFFECT"])

    def test_bus_pin_counts_for_the_whole_bus(self):
        _, r = blink(nets={"GND": ["Pico.38", "D1.K"]})
        self.assertEqual(r.issues, [])

    def test_ambiguous_wire_end(self):
        _, r = build({"U1": {"type": "dip", "pins": 4, "at": "f10", "labels": ["GND", "A", "B", "GND"]}},
                     [["U1.GND", "top-"]])
        self.assertIn("ambiguous", r.issues[0].message)


if __name__ == "__main__":
    unittest.main()
