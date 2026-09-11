use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
use serde_json::json;

use crate::agents::{self, AgentTarget};
use crate::app::{App, InputMode, SortKey};
use crate::collectors::sample::{sample_once, spawn_sampler};
use crate::config::{resolve, DEFAULT_SHOW};
use crate::domain::job::expand_user;
use crate::domain::status::emit_record;
use crate::ui::{self, Tab};

#[derive(Parser, Debug)]
#[command(
    name = "mqtop-rs",
    version,
    about = "Ratatui job monitor (##ST compatible)"
)]
pub struct Cli {
    /// Job as name=log[:pattern]; repeat. Overrides config.
    #[arg(long = "job", action = clap::ArgAction::Append, global = true)]
    pub jobs: Vec<String>,

    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub dir: Option<String>,

    #[arg(long, default_value_t = DEFAULT_SHOW, global = true)]
    pub show: usize,

    #[arg(long = "max-age", value_name = "HOURS", global = true)]
    pub max_age: Option<f64>,

    #[arg(long, default_value_t = 1.0, global = true)]
    pub interval: f64,

    /// Print one frame / snapshot and exit.
    #[arg(long, global = true)]
    pub once: bool,

    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Emit one ##ST record to stdout.
    Emit {
        #[arg(long)]
        job: String,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long)]
        current: Option<i64>,
        #[arg(long)]
        total: Option<i64>,
        #[arg(long = "metric", value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
        metrics: Vec<String>,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        stage: Option<String>,
        #[arg(long)]
        parent_stage: Option<String>,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        eta_s: Option<f64>,
    },
    /// Write agent instruction files (idempotent).
    Init {
        #[arg(long, default_value = "universal,cursor,claude")]
        agents: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        yes: bool,
    },
    /// Validate environment and agent instruction markers.
    Doctor {
        #[arg(long)]
        agents: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit JSON snapshot of current jobs/host.
    Snapshot {
        #[arg(long)]
        json: bool,
    },
}

pub fn run(mut cli: Cli) -> anyhow::Result<i32> {
    if let Some(cmd) = cli.cmd.take() {
        return run_cmd(cmd, &cli);
    }
    let (jobs, source) = resolve(
        &cli.jobs,
        cli.config.as_deref(),
        cli.dir.as_deref(),
        cli.show,
        cli.max_age.map(|h| h * 3600.0),
    )
    .map_err(|e| anyhow::anyhow!("mqtop-rs: {e}"))?;

    let watch_dir = cli
        .dir
        .as_ref()
        .map(|d| expand_user(PathBuf::from(d)))
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));

    let mut hist = (VecDeque::new(), VecDeque::new(), VecDeque::new());
    let _ = crate::collectors::host::cpu_pct();
    let sample = sample_once(&jobs, &source, &watch_dir, &mut hist);

    if cli.once {
        print_once_frame(&sample);
        return Ok(0);
    }

    let (rx, stop) = spawn_sampler(
        jobs,
        source,
        watch_dir,
        Duration::from_secs_f64(cli.interval.max(0.2)),
    );
    let mut terminal = ratatui::init();
    let mut app = App::new(sample);
    let result = run_ui(&mut terminal, &mut app, &rx);
    drop(stop);
    ratatui::restore();
    result?;
    Ok(0)
}

