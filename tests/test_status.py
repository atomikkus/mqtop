"""Status records and the pieces the monitor renders from them.

All pure functions, deliberately: the terminal drawing is untestable and unimportant,
while the parsing and the staleness arithmetic are what decide whether a stalled job
looks stalled.
"""
import json
import time

import pytest

from mqtop.status import (
    PREFIX,
    age,
    human_secs,
    last_line,
    parse_line,
    read_records,
    sparkline,
)


def write(path, lines):
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")
    return path


def rec(**f):
    return PREFIX + json.dumps({"job": "j", **f})


# --- parsing ------------------------------------------------------------------


def test_a_status_line_parses_and_ordinary_output_does_not():
    assert parse_line(rec(i=3))["i"] == 3
    assert parse_line("[  12] IN-423-BZFLA9  40x  2000 crops") is None
    assert parse_line("") is None


def test_malformed_json_returns_none_rather_than_raising():
    """A job printing a broken record must not take the monitor down with it."""
    assert parse_line(PREFIX + "{not json") is None
    assert parse_line(PREFIX + "[1,2,3]") is None, "a list is not a record"


def test_records_are_read_from_the_end_of_a_large_log(tmp_path):
    """Build logs run to tens of megabytes; the interesting record is the last one."""
    p = tmp_path / "big.log"
    filler = ["x" * 200] * 4000
    write(p, filler + [rec(i=1), "noise", rec(i=2)])
    got = read_records(p, tail_bytes=4096)
    assert [r["i"] for r in got] == [1, 2]


def test_a_partial_line_from_the_seek_is_discarded(tmp_path):
    """Seeking into the middle of a status line would yield a JSON fragment."""
    p = tmp_path / "cut.log"
    write(p, [rec(i=99, pad="y" * 3000), rec(i=100)])
    got = read_records(p, tail_bytes=512)
    assert [r["i"] for r in got] == [100]


def test_a_missing_log_is_empty_not_an_error(tmp_path):
    assert read_records(tmp_path / "nope.log") == []
    assert last_line(tmp_path / "nope.log") == ""
    assert age(tmp_path / "nope.log") is None


# --- the fallback for jobs with no emit() -------------------------------------


def test_last_line_ignores_status_records(tmp_path):
    """A job that emits both should show its human line, not its JSON."""
    p = write(tmp_path / "a.log", ["[ 1] slide one", rec(i=1)])
    assert last_line(p) == "[ 1] slide one"


def test_last_line_takes_only_what_follows_a_carriage_return(tmp_path):
    """Progress bars overwrite themselves; without this the panel renders a screenful
    of tqdm history as one line."""
    p = write(tmp_path / "b.log", ["Loading:  10%\rLoading:  50%\rLoading: 100%"])
    assert last_line(p) == "Loading: 100%"


def test_blank_and_whitespace_lines_are_skipped(tmp_path):
    p = write(tmp_path / "c.log", ["real output", "   ", ""])
    assert last_line(p) == "real output"


# --- staleness ----------------------------------------------------------------


def test_age_reports_seconds_since_the_last_write(tmp_path):
    """The signal a scrolling log cannot give: a job that stopped quietly looks
    identical to one merely between messages."""
    p = write(tmp_path / "d.log", ["hello"])
    assert age(p) == pytest.approx(0, abs=5)


def test_human_secs_reads_at_a_glance():
    assert human_secs(0) == "0s"
    assert human_secs(45) == "45s"
    assert human_secs(90) == "1m"
    assert human_secs(3600) == "1h00"
    assert human_secs(11160) == "3h06"
    assert human_secs(None) == "--"


# --- sparkline ----------------------------------------------------------------


def test_a_rising_series_renders_rising():
    s = sparkline([1, 2, 3, 4, 5])
    assert len(s) == 5
    assert s[0] < s[-1], "first block should be lower than last"


def test_the_scale_follows_the_window_not_zero():
    """These series are things like a cosine between 0.82 and 0.90. Anchored at zero
    every point renders as the same block and the panel says nothing."""
    s = sparkline([0.827, 0.842, 0.860, 0.874, 0.890])
    assert len(set(s)) > 1, f"flat sparkline {s!r} for a series that clearly moves"


def test_a_flat_series_does_not_divide_by_zero():
    assert sparkline([0.5, 0.5, 0.5]) == "▁▁▁"


def test_only_the_last_values_are_shown():
    assert len(sparkline(list(range(100)), width=8)) == 8


def test_non_numeric_entries_are_dropped():
    """A record missing the field yields None, which must not break the panel."""
    assert sparkline([1, None, 2, "x", 3]) == sparkline([1, 2, 3])


def test_an_empty_series_renders_nothing():
    assert sparkline([]) == ""
    assert sparkline([None, None]) == ""


# --- braille plots ------------------------------------------------------------


def braille_only(rows):
    return all(0x2800 <= ord(c) <= 0x28FF for row in rows for c in row)


def test_a_plot_has_the_requested_shape_and_is_all_braille():
    from mqtop.status import braille_plot

    rows = braille_plot(list(range(50)), width=20, height=4)
    assert len(rows) == 4 and all(len(r) == 20 for r in rows)
    assert braille_only(rows)


def test_a_rising_series_climbs_the_canvas():
    """The whole point of four rows rather than one: the shape has to be visible."""
    from mqtop.status import braille_plot

    rows = braille_plot(list(range(40)), width=20, height=4)
    top_ink = sum(ch != chr(0x2800) for ch in rows[0])
    bottom_ink = sum(ch != chr(0x2800) for ch in rows[-1])
    left_col_top = rows[0][0] != chr(0x2800)
    right_col_top = rows[0][-1] != chr(0x2800)
    assert top_ink and bottom_ink, "a ramp should touch both the top and bottom rows"
    assert right_col_top and not left_col_top, "the rise should end high, not start high"


def test_a_fixed_scale_keeps_an_idle_trace_flat():
    """GPU utilisation belongs on 0-100. Auto-scaling an idle stretch turns 0-2% jitter
    into a dramatic mountain range, which is worse than useless on a monitor."""
    from mqtop.status import braille_plot

    idle = [0, 1, 0, 2, 1, 0, 1, 0]
    fixed = braille_plot(idle, width=8, height=4, lo=0, hi=100)
    auto = braille_plot(idle, width=8, height=4)
    assert all(ch == chr(0x2800) for ch in fixed[0]), "idle should not reach the top row"
    assert any(ch != chr(0x2800) for ch in auto[0]), "auto-scaled, it does"


def test_values_outside_a_fixed_scale_are_clamped_not_wrapped():
    from mqtop.status import braille_plot

    rows = braille_plot([-50, 150], width=4, height=2, lo=0, hi=100)
    assert braille_only(rows)


def test_a_short_series_is_stretched_across_the_width():
    from mqtop.status import braille_plot

    rows = braille_plot([0, 10], width=10, height=2)
    inked = sum(ch != chr(0x2800) for row in rows for ch in row)
    assert inked >= 8, f"only {inked} cells inked; the series was not stretched"


def test_a_long_series_keeps_the_most_recent_samples():
    """A full ring buffer showing its oldest samples would freeze while the job ran."""
    from mqtop.status import braille_plot

    rising = braille_plot(list(range(500)), width=10, height=3)
    falling = braille_plot(list(range(500)) + list(range(20, 0, -1)), width=10, height=3)
    assert rising != falling


def test_an_empty_series_renders_blank_rows_of_the_right_shape():
    from mqtop.status import braille_plot

    rows = braille_plot([], width=12, height=3)
    assert len(rows) == 3 and all(r == " " * 12 for r in rows)
