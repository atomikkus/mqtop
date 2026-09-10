"""A terminal monitor for long jobs, drawn from their logs and the machine's own counters.

    mqtop                       whatever has been written to most recently
    mqtop --dir ~/runs          look there instead
    mqtop --job train=~/train.log:train[.]py

It is built to run *on the box*, reading local files. The monitors it replaces polled
over ssh, and a long-lived ssh session to a busy host drops often enough that several
died mid-run and sat silent while the job carried on. A monitor that lives beside the
job has no network in its display path: the connection dropping kills the view, not the
watch, and reattaching costs one command.

Two sources per job, in order of preference. A job that calls `status.emit()` gives
structured records and gets a real progress line. A job that does not still gets its
last log line and, more usefully, its **staleness** -- a scrolling log cannot tell a job
that has stopped from one that is merely quiet, which is the failure that wastes an
afternoon.

The plots are braille, 2x4 dots a cell, so a four-line panel carries a real curve. GPU
and CPU are pinned to 0-100 rather than auto-scaled: an idle stretch auto-scaled to its
own 0-2% jitter looks like a crisis.
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import subprocess
import sys
import time
from collections import deque
from pathlib import Path

from .jobs import DEFAULT_SHOW, Job, resolve
from .status import age, braille_plot, human_secs, last_line, read_records

HIST = 240                     # ~4 minutes of samples at 1 Hz
STALE_S = 300                  # quiet for this long, while still running, reads as stuck

RESET, DIM, BOLD = "\x1b[0m", "\x1b[2m", "\x1b[1m"
GREEN, YELLOW, RED, CYAN = "\x1b[32m", "\x1b[33m", "\x1b[31m", "\x1b[36m"


def gpu() -> tuple[float | None, float | None, float | None]:
    """(utilisation %, memory used GB, memory total GB). None when there is no GPU."""
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=utilization.gpu,memory.used,memory.total",
             "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=2).stdout.strip().splitlines()
        u, used, tot = (float(x) for x in out[0].split(","))
        return u, used / 1024, tot / 1024
    except Exception:
        return None, None, None


_prev_cpu = [0.0, 0.0]


def cpu_pct() -> float:
    """Whole-machine CPU from /proc/stat deltas -- no psutil, no sampling sleep."""
    try:
        parts = [float(x) for x in
                 Path("/proc/stat").read_text().splitlines()[0].split()[1:]]
    except OSError:
        return 0.0
    idle, total = parts[3] + parts[4], sum(parts)
    d_idle, d_total = idle - _prev_cpu[0], total - _prev_cpu[1]
    _prev_cpu[0], _prev_cpu[1] = idle, total
    return 0.0 if d_total <= 0 else max(0.0, min(100.0, 100 * (1 - d_idle / d_total)))


def mem_pct() -> tuple[float, float]:
    """(percent used, GB used)."""
    try:
        info = {}
        for line in Path("/proc/meminfo").read_text().splitlines():
            k, _, v = line.partition(":")
            info[k] = float(v.split()[0]) / 1048576      # kB -> GB
        total, avail = info["MemTotal"], info["MemAvailable"]
        return 100 * (1 - avail / total), total - avail
    except (OSError, KeyError):
        return 0.0, 0.0


def disk(where: Path) -> tuple[float, float]:
    """(free GB, percent used) for the filesystem the logs live on."""
    try:
        s = os.statvfs(str(where))
        free = s.f_bavail * s.f_frsize / 1e9
        return free, 100 * (1 - s.f_bavail / s.f_blocks)
    except (OSError, AttributeError, ZeroDivisionError):
        return 0.0, 0.0


def alive(pattern: str, exclude: set[int] | None = None) -> bool:
    """Is a process matching `pattern` running, not counting this monitor?

    `pgrep -f` searches whole command lines and excludes only itself, so a monitor
    invoked as `mqtop --job x=~/train.log` matches any pattern derived from that
    argument -- and then reports the job alive forever, whatever it is doing. Dropping
    our own pid and our shell's makes that structural rather than a matter of writing
    the pattern carefully.
    """
    drop = {os.getpid(), os.getppid()} | (exclude or set())
    try:
        r = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True,
                           timeout=2)
    except Exception:
        return False
    pids = {int(x) for x in r.stdout.split() if x.isdigit()}
    return bool(pids - drop)


def bar(pct: float, width: int) -> str:
    filled = int(round(pct / 100 * width))
    colour = RED if pct >= 90 else YELLOW if pct >= 75 else GREEN
    return f"{colour}{'█' * filled}{DIM}{'░' * (width - filled)}{RESET}"


def job_state(job: Job, running: bool, since: float | None, exists: bool) -> str:
    """The one word that says what is going on, before any colour is applied.

    Split out from the drawing because this is the judgement the monitor exists to make
    and it is the part worth testing; everything around it is escape codes.
    """
    if not exists:
        return "no log"
    if running and since is not None and since > STALE_S:
        return "stalled"
    if running:
        return "running"
    if job.match_is_guess:
        # The pattern was inferred from the filename, so "no process matched" is not
        # evidence the job ended -- the process may simply not be named after its log.
        return "quiet"
    return "ended"


def job_panel(job: Job, width: int) -> list[str]:
    recs = read_records(job.log)
    exists = job.log.exists()
    running = alive(job.match) if exists else False
    since = age(job.log)
    state = job_state(job, running, since, exists)

    dot, text = {
        "no log": (f"{DIM}○{RESET}", f"{DIM}no log{RESET}"),
        "stalled": (f"{YELLOW}◐{RESET}",
                    f"{YELLOW}stalled? {human_secs(since)} quiet{RESET}"),
        "running": (f"{GREEN}●{RESET}", f"{GREEN}running{RESET}"),
        "quiet": (f"{DIM}·{RESET}", f"{DIM}quiet {human_secs(since)}{RESET}"),
        "ended": (f"{DIM}✓{RESET}", f"{DIM}ended {human_secs(since)} ago{RESET}"),
    }[state]

    lines = [f" {dot} {BOLD}{job.name:<16}{RESET}{text}"]

    if recs:
        r = recs[-1]
        i, n = r.get("i"), r.get("n")
        prog = f"{i}/{n}" if i and n else str(i or "")
        extra = "  ".join(f"{k} {v:,}" if isinstance(v, int) else f"{k} {v}"
                          for k, v in r.items()
                          if k not in ("job", "t", "i", "n", "phase", "eta_s"))
        eta = f"eta {human_secs(r.get('eta_s'))}" if r.get("eta_s") else ""
        lines.append(f"   {DIM}{r.get('phase', ''):<8}{RESET}{prog:<12}{extra}"
                     f"  {DIM}{eta}{RESET}")
    else:
        tail = last_line(job.log)
        if tail:
            lines.append(f"   {DIM}{tail[:max(10, width - 5)]}{RESET}")
    return lines


def render(jobs: list[Job], hist: dict, width: int, source: str,
           watch_dir: Path) -> str:
    plot_w = max(20, min(width - 22, 78))
    u, gused, gtot = gpu()
    c = cpu_pct()
    mp, mused = mem_pct()
    dfree, dpct = disk(watch_dir)

    for key, val in (("gpu", u), ("cpu", c), ("mem", mp)):
        if val is not None:
            hist[key].append(val)

    host = os.uname().nodename if hasattr(os, "uname") else "local"
    right = f"{time.strftime('%H:%M:%S')}  q to quit"
    left = f"mqtop · {host}"
    pad = max(1, width - len(left) - len(right))
    out = [f"{BOLD}mqtop{RESET} {DIM}·{RESET} {host}{' ' * pad}{DIM}{right}{RESET}",
           f"{DIM}{('─' * width)}{RESET}"]

    if not jobs:
        out.append(f" {DIM}no logs found in {watch_dir}{RESET}")
    for job in jobs:
        out += job_panel(job, width)
    out.append(f"{DIM}{('─' * width)}{RESET}")

    # GPU utilisation is pinned to 0-100: an idle stretch auto-scaled to its own noise
    # reads as a crisis, which is the opposite of what a glance should convey.
    for label, key in ((f"gpu {u:.0f}%" if u is not None else "gpu --", "gpu"),
                       (f"cpu {c:.0f}%", "cpu"),
                       (f"mem {mp:.0f}%", "mem")):
        rows = braille_plot(list(hist[key]), width=plot_w, height=3, lo=0, hi=100)
        for j, row in enumerate(rows):
            tag = f"{CYAN}{label:<9}{RESET}" if j == 1 else " " * 9
            out.append(f" {tag}{row}")
        out.append("")

    gpu_mem = f"{gused:.1f}/{gtot:.0f} GB" if gused is not None else "--"
    out.append(f" {DIM}gpu mem{RESET} {gpu_mem:<14}{DIM}ram{RESET} {mused:.0f} GB")
    dlabel = f"{dfree:.0f} GB free"
    colour = RED if dpct >= 95 else YELLOW if dpct >= 85 else ""
    out.append(f" {DIM}disk{RESET}    {colour}{dlabel:<14}{RESET}"
               f"{bar(dpct, 28)} {dpct:.0f}%")
    out.append(f" {DIM}jobs from {source}{RESET}")
    return "\n".join(out)


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="mqtop", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--job", action="append", default=[], metavar="NAME=LOG[:PATTERN]",
                    help="a job to watch; repeat. Overrides any config file")
    ap.add_argument("--config", default=None, help="a mqtop.toml to read")
    ap.add_argument("--dir", default=None,
                    help="where to look for logs when nothing is configured")
    ap.add_argument("--show", type=int, default=DEFAULT_SHOW,
                    help="how many discovered logs to show")
    ap.add_argument("--max-age", type=float, default=None, metavar="HOURS",
                    help="ignore discovered logs older than this")
    ap.add_argument("--interval", type=float, default=1.0, help="seconds between frames")
    ap.add_argument("--once", action="store_true",
                    help="print one frame and exit; for scripts and for checking a "
                         "config without taking over the terminal")
    return ap


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        jobs, source = resolve(args.job, args.config, args.dir, args.show,
                               args.max_age * 3600 if args.max_age else None)
    except (ValueError, OSError) as exc:
        print(f"mqtop: {exc}", file=sys.stderr)
        return 2

    watch_dir = Path(args.dir).expanduser() if args.dir else Path.home()
    hist = {k: deque(maxlen=HIST) for k in ("gpu", "cpu", "mem")}
    cpu_pct()                       # prime the delta so the first sample is not 100%

    if args.once:
        print(render(jobs, hist, min(shutil.get_terminal_size((100, 30)).columns, 120),
                     source, watch_dir))
        return 0

    stop = {"now": False}
    signal.signal(signal.SIGINT, lambda *a: stop.__setitem__("now", True))

    # alternate screen + hidden cursor, so quitting leaves the scrollback as it was
    sys.stdout.write("\x1b[?1049h\x1b[?25l")
    try:
        while not stop["now"]:
            width = min(shutil.get_terminal_size((100, 30)).columns, 120)
            sys.stdout.write("\x1b[H\x1b[J" + render(jobs, hist, width, source,
                                                     watch_dir))
            sys.stdout.flush()
            time.sleep(args.interval)
    finally:
        sys.stdout.write("\x1b[?25h\x1b[?1049l")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
