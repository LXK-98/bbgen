import unittest

from bbgen.check import check
from bbgen.render import BAND, GOLD, SILVER, _outline, render, resistor_bands
from tests.helpers import build

NAMES = {**dict(zip(BAND, "black brown red orange yellow green blue violet grey white".split())),
         GOLD: "gold", SILVER: "silver"}


def bands(value):
    b = resistor_bands(value)
    return b and " ".join(NAMES[c] for c in b)


class BandTest(unittest.TestCase):
    def test_two_digit_values(self):
        for value, expect in [
            ("10k", "brown black orange gold"), ("330R", "orange orange brown gold"),
            ("4k7", "yellow violet red gold"), ("4.7k", "yellow violet red gold"),
            ("2.2M", "red red green gold"), ("100", "brown black brown gold"),
            ("220 Ω", "red red brown gold"), ("1R5", "brown green gold gold"),
            ("0.47", "yellow violet silver gold"), ("0R", "black"),
        ]:
            with self.subTest(value):
                self.assertEqual(bands(value), expect)

    def test_three_digits_get_five_bands(self):
        self.assertEqual(bands("4.99k"), "yellow white white brown brown")
        self.assertEqual(bands("25.6R"), "red green blue gold brown")

    def test_unreadable_values_get_no_bands(self):
        for value in ["", "k", "banana", "1.2.3", "10µF"]:
            with self.subTest(value):
                self.assertIsNone(resistor_bands(value))

    def test_bands_are_drawn(self):
        c, result = build({"R1": {"type": "resistor", "value": "4k7", "pins": ["a1", "a5"]}})
        svg = render(c, result)
        for color in (BAND[4], BAND[7], BAND[2], GOLD):
            self.assertIn(f'fill="{color}"', svg)


class OutlineTest(unittest.TestCase):
    def test_dark_wires_get_a_light_outline(self):
        self.assertNotEqual(_outline("#262626"), "#1d1d1d")
        for color in ("#1f5fbf", "#f7f7f7", "#d62728", "not-a-hex"):
            with self.subTest(color):
                self.assertEqual(_outline(color), "#1d1d1d")


if __name__ == "__main__":
    unittest.main()
