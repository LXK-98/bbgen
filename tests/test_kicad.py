import unittest

from bbgen.circuit import Circuit, CircuitError
from bbgen.kicad import clean_name, library_roots
from bbgen.parts import make_part
from tests.helpers import build, codes, hole

HAVE_KICAD = bool(library_roots())


class NameTest(unittest.TestCase):
    def test_markup_is_dropped(self):
        self.assertEqual(clean_name("~{RST}"), "/RST")
        self.assertEqual(clean_name("V_{IN}"), "VIN")
        self.assertEqual(clean_name("GPIO22"), "GPIO22")


@unittest.skipUnless(HAVE_KICAD, "KiCad's library isn't installed")
class KicadPartTest(unittest.TestCase):
    def test_a_dip_from_its_symbol(self):
        part = make_part("U1", {"symbol": "Timer:NE555P"})
        self.assertEqual([p.name for p in part.pins], ["GND", "TRIG", "OUT", "/RST", "CONT", "THRES", "DISCH", "VCC"])
        # laid out like bbgen's own DIP: pins 1-4 along a row, 5-8 back along the row 0.3 in above
        self.assertEqual([p.offset for p in part.pins], [(0, 0), (1, 0), (2, 0), (3, 0), (3, -3), (2, -3), (1, -3), (0, -3)])
        self.assertEqual(part.source, "Timer:NE555P on Package_DIP:DIP-8_W7.62mm")

    def test_placed_and_checked_like_any_part(self):
        c, r = build({"U1": {"symbol": "Timer:NE555P", "at": "f10"}}, [["U1.VCC", "top+"], ["U1.GND", "bot-"]],
                     {"VCC": ["U1.VCC"], "GND": ["U1.GND"]})
        self.assertEqual(codes(r), [])
        self.assertEqual(hole(c, "U1", 8), "e10")

    def test_a_module_brings_its_buses_and_keepouts(self):
        part = make_part("Pico", {"symbol": "MCU_Module:RaspberryPi_Pico",
                                  "footprint": "Module:RaspberryPi_Pico_Common_THT"})
        self.assertEqual(len(part.pins), 40)
        [gnd] = part.buses
        self.assertEqual({p.name for p in gnd}, {"GND"})
        self.assertEqual([what for what, _ in part.keepouts], ["USB Cable"])

    def test_an_smd_part_on_an_adapter(self):
        part = make_part("U2", {"symbol": "Regulator_Linear:AP2112K-1.8", "adapter": {"pins": 6, "map": {5: 6}}})
        self.assertEqual(part.kind, "dip")
        self.assertEqual([p.name for p in part.pins], ["VIN", "GND", "EN", "NC", "5", "VOUT"])
        self.assertIn("pad 5 on pin 6", part.adapter)

    def test_mistakes_say_what_to_do(self):
        cases = [
            ({"symbol": "Timer:NE555X"}, "no symbol"),
            ({"symbol": "Regulator_Linear:AP2112K-1.8"}, "adapter"),
            ({"symbol": "Memory_EEPROM:24LC256"}, "names no footprint"),
            ({"footprint": "Package_TO_SOT_THT:TO-92_Inline"}, "0.1 in grid"),
            ({"symbol": "Regulator_Linear:AP2112K-1.8", "adapter": {"pins": 4}}, "map it"),
        ]
        for spec, words in cases:
            with self.subTest(spec):
                with self.assertRaises(CircuitError) as e:
                    Circuit({"parts": {"U1": spec}})
                self.assertIn(words, str(e.exception))

    def test_bbgen_places_a_kicad_part(self):
        c, r = build({"U1": {"symbol": "Timer:NE555P"}, "C1": {"type": "capacitor"}},
                     nets={"5V": ["U1.VCC", "C1.1"], "GND": ["U1.GND", "C1.2"]})
        self.assertEqual(codes(r), [])
        self.assertTrue(all(p.hole for p in c.parts["U1"].pins))


if __name__ == "__main__":
    unittest.main()
