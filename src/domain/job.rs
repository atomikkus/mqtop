//! Job definitions and process-pattern guessing.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub name: String,
    pub log: PathBuf,
    pub match_pat: String,
    /// Guessed from filename — "no process" must not mean "ended".
    pub match_is_guess: bool,
}

impl Job {
    pub fn new(name: impl Into<String>, log: PathBuf, match_pat: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            log,
            match_pat: match_pat.into(),
            match_is_guess: false,
        }
    }

    pub fn guessed(name: impl Into<String>, log: PathBuf) -> Self {
        let pat = default_pattern(&log);
        Self {
            name: name.into(),
            log,
            match_pat: pat,
            match_is_guess: true,
        }
    }
}

/// `build_corpus.log` → `build_corpus[.]` (not `[.]py`).
pub fn default_pattern(log: &Path) -> String {
    let stem = log
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("job");
    let mut out = String::new();
    for ch in stem.chars() {
        match ch {
            '.' => out.push_str("[.]"),
            '\\' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push_str("[.]");
    out
}

/// `name=log[:pattern]`. Colon after last path separator only (Windows drives).
pub fn parse_job_arg(arg: &str) -> Result<Job, String> {
    let (name, rest) = arg
        .split_once('=')
        .ok_or_else(|| format!("--job wants name=log[:pattern], got {arg:?}"))?;
    if name.trim().is_empty() || rest.trim().is_empty() {
        return Err(format!("--job wants name=log[:pattern], got {arg:?}"));
    }
    let last_sep = rest.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
    let tail = &rest[last_sep..];
    let (log, pattern) = if let Some(idx) = tail.find(':') {
        let cut = last_sep + idx;
        (&rest[..cut], &rest[cut + 1..])
    } else {
        (rest, "")
    };
    let log = expand_user(PathBuf::from(log.trim()));
    if pattern.trim().is_empty() {
        Ok(Job::guessed(name.trim(), log))
    } else {
        Ok(Job::new(name.trim(), log, pattern.trim()))
    }
}

pub fn expand_user(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") || s == "~" {
        if let Some(home) = dirs::home_dir() {
            if s == "~" {
                return home;
            }
            return home.join(&s[2..]);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_flag_carries_name_log_pattern() {
        let j = parse_job_arg("train=/var/log/train.log:train[.]py").unwrap();
        assert_eq!(j.name, "train");
        assert_eq!(j.match_pat, "train[.]py");
        assert!(!j.match_is_guess);
        assert_eq!(j.log.file_name().unwrap(), "train.log");
    }

    #[test]
    fn pattern_guessed_from_log_name() {
        let j = parse_job_arg("build=/tmp/build_corpus.log").unwrap();
        assert_eq!(j.match_pat, "build_corpus[.]");
        assert!(j.match_is_guess);
    }

    #[test]
    fn guess_does_not_assume_python() {
        assert_eq!(
            default_pattern(Path::new("/tmp/chain_distil.log")),
            "chain_distil[.]"
        );
    }

    #[test]
    fn windows_drive_not_pattern() {
        let j = parse_job_arg(r"train=C:\logs\train.log").unwrap();
        assert!(j.log.to_string_lossy().ends_with("train.log"));
        assert!(j.log.to_string_lossy().contains("logs"));
    }

    #[test]
    fn pattern_after_windows_path() {
        let j = parse_job_arg(r"train=C:\logs\train.log:python.*train").unwrap();
        assert_eq!(j.match_pat, "python.*train");
    }

    #[test]
    fn malformed_refused() {
        for bad in ["", "noequals", "=/tmp/a.log", "name="] {
            assert!(parse_job_arg(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn guessed_pattern_has_brackets() {
        assert!(default_pattern(Path::new("/tmp/build_corpus.log")).contains("[.]"));
    }
}
