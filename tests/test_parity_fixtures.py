"""Parity helpers: Python and Rust must agree on ##ST parse and job_state labels."""
from pathlib import Path

from mqtop.status import parse_line
from mqtop.jobs import Job
from mqtop.tui import job_state, STALE_S

FIXTURES = Path(__file__).parent / "fixtures"


def test_fixture_status_lines_parse():
    text = (FIXTURES / "mixed_status.log").read_text(encoding="utf-8")
    recs = [parse_line(l) for l in text.splitlines() if parse_line(l)]
    assert len(recs) == 2
    assert recs[0]["i"] == 40
    assert recs[1].get("run_id") == "exp1"


def test_job_state_matrix_matches_documented_labels():
    j = Job("j", Path("j.log"), "j[.]py", match_is_guess=False)
    assert job_state(j, True, 5, True) == "running"
    assert job_state(j, True, STALE_S + 1, True) == "stalled"
    assert job_state(j, False, 10, True) == "ended"
    g = Job("j", Path("j.log"), "j[.]py", match_is_guess=True)
    assert job_state(g, False, 10, True) == "quiet"
    assert job_state(j, False, 15, True, shared_log=True) == "not running"
