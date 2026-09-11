//! Host metrics: Linux /proc + nvidia-smi; elsewhere → unavailable.

use std::path::Path;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostMetrics {
    pub hostname: String,
    pub cpu_pct: Option<f64>,
    pub mem_pct: Option<f64>,
    pub mem_used_gb: Option<f64>,
    pub gpu_util: Option<f64>,
    pub gpu_mem_used_gb: Option<f64>,
    pub gpu_mem_total_gb: Option<f64>,
    pub disk_free_gb: Option<f64>,
    pub disk_used_pct: Option<f64>,
    pub notes: Vec<String>,
}

#[cfg(target_os = "linux")]
static PREV_CPU: Mutex<Option<(f64, f64)>> = Mutex::new(None);

pub fn hostname() -> String {
    #[cfg(unix)]
    {
        if let Ok(out) = Command::new("uname").arg("-n").output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "local".into())
}

/// `(util%, mem_used_gb, mem_total_gb)` or Nones.
pub fn gpu() -> (Option<f64>, Option<f64>, Option<f64>) {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = out else {
        return (None, None, None);
    };
    if !out.status.success() {
        return (None, None, None);
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.lines().next().unwrap_or("").trim();
    let parts: Vec<_> = line.split(',').map(|s| s.trim()).collect();
    if parts.len() < 3 {
        return (None, None, None);
    }
    let u = parts[0].parse().ok();
    let used: Option<f64> = parts[1].parse().ok().map(|x: f64| x / 1024.0);
    let tot: Option<f64> = parts[2].parse().ok().map(|x: f64| x / 1024.0);
    (u, used, tot)
}

pub fn cpu_pct() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        let line = text.lines().next()?;
        let parts: Vec<f64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|x| x.parse().ok())
            .collect();
        if parts.len() < 5 {
            return None;
        }
        let idle = parts[3] + parts[4];
        let total: f64 = parts.iter().sum();
        let mut prev = PREV_CPU.lock().ok()?;
        let result = if let Some((p_idle, p_total)) = *prev {
            let d_idle = idle - p_idle;
            let d_total = total - p_total;
            if d_total <= 0.0 {
                Some(0.0)
            } else {
                Some((100.0 * (1.0 - d_idle / d_total)).clamp(0.0, 100.0))
            }
        } else {
            None // prime
        };
        *prev = Some((idle, total));
        return result;
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn mem_pct() -> (Option<f64>, Option<f64>) {
    #[cfg(target_os = "linux")]
    {
        let text = match std::fs::read_to_string("/proc/meminfo") {
            Ok(t) => t,
            Err(_) => return (None, None),
        };
        let mut total = None;
        let mut avail = None;
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("");
            let val: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            // kB -> GB
            let gb = val / 1_048_576.0;
            if key == "MemTotal:" {
                total = Some(gb);
            } else if key == "MemAvailable:" {
                avail = Some(gb);
            }
        }
        match (total, avail) {
            (Some(t), Some(a)) if t > 0.0 => {
                let used = t - a;
                (Some(100.0 * (1.0 - a / t)), Some(used))
            }
            _ => (None, None),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        (None, None)
    }
}

pub fn disk(where_: &Path) -> (Option<f64>, Option<f64>) {
    #[cfg(target_os = "linux")]
    {
        let out = match Command::new("df")
            .args(["-k", &where_.display().to_string()])
            .output()
        {
            Ok(o) => o,
            Err(_) => return (None, None),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let Some(line) = text.lines().nth(1) else {
            return (None, None);
        };
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return (None, None);
        }
        let (Ok(total_k), Ok(avail_k)) = (parts[1].parse::<f64>(), parts[3].parse::<f64>()) else {
            return (None, None);
        };
        let free_gb = avail_k * 1024.0 / 1e9;
        let used_pct = if total_k > 0.0 {
            100.0 * (1.0 - avail_k / total_k)
        } else {
            0.0
        };
        return (Some(free_gb), Some(used_pct));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = where_;
        (None, None)
    }
}

pub fn collect_host(watch_dir: &Path) -> HostMetrics {
    let mut notes = Vec::new();
    let (gpu_util, gpu_used, gpu_tot) = gpu();
    if gpu_util.is_none() {
        notes.push("gpu unavailable".into());
    }
    let cpu = cpu_pct();
    if cpu.is_none() {
        #[cfg(not(target_os = "linux"))]
        notes.push("cpu metrics require Linux /proc".into());
    }
    let (mp, mused) = mem_pct();
    let (dfree, dpct) = disk(watch_dir);
    if dfree.is_none() {
        #[cfg(not(target_os = "linux"))]
        notes.push("disk metrics require Linux".into());
    }
    HostMetrics {
        hostname: hostname(),
        cpu_pct: cpu,
        mem_pct: mp,
        mem_used_gb: mused,
        gpu_util,
        gpu_mem_used_gb: gpu_used,
        gpu_mem_total_gb: gpu_tot,
        disk_free_gb: dfree,
        disk_used_pct: dpct,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_missing_is_none() {
        // On machines without nvidia-smi this returns None; with it, Some — either ok.
        let _ = gpu();
    }

    #[test]
    fn hostname_nonempty() {
        assert!(!hostname().is_empty());
    }
}
