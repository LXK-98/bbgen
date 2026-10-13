"""A video of bbgen at work (`bbgen build --video x.mp4`): one frame per step of placing and wiring.

Placement and wiring call step() as they go; it does nothing unless a recording is running. A frame
is the full drawing of the circuit as it stands, captioned with what the step did, the parts and
wires it changed ringed in orange. By default only the steps bbgen keeps are recorded (the starting
layouts that beat the one before, the moves that improved the score, each wire as it goes in);
with every=True the rejected tries are in too, briefly. The frames become a video with ImageMagick
(SVG to PNG) and ffmpeg.
"""

from __future__ import annotations

import math
import re
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from pathlib import Path

KEPT_SECONDS = 0.8      # a placement step bbgen kept
TRIED_SECONDS = 0.25    # one it threw away (every=True)
WIRE_SECONDS = 0.3      # a wire going in
LAST_SECONDS = 3.0      # the finished layout
DENSITY = 120           # pixels per inch of the drawing: big enough to read the labels


class Recorder:
    def __init__(self, every: bool = False):
        self.every = every
        self.frames: list[tuple[str, float]] = []  # (svg, seconds on screen)
        self.placing = 0                            # inside autoplace: its trial builds' wiring isn't shown

    def step(self, c, caption: str, kept: bool = True, focus=(), seconds: float | None = None,
             wiring: bool = False) -> None:
        if (wiring and self.placing) or (not kept and not self.every):
            return
        from .check import check
        from .render import render
        seconds = seconds or (WIRE_SECONDS if wiring else KEPT_SECONDS if kept else TRIED_SECONDS)
        self.frames.append((render(c, check(c), caption=caption, focus=focus), seconds))


_recorder: Recorder | None = None


def step(c, caption: str, **kw) -> None:
    """Record a frame, if a recording is running."""
    if _recorder is not None:
        _recorder.step(c, caption, **kw)


def active() -> bool:
    return _recorder is not None


@contextmanager
def recording(every: bool = False):
    global _recorder
    _recorder = Recorder(every)
    try:
        yield _recorder
    finally:
        _recorder = None


@contextmanager
def placing():
    """Placement's trial builds wire themselves too; only the final build's wiring makes frames."""
    if _recorder is not None:
        _recorder.placing += 1
    try:
        yield
    finally:
        if _recorder is not None:
            _recorder.placing -= 1


def _size(svg: str) -> tuple[int, int]:
    m = re.search(r'width="(\d+)" height="(\d+)"', svg)
    return int(m.group(1)), int(m.group(2))


def write_video(frames: list[tuple[str, float]], out: Path) -> None:
    """Frames to an .mp4 (or .gif, by out's suffix). Needs ImageMagick and ffmpeg."""
    missing = [tool for tool in ("magick", "ffmpeg") if not shutil.which(tool)]
    if missing:
        raise RuntimeError(f"a video needs {' and '.join(missing)} on PATH")
    if not frames:
        raise RuntimeError("nothing to record: no step was taken")
    # every frame on one canvas, as big as the biggest, in even pixels for the video codec
    width = max(_size(svg)[0] for svg, _ in frames) * DENSITY / 96  # the SVG's sizes are CSS pixels
    height = max(_size(svg)[1] for svg, _ in frames) * DENSITY / 96
    width, height = 2 * math.ceil(width / 2), 2 * math.ceil(height / 2)
    with tempfile.TemporaryDirectory(prefix="bbgen-video-") as tmp:
        tmp = Path(tmp)

        def to_png(i: int) -> Path:
            svg, png = tmp / f"{i:05}.svg", tmp / f"{i:05}.png"
            svg.write_text(frames[i][0], encoding="utf-8")
            subprocess.run(["magick", "-density", str(DENSITY), "-background", "white", str(svg),
                            "-gravity", "northwest", "-extent", f"{width}x{height}", str(png)],
                           check=True, capture_output=True)
            return png

        with ThreadPoolExecutor() as pool:
            pngs = list(pool.map(to_png, range(len(frames))))
        lines = []
        for path, (_, seconds) in zip(pngs, frames):
            lines += [f"file '{path.name}'", f"duration {seconds}"]
        lines.append(f"file '{pngs[-1].name}'")  # the concat demuxer drops the last duration otherwise
        (tmp / "frames.txt").write_text("\n".join(lines) + "\n")
        if out.suffix.lower() == ".gif":
            codec = ["-vf", "fps=10,split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"]
        else:
            codec = ["-vf", "fps=25", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20"]
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i",
                        str(tmp / "frames.txt"), *codec, str(out.resolve())], check=True, cwd=tmp)
