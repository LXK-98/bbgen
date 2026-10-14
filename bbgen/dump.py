"""A record of one build, stage by stage, for proving that another implementation of bbgen (the Rust port)
makes the same choices in the same order, and for finding the first one where it doesn't.

Placement and wiring call event() as they go; it does nothing unless a record is being made. What's
recorded: the footprint choices placement weighs, each annealing run's best layouts, every trial layout
built with its score and which ones were kept, the file as placed, then the final build's parts, rails and
wires as they go in (with the cost each wire was chosen at) and come out. Placement's trial builds wire
themselves too; their wiring isn't recorded (build=True events are dropped inside placing()), since a
trial's layout and score are, and building that layout on its own records its wiring.

Values are written as plain JSON: tuples as lists, holes by name, and floats as Python has them (repr
round-trips exactly), with inf as the string "inf". A record forces placement to build its trials one
at a time, as a video does: a worker's events would be lost.
"""

from __future__ import annotations

import math
from contextlib import contextmanager

_events: list | None = None
_placing = 0


def active() -> bool:
    return _events is not None


def event(kind: str, build: bool = False, **fields) -> None:
    """Record an event, if a record is being made (a build=True one only outside placement's trials)."""
    if _events is not None and not (build and _placing):
        _events.append({"kind": kind, **{k: plain(v) for k, v in fields.items()}})


def plain(v):
    if isinstance(v, float):
        return v if math.isfinite(v) else ("inf" if v > 0 else "-inf" if v < 0 else "nan")
    if isinstance(v, (list, tuple)):
        return [plain(x) for x in v]
    if isinstance(v, dict):
        return {str(k): plain(x) for k, x in v.items()}
    if isinstance(v, (str, int, bool)) or v is None:
        return v
    return str(v)


@contextmanager
def recording():
    global _events
    _events = []
    try:
        yield _events
    finally:
        _events = None


@contextmanager
def placing():
    global _placing
    _placing += 1
    try:
        yield
    finally:
        _placing -= 1
