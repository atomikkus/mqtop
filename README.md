# mqtop

A terminal monitor for long jobs, read from their logs on the box they run on.

```bash
mqtop-rs
```

No arguments, no config: it shows the logs in your home directory that were written to
most recently, whether each one's process is still alive, how long each has been quiet,
and the machine's GPU, CPU, memory and disk.

`mqtop-rs` is the Ratatui monitor (v1.0.0). The original Python package remains as a
zero-dependency `emit()` helper and a transitional `mqtop` command.

```
mqtop-rs · wsi-a100                                      14:22:51  q quit
┌ views ──────────────────────────────────────────────────────────────────┐
│ Overview │ Job │ Host │ Alerts │ Pipeline │ Runs │ Fleet │ Diag │ Help │
└─────────────────────────────────────────────────────────────────────────┘
┌ jobs ───────────────────────────────────────────────────────────────────┐
│   name           state     phase    progress           eta    quiet     │
│ > chain_distil   running   teacher  213504/1000000     1h47   3s        │
│   build_corpus   stalled            611/694            1h00   41m       │
│   distil_train   ended     train    1000000/1000000    --     2h14      │
└─────────────────────────────────────────────────────────────────────────┘
 jobs from ~/.config/mqtop/jobs.toml  Tab/h/l views  j/k select  / filter
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

## Install (Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/atomikkus/mqtop/main/install.sh | sh
```

That installs `mqtop-rs` to `~/.local/bin`. Include the fleet agent with:

```bash
curl -fsSL https://raw.githubusercontent.com/atomikkus/mqtop/main/install.sh |
  MQTOP_INSTALL_AGENT=1 sh
```

Pin a version:

```bash
curl -fsSL https://raw.githubusercontent.com/atomikkus/mqtop/main/install.sh |
  MQTOP_VERSION=v1.0.0 sh
```

Release assets: [v1.0.0](https://github.com/atomikkus/mqtop/releases/tag/v1.0.0)
(`x86_64` and `aarch64` musl binaries + SHA-256).

### From source

```bash
cargo build --release --features fleet
cp target/release/mqtop-rs ~/.local/bin/
```

### Python emit helper (optional)

Jobs that call `from mqtop import emit` still need the Python package:

```bash
pip install -e .          # or: pipx install .
```

Or copy `mqtop/` onto the box and run `python3 -m mqtop`. The Python monitor has no
runtime dependencies by design.

## Two sources per job

A job that emits **status records** gets a real progress line:

```python
from mqtop import emit

emit("build_corpus", phase="crop", i=100, n=694, kept=84, eta_s=3600)
```

or from the shell:

```bash
mqtop-rs emit --job build_corpus --phase crop --current 100 --total 694 --eta-s 3600
```

which prints one line beside the job's ordinary output:

```
##ST {"job":"build_corpus","t":1757500000.0,"phase":"crop","i":100,"n":694,"kept":84}
```

That is the whole contract — the prefix, then one line of JSON. The fields are the job's
own business; nothing validates them. Optional v2 fields: `schema`, `run_id`, `stage`,
`parent_stage`, `state`, `metrics`.

A job that emits nothing still gets its **last log line** and, more usefully, its
**staleness**. A scrolling log cannot tell a job that has stopped from one that is merely
between messages; a monitor that watches the mtime can.

## Configuring it

```bash
mqtop-rs --dir ~/runs                          # discover there instead of in ~
mqtop-rs --job train=~/train.log:train[.]py    # name it explicitly; repeat the flag
mqtop-rs --max-age 6                           # ignore logs untouched for six hours
mqtop-rs --once                                # one frame, then exit
mqtop-rs snapshot --json                       # machine-readable snapshot
mqtop-rs doctor --agents                       # check env + agent instruction markers
mqtop-rs init --agents universal,cursor,claude --yes
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

Leave `match` out and it is guessed from the log's name (`train.log` → `train[.]`). A
guessed pattern is marked as one, and a job whose guess matches nothing reads as *quiet*
rather than *ended* — because "no process matched" only means the job finished if you
knew what to look for.

Precedence: `--job` flags, then a config file, then discovery. The footer says which one
you are looking at, since watching the wrong machine's idea of the job list is otherwise
very easy to do.

## Views (mqtop-rs)

| Key | Action |
|-----|--------|
| `Tab` / `h` `l` | switch views |
| `j` `k` | select job |
| `/` | filter |
| `f` | toggle log follow |
| `s` / `S` | cycle sort / reverse |
| `q` | quit |

Views: Overview, Job, Host, Alerts, Pipeline, Runs, Fleet, Diagnostics, Help.

Read-only: no pause, kill, or relaunch.

## Agent setup

```bash
mqtop-rs init --agents universal,cursor,claude --yes
mqtop-rs doctor --agents
```

Writes marker-delimited blocks into `AGENTS.md`, `.cursor/rules/mqtop.mdc`, and
`CLAUDE.md` without overwriting unrelated guidance.

## Tests

```bash
cargo test --locked --features fleet
python -m pytest -q
```

What is tested is the judgement underneath the drawing — whether a quiet-but-live job
reads as stalled, whether a guessed pattern is allowed to claim a job ended, whether
discovery finds the log that is moving — plus the record parsing and the staleness
arithmetic.

## Licence

MIT.
