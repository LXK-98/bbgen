import unittest

from tests.helpers import codes

from bbgen.check import check
from bbgen.circuit import Circuit, CircuitError

STACKED = [{"type": "half"}, {"type": "half", "below": 1}]
WIDE = {"type": "dip", "pins": 8, "width": 10, "labels": ["VCC", "A", "B", "GND", "X", "Y", "Z", "W"]}


def build(data):
    c = Circuit(data, "test.yaml")
    return c, check(c)


class StackTest(unittest.TestCase):
    def test_a_board_below_another_sits_on_its_grid(self):
        c, _ = build({"boards": STACKED})
        up, down = c.boards["1"], c.boards["2"]
        self.assertEqual(up.x0, down.x0)
        # its row a is 1.0 in under the upper board's row i
        self.assertAlmostEqual(down.y("a") - up.y("i"), 10)
        self.assertGreater(down.top, up.bottom)

    def test_below_names_a_board_before_it(self):
        with self.assertRaisesRegex(CircuitError, "below"):
            build({"boards": [{"type": "half", "below": 2}, "half"]})

    def test_a_wide_part_spans_two_stacked_boards(self):
        c, r = build({"boards": STACKED, "parts": {"U1": {**WIDE, "at": "i10", "rot": 180}},
                      "nets": {"A": ["U1.2"]}})
        boards = {(p.hole.board, p.hole.row) for p in c.parts["U1"].pins}
        self.assertEqual(boards, {("1", "i"), ("2", "a")})
        self.assertNotIn("OFF_HOLES", codes(r))

    def test_placement_spans_them_too(self):
        nets = {"V": {"pins": ["U1.VCC", "U1.W"], "rail": "+"}, "G": {"pins": ["U1.GND", "U1.X"], "rail": "-"},
                "S": ["U1.A", "U1.Y"]}
        c, r = build({"boards": STACKED, "parts": {"U1": WIDE}, "nets": nets})
        self.assertEqual(codes(r), [])
        self.assertEqual({p.hole.board for p in c.parts["U1"].pins}, {"1", "2"})

    def test_wires_between_boards_are_flexible_jumpers(self):
        nets = {"S": ["U1.A", "U1.Y"]}  # pin 2 on the upper board, pin 6 on the lower
        c, r = build({"boards": STACKED, "parts": {"U1": {**WIDE, "at": "i10", "rot": 180}}, "nets": nets,
                      "autowire": True})
        self.assertEqual(codes(r), [])
        for w in c.wires:
            if w.a.hole and w.b.hole and w.a.hole.board != w.b.hole.board:
                self.assertTrue(w.flex, (c.hole_name(w.a.hole), c.hole_name(w.b.hole)))
        self.assertTrue(any(w.flex for w in c.wires))


if __name__ == "__main__":
    unittest.main()
