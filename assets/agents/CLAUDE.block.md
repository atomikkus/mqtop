# mqtop (Claude Code)

Long-running jobs: follow the mqtop-rs block in `AGENTS.md`.

Quick:
- `from mqtop import emit` or `mqtop-rs emit --job ...`
- Log to `*.log` with stdout+stderr; unbuffered.
- Match patterns like `train[.]py`.
- Verify: `mqtop-rs doctor --agents`
