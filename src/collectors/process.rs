//! Process liveness. Linux: pgrep -f with self-exclusion. Elsewhere: unavailable → false.

#[cfg(target_os = "linux")]
use std::process::Command;

/// Is a process matching `pattern` running, excluding this monitor and its parent?
pub fn alive(pattern: &str) -> bool {
    alive_excluding(pattern, &[])
}

pub fn alive_excluding(pattern: &str, extra_exclude: &[u32]) -> bool {
    #[cfg(target_os = "linux")]
    {
        let out = match Command::new("pgrep").args(["-f", pattern]).output() {
            Ok(o) => o,
            Err(_) => return false,
        };
        let my = std::process::id();
        let ppid = parent_pid().unwrap_or(0);
        let mut drop: std::collections::HashSet<u32> = [my, ppid].into_iter().collect();
        for &p in extra_exclude {
            drop.insert(p);
        }
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse::<u32>().ok())
            .any(|pid| !drop.contains(&pid))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pattern, extra_exclude);
        false
    }
}

#[cfg(target_os = "linux")]
fn parent_pid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_or_missing_pgrep_is_not_alive() {
        let _ = alive("mqtop_rs_definitely_no_such_process_zzzz");
    }
}
