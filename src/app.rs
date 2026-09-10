use crate::collectors::sample::Sample;
use crate::pipeline::{build_pipeline_view, ExperimentCompare, PipelineView};
use crate::ui::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    State,
    Age,
    Progress,
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub level: AlertLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
pub struct App {
    pub sample: Sample,
    pub tab: Tab,
    pub selected: usize,
    pub filter: String,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub log_follow: bool,
    pub log_scroll: u16,
    pub log_search: String,
    pub alerts: Vec<Alert>,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub fleet_hosts: Vec<FleetHostRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Filter,
    Search,
}

#[derive(Debug, Clone)]
pub struct FleetHostRow {
    pub name: String,
    pub last_seen_age_s: Option<f64>,
    pub job_count: usize,
    pub status: String,
}

impl App {
    pub fn new(sample: Sample) -> Self {
        let mut app = Self {
            sample,
            tab: Tab::Overview,
            selected: 0,
            filter: String::new(),
            sort: SortKey::Name,
            sort_desc: false,
            log_follow: true,
            log_scroll: 0,
            log_search: String::new(),
            alerts: Vec::new(),
            should_quit: false,
            input_mode: InputMode::Normal,
            fleet_hosts: Vec::new(),
        };
        app.refresh_alerts();
        app
    }

    pub fn apply_sample(&mut self, sample: Sample) {
        self.sample = sample;
        if self.selected >= self.visible_indices().len() && self.selected > 0 {
            self.selected = self.visible_indices().len().saturating_sub(1);
        }
        self.refresh_alerts();
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let mut idxs: Vec<usize> = (0..self.sample.jobs.len())
            .filter(|&i| {
                let j = &self.sample.jobs[i];
                if self.filter.is_empty() {
                    true
                } else {
                    j.job
                        .name
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
                        || j.state.as_str().contains(&self.filter.to_lowercase())
                }
            })
            .collect();
        idxs.sort_by(|&a, &b| {
            let ja = &self.sample.jobs[a];
            let jb = &self.sample.jobs[b];
            let ord = match self.sort {
                SortKey::Name => ja.job.name.cmp(&jb.job.name),
                SortKey::State => ja.state.as_str().cmp(jb.state.as_str()),
                SortKey::Age => ja
                    .since
                    .partial_cmp(&jb.since)
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortKey::Progress => {
                    let pa = ja.records.last().and_then(|r| r.i.as_ref().and_then(|v| v.as_f64()));
                    let pb = jb.records.last().and_then(|r| r.i.as_ref().and_then(|v| v.as_f64()));
                    pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
                }
            };
            if self.sort_desc {
                ord.reverse()
            } else {
                ord
            }
        });
        idxs
    }

    pub fn selected_job_index(&self) -> Option<usize> {
        self.visible_indices().get(self.selected).copied()
    }

    pub fn next_job(&mut self) {
        let n = self.visible_indices().len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected + 1) % n;
    }

    pub fn prev_job(&mut self) {
        let n = self.visible_indices().len();
        if n == 0 {
            return;
        }
        self.selected = if self.selected == 0 { n - 1 } else { self.selected - 1 };
    }

    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
    }

    pub fn prev_tab(&mut self) {
        self.tab = self.tab.prev();
    }

    pub fn refresh_alerts(&mut self) {
        let mut alerts = Vec::new();
        for j in &self.sample.jobs {
            match j.state {
                crate::domain::JobState::Stalled => alerts.push(Alert {
                    level: AlertLevel::Warn,
                    message: format!("{} stalled ({} quiet)", j.job.name, crate::domain::human_secs(j.since)),
                }),
                crate::domain::JobState::NoLog => alerts.push(Alert {
                    level: AlertLevel::Error,
                    message: format!("{}: log missing ({})", j.job.name, j.job.log.display()),
                }),
                _ => {}
            }
            if let Some(h) = &j.malformed_hint {
                alerts.push(Alert {
                    level: AlertLevel::Warn,
                    message: format!("{}: {h}", j.job.name),
                });
            }
        }
        if let Some(pct) = self.sample.host.disk_used_pct {
            if pct >= 95.0 {
                alerts.push(Alert {
                    level: AlertLevel::Error,
                    message: format!("disk critically full ({pct:.0}%)"),
                });
            } else if pct >= 85.0 {
                alerts.push(Alert {
                    level: AlertLevel::Warn,
                    message: format!("disk high ({pct:.0}%)"),
                });
            }
        }
        for n in &self.sample.host.notes {
            alerts.push(Alert {
                level: AlertLevel::Info,
                message: n.clone(),
            });
        }
        self.alerts = alerts;
    }

    pub fn pipeline_view(&self) -> PipelineView {
        build_pipeline_view(&self.sample)
    }

    pub fn experiment_compare(&self) -> ExperimentCompare {
        ExperimentCompare::from_sample(&self.sample)
    }
}
