//! Evidence-based job state classification (ported from Python `job_state`).

use crate::domain::job::Job;

/// Quiet this long while still running → stalled.
pub const STALE_S: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    NoLog,
    Stalled,
    Running,
    Quiet,
    NotRunning,
    Ended,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoLog => "no log",
            Self::Stalled => "stalled",
            Self::Running => "running",
            Self::Quiet => "quiet",
            Self::NotRunning => "not running",
            Self::Ended => "ended",
        }
    }
}

pub fn classify(
    job: &Job,
    running: bool,
    since: Option<f64>,
    exists: bool,
    shared_log: bool,
) -> JobState {
    if !exists {
        return JobState::NoLog;
    }
    if running && since.is_some_and(|s| s > STALE_S) {
        return JobState::Stalled;
    }
    if running {
        return JobState::Running;
    }
    if job.match_is_guess {
        return JobState::Quiet;
    }
    if shared_log {
        return JobState::NotRunning;
    }
    JobState::Ended
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn job(guess: bool) -> Job {
        Job {
            name: "j".into(),
            log: PathBuf::from("j.log"),
            match_pat: "j[.]py".into(),
            match_is_guess: guess,
        }
    }

    #[test]
    fn live_writing_is_running() {
        assert_eq!(
            classify(&job(false), true, Some(5.0), true, false),
            JobState::Running
        );
    }

    #[test]
    fn live_quiet_is_stalled() {
        assert_eq!(
            classify(&job(false), true, Some(STALE_S + 1.0), true, false),
            JobState::Stalled
        );
    }

    #[test]
    fn known_pattern_ended() {
        assert_eq!(
            classify(&job(false), false, Some(10.0), true, false),
            JobState::Ended
        );
    }

    #[test]
    fn guessed_pattern_quiet() {
        assert_eq!(
            classify(&job(true), false, Some(10.0), true, false),
            JobState::Quiet
        );
    }

    #[test]
    fn missing_log() {
        assert_eq!(
            classify(&job(false), false, None, false, false),
            JobState::NoLog
        );
    }

    #[test]
    fn shared_log_not_ended() {
        assert_eq!(
            classify(&job(false), false, Some(15.0), true, true),
            JobState::NotRunning
        );
    }
}
