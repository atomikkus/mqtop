//! Pipeline / experiment views from additive v2 ##ST fields.

use std::collections::{BTreeMap, BTreeSet};

use crate::collectors::sample::Sample;
use crate::domain::JobState;

#[derive(Debug, Clone)]
pub struct StageNode {
    pub name: String,
    pub parent: Option<String>,
    pub job_state: JobState,
    pub state_label: String,
    pub critical: bool,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineView {
    pub stages: Vec<StageNode>,
    pub run_ids: Vec<String>,
}

pub fn build_pipeline_view(sample: &Sample) -> PipelineView {
    let mut stages = Vec::new();
    let mut run_ids = BTreeSet::new();
    for j in &sample.jobs {
        let rec = j.records.last();
        let stage_name = rec
            .and_then(|r| r.stage.clone())
            .unwrap_or_else(|| j.job.name.clone());
        let parent = rec.and_then(|r| r.parent_stage.clone());
        let run_id = rec.and_then(|r| r.run_id.clone());
        if let Some(r) = &run_id {
            run_ids.insert(r.clone());
        }
        let label = rec
            .and_then(|r| r.state.clone())
            .unwrap_or_else(|| j.state.as_str().to_string());
        let critical = matches!(j.state, JobState::Stalled | JobState::NoLog);
        stages.push(StageNode {
            name: stage_name,
            parent,
            job_state: j.state,
            state_label: label,
            critical,
            run_id,
        });
    }
    PipelineView {
        stages,
        run_ids: run_ids.into_iter().collect(),
    }
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: String,
    pub stages: Vec<String>,
    pub last_i: Option<f64>,
    pub metrics: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExperimentCompare {
    pub runs: Vec<RunSummary>,
}

impl ExperimentCompare {
    pub fn from_sample(sample: &Sample) -> Self {
        let mut by_run: BTreeMap<String, RunSummary> = BTreeMap::new();
        for j in &sample.jobs {
            for r in &j.records {
                let Some(run_id) = r.run_id.clone() else {
                    continue;
                };
                let entry = by_run.entry(run_id.clone()).or_insert_with(|| RunSummary {
                    run_id: run_id.clone(),
                    stages: Vec::new(),
                    last_i: None,
                    metrics: BTreeMap::new(),
                });
                if let Some(stage) = &r.stage {
                    if !entry.stages.contains(stage) {
                        entry.stages.push(stage.clone());
                    }
                }
                if let Some(i) = r.i.as_ref().and_then(|v| v.as_f64()) {
                    entry.last_i = Some(i);
                }
                if let Some(serde_json::Value::Object(m)) = r.extra.get("metrics") {
                    for (k, v) in m {
                        entry.metrics.insert(k.clone(), v.to_string());
                    }
                }
                for (k, v) in &r.extra {
                    if k == "metrics" {
                        continue;
                    }
                    if matches!(v, serde_json::Value::Number(_)) {
                        entry.metrics.insert(k.clone(), v.to_string());
                    }
                }
            }
        }
        Self {
            runs: by_run.into_values().collect(),
        }
    }
}

/// Optional bounded history trait; SQLite behind feature flag.
pub trait HistoryStore {
    fn append_sample(&mut self, sample: &Sample) -> anyhow::Result<()>;
}

pub struct NullHistory;

impl HistoryStore for NullHistory {
    fn append_sample(&mut self, _sample: &Sample) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "sqlite-history")]
pub mod sqlite {
    use super::*;
    use rusqlite::Connection;

    pub struct SqliteHistory {
        conn: Connection,
        max_rows: i64,
    }

    impl SqliteHistory {
        pub fn open(path: &std::path::Path, max_rows: i64) -> anyhow::Result<Self> {
            let conn = Connection::open(path)?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS samples (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    t REAL NOT NULL,
                    payload TEXT NOT NULL
                );",
            )?;
            Ok(Self { conn, max_rows })
        }
    }

    impl HistoryStore for SqliteHistory {
        fn append_sample(&mut self, sample: &Sample) -> anyhow::Result<()> {
            let t = crate::domain::status::now_unix();
            let payload = serde_json::json!({
                "source": sample.source,
                "hostname": sample.host.hostname,
                "jobs": sample.jobs.iter().map(|j| {
                    serde_json::json!({
                        "name": j.job.name,
                        "state": j.state.as_str(),
                        "log": j.job.log,
                    })
                }).collect::<Vec<_>>(),
            });
            self.conn.execute(
                "INSERT INTO samples (t, payload) VALUES (?1, ?2)",
                rusqlite::params![t, payload.to_string()],
            )?;
            self.conn.execute(
                "DELETE FROM samples WHERE id NOT IN (
                    SELECT id FROM samples ORDER BY id DESC LIMIT ?1
                )",
                rusqlite::params![self.max_rows],
            )?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::sample::{JobSnapshot, Sample};
    use crate::domain::job::Job;
    use crate::domain::status::{parse_line, PREFIX};
    use std::collections::VecDeque;
    use std::path::PathBuf;

    #[test]
    fn pipeline_from_v2_records() {
        let line = format!(
            "{PREFIX}{}",
            r#"{"job":"j","run_id":"r1","stage":"train","parent_stage":"prep","state":"running","i":1}"#
        );
        let rec = parse_line(&line).unwrap();
        let sample = Sample {
            jobs: vec![JobSnapshot {
                job: Job::new("j", PathBuf::from("j.log"), "j[.]py"),
                state: JobState::Running,
                exists: true,
                running: true,
                since: Some(1.0),
                shared_log: false,
                records: vec![rec],
                tail: String::new(),
                malformed_hint: None,
            }],
            host: Default::default(),
            source: "test".into(),
            watch_dir: PathBuf::from("."),
            hist_gpu: VecDeque::new(),
            hist_cpu: VecDeque::new(),
            hist_mem: VecDeque::new(),
        };
        let v = build_pipeline_view(&sample);
        assert_eq!(v.stages[0].name, "train");
        assert_eq!(v.stages[0].parent.as_deref(), Some("prep"));
        assert_eq!(v.run_ids, vec!["r1".to_string()]);
        let cmp = ExperimentCompare::from_sample(&sample);
        assert_eq!(cmp.runs.len(), 1);
    }
}
