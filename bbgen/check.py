"""Wiring check: what the breadboard actually connects, compared with the nets."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass

from .circuit import Circuit, End, Issue, Wire
from .parts import LEAD_KINDS, Pin


class _DSU:
    def __init__(self):
        self.parent: dict = {}

    def find(self, x):
        self.parent.setdefault(x, x)
        while self.parent[x] != x:
            self.parent[x] = self.parent[self.parent[x]]
            x = self.parent[x]
        return x

    def union(self, a, b) -> None:
        self.parent[self.find(a)] = self.find(b)


def end_node(c: Circuit, end: End):
    if end.pin is not None:
        return ("off", end.pin.part.ref, end.pin.num)
    return ("strip", *c.boards[end.hole.board].strip(end.hole))


@dataclass
class Result:
    issues: list[Issue]
    strip_pins: dict      # board strip -> pins sitting in it
    root_nets: dict       # connection root -> net names found on it
    dsu: _DSU

    @property
    def errors(self) -> list[Issue]:
        return [i for i in self.issues if i.level == "error"]

    @property
    def warnings(self) -> list[Issue]:
        return [i for i in self.issues if i.level == "warning"]

    def nets_at(self, node) -> list[str]:
        return self.root_nets.get(self.dsu.find(node), [])


def _short_list(pins, limit: int = 6) -> str:
    names = [str(p) for p in pins]
    return ", ".join(names[:limit]) + (f" and {len(names) - limit} more" if len(names) > limit else "")


def check(c: Circuit) -> Result:
    dsu = _DSU()
    issues = list(c.issues)
    pin_node: dict[Pin, tuple] = {}
    strip_pins: dict[tuple, list[Pin]] = defaultdict(list)

    for part in c.parts.values():
        for pin in part.pins:
            if pin.hole is not None:
                strip = c.boards[pin.hole.board].strip(pin.hole)
                strip_pins[strip].append(pin)
                pin_node[pin] = ("strip", *strip)
            elif part.kind == "offboard":
                pin_node[pin] = ("off", part.ref, pin.num)
            else:
                continue
            dsu.find(pin_node[pin])
        for bus in part.buses:
            nodes = [pin_node[p] for p in bus if p in pin_node]
            for node in nodes[1:]:
                dsu.union(nodes[0], node)

    ends: dict[tuple, list[tuple[Wire, End]]] = defaultdict(list)
    for w in c.wires:
        if w.a is None or w.b is None:
            continue
        na, nb = end_node(c, w.a), end_node(c, w.b)
        dsu.union(na, nb)
        if na == nb:
            issues.append(Issue("warning", "NO_EFFECT", f"W{w.num} has both ends in the same strip",
                                holes=[w.a.hole]))
        for end, node in ((w.a, na), (w.b, nb)):
            if end.hole is not None:
                ends[node].append((w, end))
    for node, here in ends.items():
        if len(here) == 1 and not strip_pins.get(node[1:]):
            w, end = here[0]
            issues.append(Issue("warning", "DANGLING", f"W{w.num} ends in {c.hole_name(end.hole)}, "
                                                       "where nothing else is connected", holes=[end.hole]))

    net_of: dict[Pin, str] = {}
    for net in c.nets.values():
        for pin in net.pins:
            if pin in net_of and net_of[pin] != net.name:
                issues.append(Issue("error", "TWO_NETS", f"{pin} is in both {net_of[pin]} and {net.name}",
                                    pins=[pin]))
            net_of.setdefault(pin, net.name)

    for net in c.nets.values():
        missing = [p for p in net.pins if p not in pin_node]
        if missing:
            issues.append(Issue("error", "NOT_PLACED", f"net {net.name}: {_short_list(missing)} "
                                                       "not on the board", pins=missing))
        pieces: dict = defaultdict(list)
        for pin in net.pins:
            if pin in pin_node:
                pieces[dsu.find(pin_node[pin])].append(pin)
        if len(pieces) > 1:
            text = " | ".join("{" + _short_list(g, 4) + "}" for g in pieces.values())
            issues.append(Issue("error", "OPEN", f"net {net.name} is in {len(pieces)} unconnected pieces: {text}",
                                pins=[p for g in pieces.values() for p in g]))

    groups: dict = defaultdict(list)
    for pin, node in pin_node.items():
        groups[dsu.find(node)].append(pin)
    root_nets: dict = {}
    for root, pins in groups.items():
        nets = sorted({net_of[p] for p in pins if p in net_of})
        root_nets[root] = nets
        if len(nets) > 1:
            issues.append(Issue("error", "SHORT", f"nets {', '.join(nets)} are connected together "
                                                  f"({_short_list(pins)})", pins=pins))
        units = {p.part.bus_of(p) or p for p in pins}
        if len(units) < 2:
            continue
        reported = set()
        for pin in pins:
            unit = pin.part.bus_of(pin) or pin
            if pin in net_of or unit in reported:
                continue
            reported.add(unit)
            what = f"net {nets[0]}" if len(nets) == 1 else _short_list([q for q in pins if q is not pin], 4)
            issues.append(Issue("error", "STRAY", f"{pin} is connected to {what} but isn't in any net",
                                pins=[pin]))

    for part in c.parts.values():
        if part.kind in LEAD_KINDS and all(p in pin_node for p in part.pins):
            a, b = (dsu.find(pin_node[p]) for p in part.pins)
            if a == b:
                issues.append(Issue("error", "SHORTED_PART", f"both legs of {part.ref} are in the same connection",
                                    pins=list(part.pins)))

    issues.sort(key=lambda i: i.level != "error")
    return Result(issues, dict(strip_pins), root_nets, dsu)


def wire_nets(c: Circuit, result: Result, w: Wire) -> list[str]:
    return result.nets_at(end_node(c, w.a)) if w.a else []


def wire_color(c: Circuit, result: Result, w: Wire) -> str:
    if w.color:
        return w.color
    nets = wire_nets(c, result, w)
    return c.nets[nets[0]].color if len(nets) == 1 else "gray"
