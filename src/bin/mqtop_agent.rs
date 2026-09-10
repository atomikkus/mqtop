//! Read-only durable host agent for fleet milestone.
//! Serves authenticated JSON snapshots; no mutation endpoints.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use hmac::{Hmac, Mac};
use mqtop_rs::collectors::sample::{sample_once, Sample};
use mqtop_rs::config::resolve;
use mqtop_rs::domain::job::expand_user;
use sha2::Sha256;
use tiny_http::{Header, Method, Response, Server, StatusCode};

type HmacSha256 = Hmac<Sha256>;

#[derive(Parser, Debug)]
#[command(name = "mqtop-agent", about = "Read-only mqtop host agent")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:7420")]
    bind: String,
    #[arg(long, env = "MQTOP_AGENT_TOKEN")]
    token: String,
    #[arg(long)]
    dir: Option<String>,
    #[arg(long, default_value_t = 2.0)]
    interval: f64,
    #[arg(long, default_value_t = 6)]
    show: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.token.is_empty() {
        anyhow::bail!("--token / MQTOP_AGENT_TOKEN required");
    }
    let watch_dir = args
        .dir
        .as_ref()
        .map(|d| expand_user(PathBuf::from(d)))
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));

    let (jobs, source) = resolve(&[], None, args.dir.as_deref(), args.show, None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let latest: Arc<Mutex<Option<(f64, Sample)>>> = Arc::new(Mutex::new(None));
    let latest_bg = latest.clone();
    let watch_bg = watch_dir.clone();
    let source_bg = source.clone();
    let jobs_bg = jobs.clone();
    let interval = Duration::from_secs_f64(args.interval.max(0.5));
    std::thread::spawn(move || {
        let mut hist = (VecDeque::new(), VecDeque::new(), VecDeque::new());
        loop {
            let s = sample_once(&jobs_bg, &source_bg, &watch_bg, &mut hist);
            let t = mqtop_rs::domain::status::now_unix();
            if let Ok(mut g) = latest_bg.lock() {
                *g = Some((t, s));
            }
            std::thread::sleep(interval);
        }
    });

    let server = Server::http(&args.bind).map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("mqtop-agent listening on http://{} (read-only)", args.bind);

    for request in server.incoming_requests() {
        let auth_ok = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| {
                let v = h.value.as_str();
                v.strip_prefix("Bearer ")
                    .map(|t| constant_eq(t, &args.token))
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if !auth_ok {
            let _ = request.respond(Response::from_string("unauthorized").with_status_code(401));
            continue;
        }

        // Reject non-GET — no mutation surface.
        if *request.method() != Method::Get {
            let _ = request.respond(
                Response::from_string("method not allowed").with_status_code(StatusCode(405)),
            );
            continue;
        }

        match request.url() {
            "/health" => {
                let body = serde_json::json!({"ok": true, "role": "mqtop-agent", "read_only": true});
                let _ = request.respond(
                    Response::from_string(body.to_string())
                        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
                );
            }
            "/v1/snapshot" => {
                let guard = latest.lock().unwrap();
                let Some((t, sample)) = guard.as_ref() else {
                    let _ = request.respond(Response::from_string("warming up").with_status_code(503));
                    continue;
                };
                let age = mqtop_rs::domain::status::now_unix() - t;
                let jobs: Vec<_> = sample
                    .jobs
                    .iter()
                    .map(|j| {
                        serde_json::json!({
                            "name": j.job.name,
                            "state": j.state.as_str(),
                            "log": j.job.log,
                            "since": j.since,
                        })
                    })
                    .collect();
                let body = serde_json::json!({
                    "schema": "mqtop-agent/1",
                    "captured_at": t,
                    "age_s": age,
                    "hostname": sample.host.hostname,
                    "source": sample.source,
                    "jobs": jobs,
                    "host": {
                        "cpu_pct": sample.host.cpu_pct,
                        "mem_pct": sample.host.mem_pct,
                        "gpu_util": sample.host.gpu_util,
                        "notes": sample.host.notes,
                    },
                    "sig": sign(&args.token, *t, &sample.host.hostname),
                });
                let _ = request.respond(
                    Response::from_string(body.to_string())
                        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
                );
            }
            _ => {
                let _ = request.respond(Response::from_string("not found").with_status_code(404));
            }
        }
    }
    Ok(())
}

fn sign(token: &str, t: f64, host: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(token.as_bytes()).expect("hmac");
    mac.update(format!("{t}:{host}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
