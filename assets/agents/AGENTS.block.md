# mqtop agent rules

Long-running jobs on a box should be observable with `mqtop` / `mqtop-rs` without SSH polling.

## Status protocol
Print one flushed line beside normal output:
```
##ST {"job":"NAME","t":UNIX,"phase":"PHASE","i":N,"n":TOTAL,"eta_s":SECONDS}
```
Optional v2 fields: `schema`, `run_id`, `stage`, `parent_stage`, `state`, `metrics`.

Shell without Python:
`mqtop-rs emit --job NAME --phase PHASE --current N --total TOTAL`

## Launch checklist
1. Redirect stdout **and** stderr to a durable `*.log` on the box.
2. Prefer unbuffered output (`python -u`, `stdbuf -oL`, or `emit` which flushes).
3. Prefer an explicit process match with brackets: `train[.]py` not `train.py`.
4. Run `mqtop-rs doctor --agents` (or `mqtop --once`) before claiming the job is monitored.
5. Preserve the job's exit code; never treat "output stopped" as success.
6. Run the monitor **on the same machine** as the job.

## Anti-patterns
- Grepping human log lines for progress (##ST replaced that).
- Bare `pgrep` patterns that match the monitor argv.
- Pretty-printed / multi-line JSON after `##ST `.
- Inferring completion from a guessed match with no process.
