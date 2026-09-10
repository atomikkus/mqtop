"""Which jobs the monitor shows, and how it decides whether each one is alive.

The monitor this grew out of had its four jobs written into the source, which was fine
for the one box it ran on and useless anywhere else. A job here is three things -- a
name, a log, and a pattern that finds the process -- and they come from a config file,
the command line, or, failing both, from whichever logs were written most recently.

That last case is the one that matters in practice. You ssh into a box to see what is
happening, and typing a config first defeats the point; `mqtop` with no arguments should
already be showing you the four logs that are moving.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

DEFAULT_DIR = "~"
DEFAULT_SHOW = 6
CONFIG_NAMES = ("mqtop.toml", ".mqtop.toml")


@dataclass(frozen=True)
class Job:
    name: str
    log: Path
    match: str
    #  A pattern the user wrote is a promise; one this module guessed from a filename is
    #  not, and the display says so rather than reporting "ended" for a job whose process
    #  simply has another name.
    match_is_guess: bool = False


def default_pattern(log: Path) -> str:
    """A pgrep pattern guessed from the log's name: build_corpus.log -> build_corpus[.]

    Deliberately not `[.]py`. The first live run of this watched a chain script called
    chain_distil.sh through chain_distil.log and reported it quiet while it was running,
    because the guess had assumed Python. The stem plus a literal dot matches whatever
    the extension turns out to be.

    The bracket is not decoration: `pgrep -f build_corpus.py` matches any command line
    containing that text, including the monitor's own, which made three earlier versions
    of this check report every job alive forever. `alive()` also drops our own pid, so
    the two guards are independent.
    """
    return re.escape(log.stem).replace(r"\.", "[.]").replace("\\", "") + "[.]"


def parse_job_arg(arg: str) -> Job:
    """`name=log[:pattern]`, as passed to --job.

    The pattern is optional and separated by a colon, which Windows paths also use;
    the split therefore looks for a colon after the last path separator only.
    """
    name, sep, rest = arg.partition("=")
    if not sep or not name.strip() or not rest.strip():
        raise ValueError(f"--job wants name=log[:pattern], got {arg!r}")
    tail = rest[max(rest.rfind("/"), rest.rfind("\\")) + 1:]
    if ":" in tail:
        cut = len(rest) - len(tail) + tail.index(":")
        log, pattern = rest[:cut], rest[cut + 1:]
    else:
        log, pattern = rest, ""
    p = Path(log.strip()).expanduser()
    return (Job(name.strip(), p, pattern.strip()) if pattern.strip()
            else Job(name.strip(), p, default_pattern(p), match_is_guess=True))


def find_config(start: Path | None = None) -> Path | None:
    """A config in the working directory, then in ~/.config/mqtop/."""
    here = Path(start or Path.cwd())
    for name in CONFIG_NAMES:
        if (here / name).exists():
            return here / name
    cfg = Path.home() / ".config" / "mqtop" / "jobs.toml"
    return cfg if cfg.exists() else None


def load_config(path: Path) -> tuple[list[Job], dict]:
    """`(jobs, settings)` from a TOML file. A malformed file raises; a monitor that
    silently ignored half a config would be worse than one that refuses to start."""
    import tomllib

    data = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    jobs = []
    for i, entry in enumerate(data.get("job", [])):
        if "name" not in entry or "log" not in entry:
            raise ValueError(f"{path}: job {i} needs both a name and a log")
        log = Path(str(entry["log"])).expanduser()
        pattern = str(entry.get("match", "")).strip()
        jobs.append(Job(str(entry["name"]), log,
                        pattern or default_pattern(log),
                        match_is_guess=not pattern))
    settings = {k: v for k, v in data.items() if k != "job"}
    return jobs, settings


def discover(directory: Path, limit: int = DEFAULT_SHOW,
             max_age_s: float | None = None) -> list[Job]:
    """The most recently written logs in a directory, newest first.

    Sorted by modification time rather than by name because the question a monitor
    answers is "what is happening now", and a box that has run fifty jobs over a month
    has fifty logs of which three are interesting.
    """
    import time

    d = Path(directory).expanduser()
    try:
        logs = [p for p in d.iterdir() if p.is_file() and p.suffix == ".log"]
    except OSError:
        return []
    now = time.time()
    picked = []
    for p in sorted(logs, key=lambda p: p.stat().st_mtime, reverse=True):
        if max_age_s is not None and now - p.stat().st_mtime > max_age_s:
            continue
        picked.append(Job(p.stem, p, default_pattern(p), match_is_guess=True))
        if len(picked) >= limit:
            break
    return picked


def resolve(job_args: list[str], config: Path | None, directory: str | None,
            limit: int = DEFAULT_SHOW, max_age_s: float | None = None
            ) -> tuple[list[Job], str]:
    """`(jobs, where they came from)`, in precedence order.

    Explicit --job flags win outright; a config file is next; discovery is the fallback.
    The provenance string is returned rather than logged because the monitor puts it in
    the header -- looking at a screen and not knowing which config produced it is a
    surprisingly easy way to watch the wrong machine's idea of the job list.
    """
    if job_args:
        return [parse_job_arg(a) for a in job_args], "--job"

    cfg = Path(config).expanduser() if config else find_config()
    if cfg:
        jobs, settings = load_config(cfg)
        d = directory or settings.get("dir", DEFAULT_DIR)
        if not jobs:
            return discover(Path(d), limit, max_age_s), f"{cfg} (empty, discovering)"
        return jobs, str(cfg)

    d = directory or DEFAULT_DIR
    return discover(Path(d), limit, max_age_s), f"newest logs in {d}"
