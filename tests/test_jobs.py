"""Which jobs get watched, and where that list comes from.

This is the part that decides whether the monitor is looking at the right files at all.
The monitor it replaces had four logs written into its source; when the pipeline grew a
new stage, the monitor kept reporting the old four as ended and showed nothing about the
job that was actually running. Every test here exists to keep that from recurring.
"""
import time

import pytest

from mqtop import jobs


# --- parsing a --job flag ------------------------------------------------------


def test_a_job_flag_carries_name_log_and_pattern():
    j = jobs.parse_job_arg("train=/var/log/train.log:train[.]py")
    assert j.name == "train" and j.match == "train[.]py"
    assert j.log.name == "train.log" and "log" in j.log.parts
    assert not j.match_is_guess


def test_a_pattern_is_guessed_from_the_log_name_when_omitted():
    j = jobs.parse_job_arg("build=/tmp/build_corpus.log")
    assert j.match == "build_corpus[.]"
    assert j.match_is_guess, "a guess must be marked as one"


def test_the_guess_does_not_assume_python():
    """Found on the first live run: a chain script called chain_distil.sh, watched
    through chain_distil.log, read as quiet while it was running -- the guess had
    hardcoded .py."""
    assert jobs.default_pattern(jobs.Path("/tmp/chain_distil.log")) == "chain_distil[.]"


def test_a_windows_drive_letter_is_not_mistaken_for_a_pattern():
    """C:\\logs\\train.log has a colon in it, and splitting on the first one would
    leave the monitor watching a file called "C"."""
    j = jobs.parse_job_arg(r"train=C:\logs\train.log")
    assert str(j.log).endswith("train.log")
    assert "logs" in str(j.log)


def test_a_pattern_after_a_windows_path_still_parses():
    j = jobs.parse_job_arg(r"train=C:\logs\train.log:python.*train")
    assert j.match == "python.*train"
    assert str(j.log).endswith("train.log")


@pytest.mark.parametrize("bad", ["", "noequals", "=/tmp/a.log", "name="])
def test_a_malformed_job_flag_is_refused(bad):
    with pytest.raises(ValueError, match="name=log"):
        jobs.parse_job_arg(bad)


def test_the_guessed_pattern_cannot_match_the_monitors_own_command_line():
    """pgrep -f searches whole command lines, so a bare `build_corpus.py` matches the
    monitor itself whenever the name appears in its arguments -- and then every job
    reads as alive forever. Three earlier versions of this check had that bug."""
    assert "[.]" in jobs.default_pattern(jobs.Path("/tmp/build_corpus.log"))


# --- discovery -----------------------------------------------------------------


def write_log(d, name, age_s=0.0):
    p = d / name
    p.write_text("hello\n", encoding="utf-8")
    if age_s:
        t = time.time() - age_s
        import os

        os.utime(p, (t, t))
    return p


def test_discovery_finds_the_logs_that_moved_most_recently(tmp_path):
    """A box that has run fifty jobs has fifty logs, of which three matter. Sorting by
    name would show the alphabet; sorting by mtime shows what is happening."""
    write_log(tmp_path, "ancient.log", age_s=90_000)
    write_log(tmp_path, "old.log", age_s=5_000)
    write_log(tmp_path, "fresh.log", age_s=1)
    found = jobs.discover(tmp_path, limit=2)
    assert [j.name for j in found] == ["fresh", "old"]


def test_discovery_can_ignore_logs_older_than_a_cutoff(tmp_path):
    write_log(tmp_path, "ancient.log", age_s=90_000)
    write_log(tmp_path, "fresh.log", age_s=1)
    found = jobs.discover(tmp_path, limit=5, max_age_s=3600)
    assert [j.name for j in found] == ["fresh"]


def test_only_log_files_are_discovered(tmp_path):
    write_log(tmp_path, "run.log")
    (tmp_path / "notes.txt").write_text("x", encoding="utf-8")
    (tmp_path / "sub").mkdir()
    assert [j.name for j in jobs.discover(tmp_path)] == ["run"]


def test_a_missing_directory_discovers_nothing_rather_than_raising(tmp_path):
    assert jobs.discover(tmp_path / "nowhere") == []


def test_discovered_patterns_are_marked_as_guesses(tmp_path):
    """The display leans on this: "no process matched" means the job ended only if we
    knew what to look for. For a guessed pattern it means nothing, and saying "ended"
    would be a lie."""
    write_log(tmp_path, "run.log")
    assert all(j.match_is_guess for j in jobs.discover(tmp_path))


# --- config files ---------------------------------------------------------------


def test_a_config_file_supplies_names_logs_and_patterns(tmp_path):
    (tmp_path / "mqtop.toml").write_text(
        'dir = "/srv/logs"\n'
        '[[job]]\nname = "teacher"\nlog = "/srv/logs/t.log"\nmatch = "teacher[.]py"\n'
        '[[job]]\nname = "student"\nlog = "/srv/logs/s.log"\n',
        encoding="utf-8")
    js, settings = jobs.load_config(tmp_path / "mqtop.toml")
    assert [j.name for j in js] == ["teacher", "student"]
    assert js[0].match == "teacher[.]py" and not js[0].match_is_guess
    assert js[1].match == "s[.]" and js[1].match_is_guess
    assert settings["dir"] == "/srv/logs"


def test_a_job_missing_its_log_is_refused_rather_than_skipped(tmp_path):
    """Half-loading a config gives a monitor that looks complete and is not."""
    (tmp_path / "mqtop.toml").write_text('[[job]]\nname = "a"\n', encoding="utf-8")
    with pytest.raises(ValueError, match="needs both a name and a log"):
        jobs.load_config(tmp_path / "mqtop.toml")


# --- precedence ------------------------------------------------------------------


def test_an_explicit_job_flag_beats_a_config_file(tmp_path, monkeypatch):
    (tmp_path / "mqtop.toml").write_text(
        '[[job]]\nname = "from_config"\nlog = "/tmp/c.log"\n', encoding="utf-8")
    monkeypatch.chdir(tmp_path)
    js, source = jobs.resolve(["cli=/tmp/x.log"], None, None)
    assert [j.name for j in js] == ["cli"] and source == "--job"


def test_a_config_in_the_working_directory_beats_discovery(tmp_path, monkeypatch):
    write_log(tmp_path, "stray.log")
    (tmp_path / "mqtop.toml").write_text(
        '[[job]]\nname = "configured"\nlog = "/tmp/c.log"\n', encoding="utf-8")
    monkeypatch.chdir(tmp_path)
    js, source = jobs.resolve([], None, str(tmp_path))
    assert [j.name for j in js] == ["configured"] and "mqtop.toml" in source


def test_with_nothing_configured_it_discovers_and_says_so(tmp_path, monkeypatch):
    monkeypatch.setattr(jobs, "find_config", lambda start=None: None)
    write_log(tmp_path, "running.log")
    js, source = jobs.resolve([], None, str(tmp_path))
    assert [j.name for j in js] == ["running"]
    assert "newest logs" in source, "the header must say where the list came from"


def test_the_provenance_is_reported_so_you_know_which_list_you_are_reading(tmp_path,
                                                                           monkeypatch):
    """Looking at a screen and not knowing which config produced it is a good way to
    watch the wrong machine's idea of the job list."""
    monkeypatch.setattr(jobs, "find_config", lambda start=None: None)
    _, source = jobs.resolve([], None, str(tmp_path))
    assert str(tmp_path) in source
