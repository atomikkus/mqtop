"""What the monitor concludes about a job, separately from how it draws it.

The drawing is escape codes and is not worth testing. The judgement is: given a log
that has not moved for six minutes and a process that is still there, is this job
working or stuck? That call is the reason the monitor exists.
"""
from pathlib import Path

import pytest

from mqtop import tui
from mqtop.jobs import Job


def job(tmp_path, name="j", guess=False, make=True):
    log = tmp_path / f"{name}.log"
    if make:
        log.write_text("working\n", encoding="utf-8")
    return Job(name, log, f"{name}[.]py", match_is_guess=guess)


def test_a_live_job_writing_regularly_is_running(tmp_path):
    assert tui.job_state(job(tmp_path), running=True, since=5, exists=True) == "running"


def test_a_live_job_that_has_gone_quiet_is_flagged(tmp_path):
    """The failure a scrolling log cannot show: the process is there, the work is not.
    This session lost time to it twice before the check existed."""
    assert tui.job_state(job(tmp_path), running=True, since=tui.STALE_S + 1,
                         exists=True) == "stalled"


def test_a_finished_job_with_a_known_pattern_reads_as_ended(tmp_path):
    assert tui.job_state(job(tmp_path), running=False, since=10, exists=True) == "ended"


def test_a_finished_job_with_a_guessed_pattern_only_reads_as_quiet(tmp_path):
    """The pattern came from the filename, so "no process matched" is not evidence the
    job ended -- it may just not be a .py of that name. Saying "ended" would be a
    confident wrong answer, which is worse than an uncertain right one."""
    assert tui.job_state(job(tmp_path, guess=True), running=False, since=10,
                         exists=True) == "quiet"


def test_a_missing_log_says_so_rather_than_reading_as_finished(tmp_path):
    assert tui.job_state(job(tmp_path, make=False), running=False, since=None,
                         exists=False) == "no log"


def test_a_panel_prefers_a_status_record_over_the_last_log_line(tmp_path):
    """A job that emits records gets real progress; the fallback is for jobs that do
    not, and must not shadow the better source."""
    log = tmp_path / "j.log"
    log.write_text('some ordinary output\n'
                   '##ST {"job":"j","phase":"crop","i":40,"n":100,"kept":31}\n'
                   'more ordinary output\n', encoding="utf-8")
    lines = tui.job_panel(Job("j", log, "nothing_matches_this[.]py"), width=100)
    body = "\n".join(lines)
    assert "40/100" in body and "kept 31" in body
    assert "more ordinary output" not in body


def test_a_panel_falls_back_to_the_last_line_when_there_are_no_records(tmp_path):
    log = tmp_path / "j.log"
    log.write_text("[ 400] IN-423-BZF3AY 40x 2000 crops\n", encoding="utf-8")
    body = "\n".join(tui.job_panel(Job("j", log, "no_such[.]py"), width=100))
    assert "IN-423-BZF3AY" in body


def test_a_long_log_line_is_cut_to_the_terminal_width(tmp_path):
    log = tmp_path / "j.log"
    log.write_text("x" * 400 + "\n", encoding="utf-8")
    body = "\n".join(tui.job_panel(Job("j", log, "no_such[.]py"), width=60))
    assert "x" * 300 not in body


def test_one_frame_renders_without_a_gpu_or_proc(tmp_path, monkeypatch):
    """It has to run on a laptop as well as the box -- otherwise it cannot be developed
    anywhere but in production."""
    monkeypatch.setattr(tui, "gpu", lambda: (None, None, None))
    from collections import deque

    hist = {k: deque(maxlen=10) for k in ("gpu", "cpu", "mem")}
    frame = tui.render([job(tmp_path)], hist, 100, "test", tmp_path)
    assert "mqtop" in frame and "gpu --" in frame


def test_an_empty_job_list_says_where_it_looked(tmp_path, monkeypatch):
    """A blank panel with no explanation reads as "nothing is running" when the truth
    is "I was pointed at the wrong directory" -- which is exactly how a monitor comes
    to sit silent through a live run."""
    monkeypatch.setattr(tui, "gpu", lambda: (None, None, None))
    from collections import deque

    hist = {k: deque(maxlen=10) for k in ("gpu", "cpu", "mem")}
    frame = tui.render([], hist, 100, "newest logs in /tmp", tmp_path)
    assert "no logs found" in frame and str(tmp_path) in frame


