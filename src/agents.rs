//! Idempotent agent instruction generation.

use std::fs;
use std::path::{Path, PathBuf};

pub const BEGIN: &str = "<!-- mqtop-rs:begin -->";
pub const END: &str = "<!-- mqtop-rs:end -->";

pub const UNIVERSAL_BODY: &str = r#"# mqtop agent rules

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
"#;

pub const CURSOR_BODY: &str = r#"---
description: mqtop long-job monitoring for Cursor agents
globs:
  - "**/*train*.py"
  - "**/*pipeline*"
  - "**/*.sh"
alwaysApply: false
---

Follow the mqtop agent rules (AGENTS.md mqtop-rs block).

When starting a long job:
1. Ensure log path ends in `.log` and both stdout/stderr are redirected.
2. Emit `##ST` records (or `mqtop-rs emit`) with `phase`, `i`, `n`, `eta_s`.
3. Use bracketed match patterns in mqtop.toml / `--job`.
4. Verify with `mqtop-rs --once` or `mqtop-rs doctor --agents`.
5. Do not poll over SSH; attach on the box.
"#;

pub const CLAUDE_BODY: &str = r#"# mqtop (Claude Code)

Long-running jobs: follow the mqtop-rs block in `AGENTS.md`.

Quick:
- `from mqtop import emit` or `mqtop-rs emit --job ...`
- Log to `*.log` with stdout+stderr; unbuffered.
- Match patterns like `train[.]py`.
- Verify: `mqtop-rs doctor --agents`
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTarget {
    Universal,
    Cursor,
    Claude,
}

impl AgentTarget {
    pub fn parse_list(s: &str) -> Vec<Self> {
        s.split(',')
            .filter_map(|p| match p.trim().to_lowercase().as_str() {
                "universal" | "agents" => Some(Self::Universal),
                "cursor" => Some(Self::Cursor),
                "claude" => Some(Self::Claude),
                _ => None,
            })
            .collect()
    }

    pub fn rel_path(self) -> &'static str {
        match self {
            Self::Universal => "AGENTS.md",
            Self::Cursor => ".cursor/rules/mqtop.mdc",
            Self::Claude => "CLAUDE.md",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Self::Universal => UNIVERSAL_BODY,
            Self::Cursor => CURSOR_BODY,
            Self::Claude => CLAUDE_BODY,
        }
    }
}

pub fn wrap_block(body: &str) -> String {
    format!("{BEGIN}\n{body}\n{END}\n")
}

/// Merge marker-delimited block into file. Returns (path, action).
pub fn merge_file(path: &Path, body: &str, yes: bool) -> anyhow::Result<String> {
    let block = wrap_block(body);
    if path.exists() {
        let existing = fs::read_to_string(path)?;
        if existing.contains(BEGIN) && existing.contains(END) {
            let updated = replace_block(&existing, &block)?;
            if updated == existing {
                return Ok(format!("unchanged {}", path.display()));
            }
            if !yes {
                return Ok(format!(
                    "would update {} (pass --yes to write)",
                    path.display()
                ));
            }
            backup(path)?;
            fs::write(path, updated)?;
            return Ok(format!("updated {}", path.display()));
        }
        // Append block; do not overwrite unrelated guidance.
        if !yes {
            return Ok(format!(
                "would append mqtop block to {} (pass --yes)",
                path.display()
            ));
        }
        backup(path)?;
        let mut new = existing;
        if !new.ends_with('\n') {
            new.push('\n');
        }
        new.push('\n');
        new.push_str(&block);
        fs::write(path, new)?;
        return Ok(format!("appended {}", path.display()));
    }
    if !yes {
        return Ok(format!(
            "would create {} (pass --yes to write)",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, block)?;
    Ok(format!("created {}", path.display()))
}

fn replace_block(existing: &str, block: &str) -> anyhow::Result<String> {
    let start = existing
        .find(BEGIN)
        .ok_or_else(|| anyhow::anyhow!("missing begin marker"))?;
    let end = existing[start..]
        .find(END)
        .ok_or_else(|| anyhow::anyhow!("missing end marker"))?
        + start
        + END.len();
    // Include trailing newline after END if present
    let mut end = end;
    if existing[end..].starts_with('\n') {
        end += 1;
    }
    let mut out = String::new();
    out.push_str(&existing[..start]);
    out.push_str(block);
    out.push_str(&existing[end..]);
    Ok(out)
}

fn backup(path: &Path) -> anyhow::Result<()> {
    let bak = PathBuf::from(format!("{}.mqtop.bak", path.display()));
    fs::copy(path, bak)?;
    Ok(())
}

pub fn init_agents(repo: &Path, targets: &[AgentTarget], yes: bool) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for t in targets {
        let path = repo.join(t.rel_path());
        out.push(merge_file(&path, t.body(), yes)?);
    }
    Ok(out)
}

pub fn doctor_agents(repo: &Path) -> Vec<String> {
    let mut msgs = Vec::new();
    for t in [AgentTarget::Universal, AgentTarget::Cursor, AgentTarget::Claude] {
        let path = repo.join(t.rel_path());
        if !path.exists() {
            msgs.push(format!("missing {}", path.display()));
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            msgs.push(format!("unreadable {}", path.display()));
            continue;
        };
        if text.contains(BEGIN) && text.contains(END) {
            msgs.push(format!("ok {}", path.display()));
        } else {
            msgs.push(format!("no mqtop markers in {}", path.display()));
        }
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn idempotent_merge() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("AGENTS.md");
        let r1 = merge_file(&p, UNIVERSAL_BODY, true).unwrap();
        assert!(r1.starts_with("created"));
        let r2 = merge_file(&p, UNIVERSAL_BODY, true).unwrap();
        assert!(r2.starts_with("unchanged"));
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text.matches(BEGIN).count(), 1);
    }

    #[test]
    fn bodies_match_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let universal = fs::read_to_string(root.join("assets/agents/AGENTS.block.md")).unwrap();
        assert!(universal.contains("##ST"));
        assert_eq!(UNIVERSAL_BODY.trim(), universal.trim());
        let cursor = fs::read_to_string(root.join("assets/agents/cursor.mqtop.mdc")).unwrap();
        assert_eq!(CURSOR_BODY.trim(), cursor.trim());
        let claude = fs::read_to_string(root.join("assets/agents/CLAUDE.block.md")).unwrap();
        assert_eq!(CLAUDE_BODY.trim(), claude.trim());
    }

    #[test]
    fn preserves_unrelated() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("AGENTS.md");
        fs::write(&p, "# other\n\nkeep me\n").unwrap();
        merge_file(&p, "hello", true).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("keep me"));
        assert!(text.contains(BEGIN));
    }
}
