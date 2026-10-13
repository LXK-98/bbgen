import unittest

from bbgen.board import Board, Hole, parse_hole


class BoardTest(unittest.TestCase):
    def test_rail_holes(self):
        self.assertEqual(len(Board(1, "full").rail_cols), 50)
        self.assertEqual(len(Board(1, "half").rail_cols), 25)
        self.assertEqual(Board(1, "full").rail_cols[:6], [3, 4, 5, 6, 7, 9])

    def test_parse(self):
        self.assertEqual(parse_hole("a12"), (None, "a", 12))
        self.assertEqual(parse_hole("2:top+5"), ("2", "top+", 5))
        self.assertEqual(parse_hole("bot-"), (None, "bot-", None))
        self.assertIsNone(parse_hole("U1.3"))
        self.assertIsNone(parse_hole("k3"))

    def test_strips(self):
        b = Board(1, "full")
        self.assertEqual(b.strip(Hole("1", "a", 12)), b.strip(Hole("1", "e", 12)))
        self.assertNotEqual(b.strip(Hole("1", "e", 12)), b.strip(Hole("1", "f", 12)))
        self.assertEqual(b.strip(Hole("1", "top+", 3)), b.strip(Hole("1", "top+", 61)))
        split = Board(1, "full", split_rails=True)
        self.assertNotEqual(split.strip(Hole("1", "top+", 3)), split.strip(Hole("1", "top+", 61)))
        self.assertEqual(len(split.strip_holes(split.strip(Hole("1", "top+", 3)))), 25)

    def test_rail_order(self):
        b = Board(1, "full", rails={"top": "-+"})
        self.assertLess(b.rail_y("top-"), b.rail_y("top+"))
        with self.assertRaises(ValueError):
            Board(1, "full", rails={"top": "++"})

    def test_hole_at_round_trip(self):
        b = Board(1, "half")
        for h in b.holes():
            self.assertEqual(b.hole_at(*b.xy(h)), h)
        self.assertIsNone(b.hole_at(*b.xy(Hole("1", "e", 4))[:1], 8.5))


if __name__ == "__main__":
    unittest.main()
