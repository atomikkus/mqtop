"""mqtop -- a terminal monitor for long jobs, and the status records they emit.

Two halves that are useful apart. `mqtop.status.emit()` is what a job calls to say where
it has got to; it is one print of a prefixed JSON line and depends on nothing. The
monitor reads those records if they are there, and falls back to the log's last line and
its staleness if they are not, so it is useful against a job that has never heard of it.
"""

from .status import (
    PREFIX,
    age,
    braille_plot,
    emit,
    human_secs,
    last_line,
    parse_line,
    read_records,
    sparkline,
)

__version__ = "0.1.0"
__all__ = ["PREFIX", "age", "braille_plot", "emit", "human_secs", "last_line",
           "parse_line", "read_records", "sparkline"]
