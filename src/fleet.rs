//! Fleet client helpers: parse agent snapshots and last-seen age.

use serde::Deserialize;

use crate::app::FleetHostRow;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSnapshot {
    pub schema: Option<String>,
    pub captured_at: f64,
    pub age_s: Option<f64>,
    pub hostname: String,
    pub jobs: Vec<AgentJob>,
    pub host: Option<AgentHost>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentJob {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentHost {
    pub cpu_pct: Option<f64>,
    pub notes: Option<Vec<String>>,
}

pub fn host_row_from_snapshot(snap: &AgentSnapshot, fetch_age_s: f64) -> FleetHostRow {
    let last_seen = snap.age_s.unwrap_or(fetch_age_s);
    let status = if last_seen > 60.0 {
        format!("stale ({:.0}s)", last_seen)
    } else {
        "ok".into()
    };
    FleetHostRow {
        name: snap.hostname.clone(),
        last_seen_age_s: Some(last_seen),
        job_count: snap.jobs.len(),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_when_age_high() {
        let snap = AgentSnapshot {
            schema: Some("mqtop-agent/1".into()),
            captured_at: 0.0,
            age_s: Some(120.0),
            hostname: "box".into(),
            jobs: vec![],
            host: None,
        };
        let row = host_row_from_snapshot(&snap, 120.0);
        assert!(row.status.contains("stale"));
    }
}