def test_the_header_names_the_source_of_the_job_list(tmp_path, monkeypatch):
    monkeypatch.setattr(tui, "gpu", lambda: (None, None, None))
    from collections import deque

    hist = {k: deque(maxlen=10) for k in ("gpu", "cpu", "mem")}
    frame = tui.render([job(tmp_path)], hist, 100, "~/mqtop.toml", tmp_path)
    assert "jobs from ~/mqtop.toml" in frame


def test_once_prints_a_frame_and_exits(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(tui, "gpu", lambda: (None, None, None))
    (tmp_path / "run.log").write_text("hello\n", encoding="utf-8")
    assert tui.main(["--dir", str(tmp_path), "--once"]) == 0
    assert "mqtop" in capsys.readouterr().out


def test_a_bad_job_flag_exits_with_a_message_not_a_traceback(tmp_path, capsys):
    assert tui.main(["--job", "nonsense", "--once"]) == 2
    assert "name=log" in capsys.readouterr().err


# --- liveness -------------------------------------------------------------------


def test_the_monitor_never_counts_itself_as_the_job(monkeypatch):
    """pgrep -f matches whole command lines, so `mqtop --job x=~/train.log` matches any
    pattern derived from that path. Three earlier versions of this check reported every
    job alive forever because of it."""
    import os
    import subprocess

    class R:
        stdout = f"{os.getpid()}\n{os.getppid()}\n"

    monkeypatch.setattr(subprocess, "run", lambda *a, **k: R())
    assert tui.alive("anything") is False


def test_a_real_other_process_still_counts(monkeypatch):
    import os
    import subprocess

    class R:
        stdout = f"{os.getpid()}\n999999\n"

    monkeypatch.setattr(subprocess, "run", lambda *a, **k: R())
    assert tui.alive("anything") is True


def test_no_match_is_not_alive(monkeypatch):
    import subprocess

    class R:
        stdout = ""

    monkeypatch.setattr(subprocess, "run", lambda *a, **k: R())
    assert tui.alive("anything") is False


def test_a_broken_pgrep_reads_as_not_running_rather_than_crashing(monkeypatch):
    """A monitor that dies because pgrep is missing is worse than one that under-reports
    liveness; staleness still carries the signal."""
    import subprocess

    def boom(*a, **k):
        raise FileNotFoundError("pgrep")

    monkeypatch.setattr(subprocess, "run", boom)
    assert tui.alive("anything") is False


# --- several stages writing one log ----------------------------------------------


def test_a_stage_that_has_not_started_does_not_claim_to_have_ended(tmp_path):
    """Three stages of a chain wrote one log, and the two that had not started yet each
    reported "ended 15s ago" -- reading the age of a log another stage was writing."""
    assert tui.job_state(job(tmp_path), running=False, since=15, exists=True,
                         shared_log=True) == "not running"


def test_a_stage_with_its_own_log_still_reports_when_it_ended(tmp_path):
    assert tui.job_state(job(tmp_path), running=False, since=15, exists=True,
                         shared_log=False) == "ended"


def test_a_shared_log_is_only_quoted_under_the_stage_that_is_writing_it(tmp_path):
    """Otherwise one line of output appears three times and reads as three jobs making
    the same progress."""
    log = tmp_path / "chain.log"
    log.write_text("[teacher] 178,304/1,000,000\n", encoding="utf-8")
    idle = tui.job_panel(Job("distil", log, "distil[.]py"), 100, shared_log=True)
    assert len(idle) == 1 and "178,304" not in "".join(idle)


def test_stages_sharing_a_log_are_detected_from_the_job_list(tmp_path, monkeypatch):
    monkeypatch.setattr(tui, "gpu", lambda: (None, None, None))
    monkeypatch.setattr(tui, "alive", lambda *a, **k: False)
    from collections import deque

    log = tmp_path / "chain.log"
    log.write_text("progress\n", encoding="utf-8")
    jobs = [Job("teacher", log, "teacher[.]py"), Job("distil", log, "distil[.]py")]
    hist = {k: deque(maxlen=10) for k in ("gpu", "cpu", "mem")}
    frame = tui.render(jobs, hist, 100, "test", tmp_path)
    assert frame.count("not running") == 2 and "ended" not in frame