fn run_cmd(cmd: Command, cli: &Cli) -> anyhow::Result<i32> {
    match cmd {
        Command::Emit {
            job,
            phase,
            current,
            total,
            metrics,
            run_id,
            stage,
            parent_stage,
            state,
            eta_s,
        } => {
            let mut fields = serde_json::Map::new();
            if let Some(p) = phase {
                fields.insert("phase".into(), json!(p));
            }
            if let Some(i) = current {
                fields.insert("i".into(), json!(i));
            }
            if let Some(n) = total {
                fields.insert("n".into(), json!(n));
            }
            if let Some(e) = eta_s {
                fields.insert("eta_s".into(), json!(e));
            }
            if let Some(r) = run_id {
                fields.insert("schema".into(), json!("mqtop/2"));
                fields.insert("run_id".into(), json!(r));
            }
            if let Some(s) = stage {
                fields.insert("stage".into(), json!(s));
            }
            if let Some(p) = parent_stage {
                fields.insert("parent_stage".into(), json!(p));
            }
            if let Some(s) = state {
                fields.insert("state".into(), json!(s));
            }
            let mut metric_obj = serde_json::Map::new();
            for m in metrics {
                if let Some((k, v)) = m.split_once('=') {
                    if let Ok(n) = v.parse::<f64>() {
                        metric_obj.insert(k.to_string(), json!(n));
                    } else {
                        metric_obj.insert(k.to_string(), json!(v));
                    }
                }
            }
            if !metric_obj.is_empty() {
                fields.insert("metrics".into(), serde_json::Value::Object(metric_obj));
            }
            emit_record(&job, fields)?;
            Ok(0)
        }
        Command::Init { agents, path, yes } => {
            let targets = AgentTarget::parse_list(&agents);
            if targets.is_empty() {
                anyhow::bail!("no valid --agents (universal,cursor,claude)");
            }
            for msg in agents::init_agents(&path, &targets, yes)? {
                println!("{msg}");
            }
            Ok(0)
        }
        Command::Doctor {
            agents: check_agents,
            path,
            json,
        } => {
            let report = doctor_report(&path, check_agents, cli)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for line in report["messages"].as_array().unwrap_or(&vec![]) {
                    println!("{}", line.as_str().unwrap_or(""));
                }
            }
            let ok = report["ok"].as_bool().unwrap_or(false);
            Ok(if ok { 0 } else { 1 })
        }
        Command::Snapshot { json: _ } => {
            let (jobs, source) = resolve(
                &cli.jobs,
                cli.config.as_deref(),
                cli.dir.as_deref(),
                cli.show,
                cli.max_age.map(|h| h * 3600.0),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            let watch_dir = cli
                .dir
                .as_ref()
                .map(|d| expand_user(PathBuf::from(d)))
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
            let mut hist = (VecDeque::new(), VecDeque::new(), VecDeque::new());
            let sample = sample_once(&jobs, &source, &watch_dir, &mut hist);
            println!("{}", snapshot_json(&sample));
            Ok(0)
        }
    }
}

fn doctor_report(path: &Path, check_agents: bool, cli: &Cli) -> anyhow::Result<serde_json::Value> {
    let mut messages = Vec::new();
    let mut ok = true;
    messages.push(format!("binary: mqtop-rs {}", env!("CARGO_PKG_VERSION")));
    #[cfg(target_os = "linux")]
    messages.push("platform: linux (collectors enabled)".into());
    #[cfg(not(target_os = "linux"))]
    {
        messages.push("platform: non-linux — cpu/mem/disk/pgrep unavailable".into());
    }
    if std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        messages.push("nvidia-smi: ok".into());
    } else {
        messages.push("nvidia-smi: missing (gpu --)".into());
    }
    match resolve(
        &cli.jobs,
        cli.config.as_deref(),
        cli.dir.as_deref(),
        cli.show,
        None,
    ) {
        Ok((jobs, source)) => {
            messages.push(format!("config: {source} ({} jobs)", jobs.len()));
            for j in &jobs {
                if j.match_is_guess {
                    messages.push(format!(
                        "warn: {} uses guessed match {}",
                        j.name, j.match_pat
                    ));
                }
                if !j.match_pat.contains('[') {
                    messages.push(format!(
                        "warn: {} match {:?} may match monitor argv — prefer brackets",
                        j.name, j.match_pat
                    ));
                    ok = false;
                }
                if let Some(parent) = j.log.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        messages.push(format!("warn: log dir missing for {}", j.log.display()));
                    }
                }
            }
        }
        Err(e) => {
            messages.push(format!("config error: {e}"));
            ok = false;
        }
    }
    if check_agents {
        for m in agents::doctor_agents(path) {
            if m.starts_with("missing") || m.starts_with("no mqtop") {
                ok = false;
            }
            messages.push(m);
        }
    }
    Ok(json!({ "ok": ok, "messages": messages }))
}

