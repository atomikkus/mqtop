"""One-line status records, so a monitor never has to parse a log format.

Every long job in this project has been watched by grepping its human-readable log, and
every one of those greps has needed rewriting when the log changed -- five or six times
across the distillation work, once silently, so a monitor sat quiet through a run that
had already finished.

A job instead prints one machine-readable line beside its normal output:

    ##ST {"job": "build_corpus", "phase": "crop", "i": 100, "n": 694, "kept": 84}

The fields are the job's own business; nothing here validates them. What is fixed is the
prefix and that the rest is one line of JSON, which is the whole contract. Adding a job
to the monitor costs one `emit()` call and no change to the monitor.

Reading is tail-first: a build log runs to tens of megabytes and the interesting record
is always the last one.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

PREFIX = "##ST "
TAIL_BYTES = 262_144        # plenty for a few hundred records, cheap to re-read at 1 Hz


def emit(job: str, **fields) -> None:
    """Print one status record. Flushed, because a monitor reading a redirected log sees
    nothing until the buffer does -- which is how a job can look stalled while it runs."""
    rec = {"job": job, "t": time.time(), **fields}
    print(PREFIX + json.dumps(rec, separators=(",", ":"), default=str), flush=True)


def parse_line(line: str) -> dict | None:
    """A status record, or None for ordinary log output. Never raises: a job that
    prints a malformed record should not take the monitor down with it."""
    if not line.startswith(PREFIX):
        return None
    try:
        rec = json.loads(line[len(PREFIX):])
    except (ValueError, TypeError):
        return None
    return rec if isinstance(rec, dict) else None


def read_records(path, tail_bytes: int = TAIL_BYTES) -> list[dict]:
    """Status records from the end of a log. Missing file gives an empty list."""
    p = Path(path)
    try:
        size = p.stat().st_size
        with open(p, "rb") as fh:
            if size > tail_bytes:
                fh.seek(size - tail_bytes)
                fh.readline()          # discard the partial line the seek landed in
            blob = fh.read()
    except OSError:
        return []
    out = []
    for raw in blob.decode("utf-8", "replace").splitlines():
        rec = parse_line(raw)
        if rec is not None:
            out.append(rec)
    return out


def last_line(path, tail_bytes: int = 8192) -> str:
    """The final non-empty, non-status line -- what a job with no `emit()` still shows.

    Progress bars overwrite themselves with carriage returns, so only the segment after
    the last CR is real; without that a tqdm line renders as a screenful of history.
    """
    p = Path(path)
    try:
        size = p.stat().st_size
        with open(p, "rb") as fh:
            if size > tail_bytes:
                fh.seek(size - tail_bytes)
            blob = fh.read()
    except OSError:
        return ""
    for raw in reversed(blob.decode("utf-8", "replace").splitlines()):
        text = raw.split("\r")[-1].strip()
        if text and not text.startswith(PREFIX):
            return text
    return ""


def age(path) -> float | None:
    """Seconds since the file was last written, or None if it does not exist.

    This is the staleness signal, and it is the one a scrolling log cannot give: a job
    that has quietly stopped looks identical to one that is merely between messages.
    """
    try:
        return max(0.0, time.time() - Path(path).stat().st_mtime)
    except OSError:
        return None


def human_secs(s: float | None) -> str:
    if s is None:
        return "--"
    s = int(s)
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}m"
    return f"{s // 3600}h{(s % 3600) // 60:02d}"


BLOCKS = "▁▂▃▄▅▆▇█"


def sparkline(values, width: int = 12) -> str:
    """A fixed-width sparkline over the last `width` values.

    Scaled to the window's own min and max rather than to zero: these series are things
    like a cosine that lives between 0.82 and 0.90, and anchoring at zero would render
    every point as the same block.
    """
    vals = [v for v in values if isinstance(v, (int, float))][-width:]
    if not vals:
        return ""
    lo, hi = min(vals), max(vals)
    if hi - lo < 1e-12:
        return BLOCKS[0] * len(vals)
    span = hi - lo
    return "".join(BLOCKS[min(len(BLOCKS) - 1,
                              int((v - lo) / span * len(BLOCKS)))] for v in vals)


# Braille gives 2x4 dots per character cell, which is what lets a four-line panel show a
# real curve rather than a row of bar glyphs. Dot numbering in Unicode is column-major
# and skips about, so the mapping is written out rather than computed.
_DOT = {(0, 0): 0x01, (0, 1): 0x02, (0, 2): 0x04, (0, 3): 0x40,
        (1, 0): 0x08, (1, 1): 0x10, (1, 2): 0x20, (1, 3): 0x80}


def _resample(values: list[float], n: int) -> list[float]:
    """Stretch or crop a series to exactly n points.

    Cropping keeps the *last* n: a monitor showing the oldest samples on a full ring
    buffer would freeze while the job kept moving.
    """
    if len(values) >= n:
        return values[-n:]
    if len(values) == 1:
        return values * n
    out = []
    span = len(values) - 1
    for i in range(n):
        pos = i * span / (n - 1)
        lo = int(pos)
        hi = min(lo + 1, span)
        frac = pos - lo
        out.append(values[lo] * (1 - frac) + values[hi] * frac)
    return out


def braille_plot(values, width: int = 60, height: int = 4,
                 lo: float | None = None, hi: float | None = None) -> list[str]:
    """A line plot in `height` rows of `width` characters.

    `lo` and `hi` fix the scale where the range is known and meaningful -- GPU
    utilisation belongs on 0-100 whatever the samples happen to be, or an idle stretch
    renders as dramatic noise. Left unset the scale follows the data, which is what a
    loss curve or a cosine wants.
    """
    vals = [float(v) for v in values if isinstance(v, (int, float))]
    if not vals or width < 1 or height < 1:
        return [" " * width for _ in range(height)]

    cols, rows = width * 2, height * 4
    pts = _resample(vals, cols)
    lo = min(pts) if lo is None else lo
    hi = max(pts) if hi is None else hi
    span = (hi - lo) or 1.0

    canvas = [[0] * width for _ in range(height)]
    for x, v in enumerate(pts):
        frac = min(1.0, max(0.0, (v - lo) / span))
        y = rows - 1 - int(frac * (rows - 1))          # row 0 is the top
        canvas[y // 4][x // 2] |= _DOT[(x % 2, y % 4)]
    return ["".join(chr(0x2800 + c) for c in row) for row in canvas]
