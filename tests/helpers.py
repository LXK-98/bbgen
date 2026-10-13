from bbgen.check import check
from bbgen.circuit import Circuit


def build(parts, wires=(), nets=None, boards=None):
    data = {"parts": parts, "wires": list(wires), "nets": nets or {}}
    if boards:
        data["boards"] = boards
    c = Circuit(data, "test.yaml")
    return c, check(c)


def codes(result, level="error"):
    return sorted(i.code for i in result.issues if i.level == level)


def hole(c, ref, num):
    pin = c.parts[ref].find(str(num))[0]
    return c.hole_name(pin.hole) if pin.hole else None