fn snapshot_json(sample: &crate::collectors::sample::Sample) -> String {
    let jobs: Vec<_> = sample
        .jobs
        .iter()
        .map(|j| {
            json!({
                "name": j.job.name,
                "log": j.job.log,
                "match": j.job.match_pat,
                "match_is_guess": j.job.match_is_guess,
                "state": j.state.as_str(),
                "since": j.since,
                "shared_log": j.shared_log,
            })
        })
        .collect();
    json!({
        "source": sample.source,
        "hostname": sample.host.hostname,
        "jobs": jobs,
        "host": {
            "cpu_pct": sample.host.cpu_pct,
            "mem_pct": sample.host.mem_pct,
            "gpu_util": sample.host.gpu_util,
            "disk_free_gb": sample.host.disk_free_gb,
            "notes": sample.host.notes,
        }
    })
    .to_string()
}

fn print_once_frame(sample: &crate::collectors::sample::Sample) {
    println!(
        "mqtop-rs · {}                    {}",
        sample.host.hostname,
        chrono::Local::now().format("%H:%M:%S")
    );
    println!("{}", "─".repeat(60));
    if sample.jobs.is_empty() {
        println!(" no logs found in {}", sample.watch_dir.display());
    }
    for j in &sample.jobs {
        println!(" · {:<16} {}", j.job.name, j.state.as_str());
        if let Some(r) = j.records.last() {
            let prog = r
                .progress_pair()
                .map(|(i, n)| format!("{i}/{n}"))
                .unwrap_or_default();
            println!(
                "   {:<8} {prog}",
                r.phase.as_deref().unwrap_or("")
            );
        } else if !j.tail.is_empty() {
            println!("   {}", truncate(&j.tail, 50));
        }
    }
    println!("{}", "─".repeat(60));
    let gpu = sample
        .host
        .gpu_util
        .map(|u| format!("{u:.0}%"))
        .unwrap_or_else(|| "--".into());
    let cpu = sample
        .host
        .cpu_pct
        .map(|u| format!("{u:.0}%"))
        .unwrap_or_else(|| "--".into());
    println!(" gpu {gpu}   cpu {cpu}");
    println!(" jobs from {}", sample.source);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn run_ui(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    rx: &std::sync::mpsc::Receiver<crate::collectors::sample::Sample>,
) -> anyhow::Result<()> {
    loop {
        while let Ok(s) = rx.try_recv() {
            app.apply_sample(s);
        }
        terminal.draw(|f| ui::render(f, app))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(app, key.code);
            }
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode) {
    match app.input_mode {
        InputMode::Filter => match code {
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                app.filter.pop();
            }
            KeyCode::Char(c) => app.filter.push(c),
            _ => {}
        },
        InputMode::Search => match code {
            KeyCode::Esc | KeyCode::Enter => app.input_mode = InputMode::Normal,
            KeyCode::Backspace => {
                app.log_search.pop();
            }
            KeyCode::Char(c) => app.log_search.push(c),
            _ => {}
        },
        InputMode::Normal => match code {
            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => app.next_tab(),
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => app.prev_tab(),
            KeyCode::Char('j') | KeyCode::Down => {
                if app.tab == Tab::Overview || app.tab == Tab::JobDetail {
                    app.next_job();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.tab == Tab::Overview || app.tab == Tab::JobDetail {
                    app.prev_job();
                }
            }
            KeyCode::Char('/') => {
                app.input_mode = InputMode::Filter;
            }
            KeyCode::Char('f') => app.log_follow = !app.log_follow,
            KeyCode::Char('s') => {
                app.sort = match app.sort {
                    SortKey::Name => SortKey::State,
                    SortKey::State => SortKey::Age,
                    SortKey::Age => SortKey::Progress,
                    SortKey::Progress => SortKey::Name,
                };
            }
            KeyCode::Char('S') => app.sort_desc = !app.sort_desc,
            KeyCode::Char('?') => app.tab = Tab::Help,
            _ => {}
        },
    }
}
