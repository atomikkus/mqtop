//! Non-blocking sample of jobs + host into an in-memory snapshot.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::Duration;

use crate::collectors::host::{collect_host, HostMetrics};
use crate::collectors::process::alive;
use crate::domain::job::Job;
use crate::domain::state::{classify, JobState};
use crate::domain::status::{age, last_line, read_records, StatusRecord, TAIL_BYTES};

pub const HIST: usize = 240;

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub job: Job,
    pub state: JobState,
    pub exists: bool,
    pub running: bool,
    pub since: Option<f64>,
    pub shared_log: bool,
    pub records: Vec<StatusRecord>,
    pub tail: String,
    pub malformed_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub jobs: Vec<JobSnapshot>,
    pub host: HostMetrics,
    pub source: String,
    pub watch_dir: PathBuf,
    pub hist_gpu: VecDeque<f64>,
    pub hist_cpu: VecDeque<f64>,
    pub hist_mem: VecDeque<f64>,
}

impl Sample {
    pub fn empty(source: impl Into<String>, watch_dir: PathBuf) -> Self {
        Self {
            jobs: Vec::new(),
            host: HostMetrics {
                hostname: crate::collectors::host::hostname(),
                ..Default::default()
            },
            source: source.into(),
            watch_dir,
            hist_gpu: VecDeque::with_capacity(HIST),
            hist_cpu: VecDeque::with_capacity(HIST),
            hist_mem: VecDeque::with_capacity(HIST),
        }
    }
}

pub fn sample_once(
    jobs: &[Job],
    source: &str,
    watch_dir: &Path,
    hist: &mut (VecDeque<f64>, VecDeque<f64>, VecDeque<f64>),
) -> Sample {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for j in jobs {
        *counts.entry(j.log.to_string_lossy().into_owned()).or_insert(0) += 1;
    }
    let mut snaps = Vec::with_capacity(jobs.len());
    for job in jobs {
        let shared = counts
            .get(&*job.log.to_string_lossy())
            .copied()
            .unwrap_or(0)
            > 1;
        let exists = job.log.exists();
        let running = if exists {
            alive(&job.match_pat)
        } else {
            false
        };
        let since = age(&job.log);
        let state = classify(job, running, since, exists, shared);
        let records = if exists {
            read_records(&job.log, TAIL_BYTES)
        } else {
            Vec::new()
        };
        let malformed_hint = if exists {
            detect_malformed(&job.log)
        } else {
            None
        };
        let tail = if shared && !running {
            String::new()
        } else if records.is_empty() {
            last_line(&job.log, 8192)
        } else {
            String::new()
        };
        snaps.push(JobSnapshot {
            job: job.clone(),
            state,
            exists,
            running,
            since,
            shared_log: shared,
            records,
            tail,
            malformed_hint,
        });
    }
    let host = collect_host(watch_dir);
    if let Some(u) = host.gpu_util {
        push_hist(&mut hist.0, u);
    }
    if let Some(c) = host.cpu_pct {
        push_hist(&mut hist.1, c);
    }
    if let Some(m) = host.mem_pct {
        push_hist(&mut hist.2, m);
    }
    Sample {
        jobs: snaps,
        host,
        source: source.to_string(),
        watch_dir: watch_dir.to_path_buf(),
        hist_gpu: hist.0.clone(),
        hist_cpu: hist.1.clone(),
        hist_mem: hist.2.clone(),
    }
}

fn push_hist(h: &mut VecDeque<f64>, v: f64) {
    if h.len() >= HIST {
        h.pop_front();
    }
    h.push_back(v);
}

fn detect_malformed(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size > 8192 {
        let _ = file.seek(SeekFrom::Start(size - 8192));
    }
    let mut blob = Vec::new();
    file.read_to_end(&mut blob).ok()?;
    let text = String::from_utf8_lossy(&blob);
    for line in text.lines() {
        if line.starts_with(crate::domain::status::PREFIX)
            && crate::domain::status::parse_line(line).is_none()
        {
            return Some("malformed ##ST record in log tail".into());
        }
    }
    None
}

/// Channel of latest samples; send on stop to end the thread.
pub fn spawn_sampler(
    jobs: Vec<Job>,
    source: String,
    watch_dir: PathBuf,
    interval: Duration,
) -> (Receiver<Sample>, SyncSender<()>) {
    let (sample_tx, sample_rx) = mpsc::sync_channel::<Sample>(1);
    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(0);
    thread::spawn(move || {
        let mut hist = (
            VecDeque::with_capacity(HIST),
            VecDeque::with_capacity(HIST),
            VecDeque::with_capacity(HIST),
        );
        let _ = crate::collectors::host::cpu_pct();
        loop {
            let s = sample_once(&jobs, &source, &watch_dir, &mut hist);
            if sample_tx.try_send(s).is_err() {
                // Full or disconnected: try replace by sending after a non-blocking recv isn't
                // available on the sender side — drop frame if UI is behind.
            }
            if stop_rx.recv_timeout(interval).is_ok() {
                break;
            }
        }
    });
    (sample_rx, stop_tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sample_empty_jobs() {
        let dir = tempdir().unwrap();
        let mut hist = (VecDeque::new(), VecDeque::new(), VecDeque::new());
        let s = sample_once(&[], "test", dir.path(), &mut hist);
        assert!(s.jobs.is_empty());
        assert_eq!(s.source, "test");
    }

    #[test]
    fn shared_log_states() {
        let dir = tempdir().unwrap();
        let log = dir.path().join("chain.log");
        std::fs::write(&log, "progress\n").unwrap();
        let jobs = vec![
            Job::new("teacher", log.clone(), "teacher[.]py"),
            Job::new("distil", log.clone(), "distil[.]py"),
        ];
        let mut hist = (VecDeque::new(), VecDeque::new(), VecDeque::new());
        let s = sample_once(&jobs, "test", dir.path(), &mut hist);
        assert_eq!(
            s.jobs
                .iter()
                .filter(|j| j.state == JobState::NotRunning)
                .count(),
            2
        );
    }
}
