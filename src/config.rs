//! Job list resolution: --job > config > discovery.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::domain::job::{default_pattern, expand_user, parse_job_arg, Job};

pub const DEFAULT_SHOW: usize = 6;
pub const DEFAULT_DIR: &str = "~";

const CONFIG_NAMES: &[&str] = &["mqtop.toml", ".mqtop.toml"];

#[derive(Debug, Deserialize, Default)]
struct TomlRoot {
    dir: Option<String>,
    #[serde(default)]
    job: Vec<TomlJob>,
}

#[derive(Debug, Deserialize)]
struct TomlJob {
    name: Option<String>,
    log: Option<String>,
    #[serde(rename = "match")]
    match_field: Option<String>,
}

impl TomlJob {
    fn match_pat(&self) -> Option<&str> {
        self.match_field
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

pub fn find_config(start: Option<&Path>) -> Option<PathBuf> {
    let here = start
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    for name in CONFIG_NAMES {
        let p = here.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    let cfg = dirs::home_dir()?.join(".config").join("mqtop").join("jobs.toml");
    if cfg.exists() {
        Some(cfg)
    } else {
        None
    }
}

pub fn load_config(path: &Path) -> Result<(Vec<Job>, Option<String>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path:?}: {e}"))?;
    let data: TomlRoot = toml::from_str(&text).map_err(|e| format!("{path:?}: {e}"))?;
    let mut jobs = Vec::new();
    for (i, entry) in data.job.iter().enumerate() {
        let (Some(name), Some(log_s)) = (&entry.name, &entry.log) else {
            return Err(format!(
                "{}: job {i} needs both a name and a log",
                path.display()
            ));
        };
        if name.is_empty() || log_s.is_empty() {
            return Err(format!(
                "{}: job {i} needs both a name and a log",
                path.display()
            ));
        }
        let log = expand_user(PathBuf::from(log_s));
        if let Some(pat) = entry.match_pat() {
            jobs.push(Job::new(name, log, pat));
        } else {
            jobs.push(Job::guessed(name, log));
        }
    }
    Ok((jobs, data.dir))
}

pub fn discover(directory: &Path, limit: usize, max_age_s: Option<f64>) -> Vec<Job> {
    let d = expand_user(directory.to_path_buf());
    let Ok(entries) = fs::read_dir(&d) else {
        return Vec::new();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut logs: Vec<(PathBuf, f64)> = Vec::new();
    for ent in entries.flatten() {
        let p = ent.path();
        if !p.is_file() {
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let mtime = ent
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if let Some(max) = max_age_s {
            if now - mtime > max {
                continue;
            }
        }
        logs.push((p, mtime));
    }
    logs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    logs.into_iter()
        .take(limit)
        .map(|(p, _)| {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("job")
                .to_string();
            let _ = default_pattern(&p);
            Job::guessed(name, p)
        })
        .collect()
}

/// `(jobs, provenance)`.
pub fn resolve(
    job_args: &[String],
    config: Option<&Path>,
    directory: Option<&str>,
    limit: usize,
    max_age_s: Option<f64>,
) -> Result<(Vec<Job>, String), String> {
    if !job_args.is_empty() {
        let mut jobs = Vec::new();
        for a in job_args {
            jobs.push(parse_job_arg(a)?);
        }
        return Ok((jobs, "--job".into()));
    }

    let cfg = if let Some(c) = config {
        Some(expand_user(c.to_path_buf()))
    } else {
        find_config(None)
    };

    if let Some(cfg) = cfg {
        let (jobs, settings_dir) = load_config(&cfg)?;
        let d = directory
            .map(str::to_string)
            .or(settings_dir)
            .unwrap_or_else(|| DEFAULT_DIR.into());
        if jobs.is_empty() {
            return Ok((
                discover(Path::new(&d), limit, max_age_s),
                format!("{} (empty, discovering)", cfg.display()),
            ));
        }
        return Ok((jobs, cfg.display().to_string()));
    }

    let d = directory.unwrap_or(DEFAULT_DIR);
    Ok((
        discover(Path::new(d), limit, max_age_s),
        format!("newest logs in {d}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_log(d: &Path, name: &str) {
        let p = d.join(name);
        let mut f = fs::File::create(&p).unwrap();
        writeln!(f, "hello").unwrap();
    }

    #[test]
    fn discovery_newest_first() {
        let dir = tempdir().unwrap();
        write_log(dir.path(), "ancient.log");
        // ensure ordering: touch fresh last
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_log(dir.path(), "fresh.log");
        let found = discover(dir.path(), 2, None);
        assert_eq!(found[0].name, "fresh");
        assert!(found.iter().all(|j| j.match_is_guess));
    }

    #[test]
    fn only_log_files() {
        let dir = tempdir().unwrap();
        write_log(dir.path(), "run.log");
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        assert_eq!(discover(dir.path(), 5, None).len(), 1);
    }

    #[test]
    fn config_supplies_jobs() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mqtop.toml");
        fs::write(
            &cfg,
            r#"
dir = "/srv/logs"
[[job]]
name = "teacher"
log = "/srv/logs/t.log"
match = "teacher[.]py"
[[job]]
name = "student"
log = "/srv/logs/s.log"
"#,
        )
        .unwrap();
        let (js, settings) = load_config(&cfg).unwrap();
        assert_eq!(js[0].name, "teacher");
        assert!(!js[0].match_is_guess);
        assert!(js[1].match_is_guess);
        assert_eq!(settings.as_deref(), Some("/srv/logs"));
    }

    #[test]
    fn job_flag_beats_config() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("mqtop.toml"),
            r#"[[job]]
name = "from_config"
log = "/tmp/c.log"
"#,
        )
        .unwrap();
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let (js, source) = resolve(&["cli=/tmp/x.log".into()], None, None, 6, None).unwrap();
        std::env::set_current_dir(old).unwrap();
        assert_eq!(js[0].name, "cli");
        assert_eq!(source, "--job");
    }

    #[test]
    fn missing_log_refused() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mqtop.toml");
        fs::write(&cfg, "[[job]]\nname = \"a\"\n").unwrap();
        assert!(load_config(&cfg).is_err());
    }
}
