# mqtop

A terminal monitor for long jobs, read from their logs on the box they run on.

```
mqtop
```

No arguments, no config: it shows the logs in your home directory that were written to
most recently, whether each one's process is still alive, how long each has been quiet,
and the machine's GPU, CPU, memory and disk. Stdlib only, Python 3.11+.

```
mqtop · wsi-a100                                        14:22:51  q to quit
────────────────────────────────────────────────────────────────────────────
 ● chain_distil    running
   [teacher] 213,504/1,000,000 (21%)  122 crops/s  29m  eta 107m
 ◐ build_corpus    stalled? 41m quiet
   [ 611] IN-423-BYL3B3 40x 8 mm2 2000 crops | 500 kept, 1,000,000 crops
 ✓ distil_train    ended 2h14 ago
────────────────────────────────────────────────────────────────────────────
 gpu 97%   ⢀⣀⣤⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
 cpu 34%   ⣀⣀⣀⣀⡀⢀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀
 gpu mem 71.2/80 GB   ram 41 GB
 disk    32 GB free   ███████████████████████████░ 94%
 jobs from newest logs in ~
```

## Why it exists

Every monitor this replaced polled the box over ssh. A long-lived ssh session to a busy
host drops often enough that several of them died mid-run and sat silent while the job
carried on — the worst failure a monitor has, because silence is indistinguishable from
"nothing to report". `mqtop` runs *on* the box and reads local files, so a dropped
connection kills the view, not the watch, and reattaching costs one command.

Its predecessor had four log paths written into its source. When the pipeline grew a new
stage, it went on reporting the old four as ended and showed nothing at all about the job
that was actually running. Discovery by modification time is the fix: you should not have
to write a config before the monitor can tell you what is happening.

## Two sources per job

A job that emits **status records** gets a real progress line:

```python
from mqtop import emit

emit("build_corpus", phase="crop", i=100, n=694, kept=84, eta_s=3600)
```

which prints one line beside the job's ordinary output:

```
##ST {"job":"build_corpus","t":1757500000.0,"phase":"crop","i":100,"n":694,"kept":84}
```

That is the whole contract — the prefix, then one line of JSON. The fields are the job's
own business; nothing validates them. Adding a job to the monitor costs one `emit()` call
and no change to the monitor, which is the point: every earlier version of this watched
jobs by grepping their human-readable logs, and every one of those greps needed rewriting
when the log format changed — once silently, so the monitor sat quiet through a run that
had already finished.

A job that emits nothing still gets its **last log line** and, more usefully, its
**staleness**. A scrolling log cannot tell a job that has stopped from one that is merely
between messages; a monitor that watches the mtime can.

## Configuring it

```bash
mqtop --dir ~/runs                          # discover there instead of in ~
mqtop --job train=~/train.log:train[.]py    # name it explicitly; repeat the flag
mqtop --max-age 6                           # ignore logs untouched for six hours
mqtop --once                                # one frame, then exit
```

Or a `mqtop.toml` in the working directory, or `~/.config/mqtop/jobs.toml`:

```toml
dir = "~/logs"

[[job]]
name  = "teacher"
log   = "~/chain.log"
match = "teacher_over_cache[.]py"

[[job]]
name = "student"
log  = "~/distil.log"
```

`match` is a `pgrep -f` pattern. **Write the brackets.** `pgrep -f teacher.py` matches
this monitor's own command line whenever that string appears in its arguments, and then
every job reads as alive forever — a bug three earlier versions of this check had.
`teacher[.]py` matches the job and not the watcher.

Leave `match` out and it is guessed from the log's name (`train.log` → `train[.]py`). A
guessed pattern is marked as one, and a job whose guess matches nothing reads as *quiet*
rather than *ended* — because "no process matched" only means the job finished if you
knew what to look for.

Precedence: `--job` flags, then a config file, then discovery. The footer says which one
you are looking at, since watching the wrong machine's idea of the job list is otherwise
very easy to do.

## Install

```bash
pip install -e .          # or: pipx install .
mqtop
```

Or copy `mqtop/` onto the box and run `python3 -m mqtop`. It has no dependencies, which
is deliberate: this gets dropped onto machines and run with whatever interpreter is
already there, often inside a conda environment that must not be disturbed. Braille plots
and `/proc` parsing are cheaper than an install that can fail.

## Tests

```bash
python -m pytest -q
```

The terminal drawing is not tested and does not need to be. What is tested is the
judgement underneath it — whether a quiet-but-live job reads as stalled, whether a
guessed pattern is allowed to claim a job ended, whether discovery finds the log that is
moving — plus the record parsing and the staleness arithmetic.

## Licence

MIT.
