//! One-line status records: `##ST {json}`.
//!
//! Fields are the job's business; nothing validates them. Fixed contract is the
//! prefix and that the rest is one line of JSON.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

pub const PREFIX: &str = "##ST ";
pub const TAIL_BYTES: u64 = 262_144;

/// Parsed status record. Extra fields live in `extra`.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRecord {
    pub job: Option<String>,
    pub t: Option<f64>,
    pub phase: Option<String>,
    pub i: Option<Value>,
    pub n: Option<Value>,
    pub eta_s: Option<f64>,
    /// Additive v2 fields (optional; v1 records omit them).
    pub schema: Option<String>,
    pub run_id: Option<String>,
    pub stage: Option<String>,
    pub parent_stage: Option<String>,
    pub state: Option<String>,
    pub extra: serde_json::Map<String, Value>,
}

impl StatusRecord {
    pub fn from_value(v: Value) -> Option<Self> {
        let Value::Object(mut map) = v else {
            return None;
        };
        let job = map.remove("job").map(|x| match x {
            Value::String(s) => s,
            other => other.to_string(),
        });
        let t = map.remove("t").and_then(as_f64);
        let phase = map.remove("phase").map(|x| match x {
            Value::String(s) => s,
            other => other.to_string(),
        });
        let i = map.remove("i");
        let n = map.remove("n");
        let eta_s = map.remove("eta_s").and_then(as_f64);
        let schema = map.remove("schema").and_then(as_string);
        let run_id = map.remove("run_id").and_then(as_string);
        let stage = map.remove("stage").and_then(as_string);
        let parent_stage = map.remove("parent_stage").and_then(as_string);
        let state = map.remove("state").and_then(as_string);
        // Nested metrics object (v2) flattened into extra under key "metrics" if present,
        // plus any leftover keys.
        Some(Self {
            job,
            t,
            phase,
            i,
            n,
            eta_s,
            schema,
            run_id,
            stage,
            parent_stage,
            state,
            extra: map,
        })
    }

    pub fn progress_pair(&self) -> Option<(String, String)> {
        let i = self.i.as_ref()?;
        let n = self.n.as_ref()?;
        // Match Python: both truthy
        if is_truthy(i) && is_truthy(n) {
            Some((format_num(i), format_num(n)))
        } else {
            None
        }
    }
}

fn as_f64(v: Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn as_string(v: Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn format_num(v: &Value) -> String {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                format!("{i}")
            } else if let Some(u) = n.as_u64() {
                format!("{u}")
            } else {
                n.to_string()
            }
        }
        other => other.to_string(),
    }
}

/// A status record, or None for ordinary log output. Never panics.
pub fn parse_line(line: &str) -> Option<StatusRecord> {
    let line = line.trim_end_matches(['\r', '\n']);
    if !line.starts_with(PREFIX) {
        return None;
    }
    let payload = &line[PREFIX.len()..];
    let v: Value = serde_json::from_str(payload).ok()?;
    StatusRecord::from_value(v)
}

/// Status records from the end of a log. Missing file → empty.
pub fn read_records(path: &Path, tail_bytes: u64) -> Vec<StatusRecord> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(meta) = file.metadata() else {
        return Vec::new();
    };
    let size = meta.len();
    if size > tail_bytes {
        if file.seek(SeekFrom::Start(size - tail_bytes)).is_err() {
            return Vec::new();
        }
        // Discard partial line the seek landed in.
        let mut discard = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(_) if buf[0] == b'\n' => break,
                Ok(_) => discard.push(buf[0]),
                Err(_) => return Vec::new(),
            }
        }
        let _ = discard;
    }
    let mut blob = Vec::new();
    if file.read_to_end(&mut blob).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&blob);
    let mut out = Vec::new();
    for raw in text.lines() {
        if let Some(rec) = parse_line(raw) {
            out.push(rec);
        }
    }
    out
}

/// Final non-empty, non-status line. Handles `\r` progress bars.
pub fn last_line(path: &Path, tail_bytes: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let Ok(meta) = file.metadata() else {
        return String::new();
    };
    let size = meta.len();
    if size > tail_bytes {
        let _ = file.seek(SeekFrom::Start(size.saturating_sub(tail_bytes)));
    }
    let mut blob = Vec::new();
    if file.read_to_end(&mut blob).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&blob);
    for raw in text.lines().rev() {
        let text = raw.rsplit('\r').next().unwrap_or(raw).trim();
        if !text.is_empty() && !text.starts_with(PREFIX) {
            return text.to_string();
        }
    }
    String::new()
}

/// Seconds since last write, or None if missing.
pub fn age(path: &Path) -> Option<f64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let now = SystemTime::now();
    let secs = now.duration_since(modified).ok()?.as_secs_f64();
    Some(secs.max(0.0))
}

pub fn human_secs(s: Option<f64>) -> String {
    let Some(s) = s else {
        return "--".into();
    };
    let s = s as i64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h{:02}", s / 3600, (s % 3600) / 60)
    }
}

pub fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Emit one ##ST record to stdout (flushed).
pub fn emit_record(job: &str, fields: serde_json::Map<String, Value>) -> anyhow::Result<()> {
    let mut map = serde_json::Map::new();
    map.insert("job".into(), Value::String(job.to_string()));
    map.insert("t".into(), Value::Number(serde_json::Number::from_f64(now_unix()).unwrap_or(0.into())));
    for (k, v) in fields {
        map.insert(k, v);
    }
    let line = format!("{PREFIX}{}", serde_json::to_string(&Value::Object(map))?);
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")?;
    out.flush()?;
    Ok(())
}

const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn sparkline(values: &[Option<f64>], width: usize) -> String {
    let vals: Vec<f64> = values.iter().copied().flatten().collect();
    let vals: Vec<f64> = if vals.len() > width {
        vals[vals.len() - width..].to_vec()
    } else {
        vals
    };
    if vals.is_empty() {
        return String::new();
    }
    let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (hi - lo).abs() < 1e-12 {
        return BLOCKS[0].to_string().repeat(vals.len());
    }
    let span = hi - lo;
    vals.iter()
        .map(|v| {
            let idx = (((v - lo) / span) * (BLOCKS.len() - 1) as f64) as usize;
            BLOCKS[idx.min(BLOCKS.len() - 1)]
        })
        .collect()
}

const DOT: [[u32; 4]; 2] = [
    [0x01, 0x02, 0x04, 0x40],
    [0x08, 0x10, 0x20, 0x80],
];

fn resample(values: &[f64], n: usize) -> Vec<f64> {
    if values.is_empty() || n == 0 {
        return Vec::new();
    }
    if values.len() >= n {
        return values[values.len() - n..].to_vec();
    }
    if values.len() == 1 {
        return vec![values[0]; n];
    }
    let mut out = Vec::with_capacity(n);
    let span = (values.len() - 1) as f64;
    for i in 0..n {
        let pos = i as f64 * span / (n - 1) as f64;
        let lo = pos as usize;
        let hi = (lo + 1).min(values.len() - 1);
        let frac = pos - lo as f64;
        out.push(values[lo] * (1.0 - frac) + values[hi] * frac);
    }
    out
}

/// Braille line plot; `lo`/`hi` fix scale when meaningful (e.g. 0–100).
pub fn braille_plot(
    values: &[f64],
    width: usize,
    height: usize,
    lo: Option<f64>,
    hi: Option<f64>,
) -> Vec<String> {
    if values.is_empty() || width < 1 || height < 1 {
        return vec![" ".repeat(width); height];
    }
    let cols = width * 2;
    let rows = height * 4;
    let pts = resample(values, cols);
    let lo_v = lo.unwrap_or_else(|| pts.iter().cloned().fold(f64::INFINITY, f64::min));
    let hi_v = hi.unwrap_or_else(|| pts.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let span = if (hi_v - lo_v).abs() < f64::EPSILON {
        1.0
    } else {
        hi_v - lo_v
    };

    let mut canvas = vec![vec![0u32; width]; height];
    for (x, v) in pts.iter().enumerate() {
        let frac = ((v - lo_v) / span).clamp(0.0, 1.0);
        let y = rows - 1 - (frac * (rows - 1) as f64) as usize;
        canvas[y / 4][x / 2] |= DOT[x % 2][y % 4];
    }
    canvas
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|c| char::from_u32(0x2800 + c).unwrap_or(' '))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn status_line_parses_ordinary_does_not() {
        let line = format!("{PREFIX}{}", r#"{"job":"j","i":3}"#);
        assert_eq!(parse_line(&line).unwrap().i.unwrap().as_i64(), Some(3));
        assert!(parse_line("[  12] crops").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_line(&format!("{PREFIX}{{not json")).is_none());
        assert!(parse_line(&format!("{PREFIX}[1,2,3]")).is_none());
    }

    #[test]
    fn records_from_end_of_large_log() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("big.log");
        let mut f = File::create(&p).unwrap();
        for _ in 0..4000 {
            writeln!(f, "{}", "x".repeat(200)).unwrap();
        }
        writeln!(f, "{PREFIX}{{\"job\":\"j\",\"i\":1}}").unwrap();
        writeln!(f, "noise").unwrap();
        writeln!(f, "{PREFIX}{{\"job\":\"j\",\"i\":2}}").unwrap();
        let got = read_records(&p, 4096);
        let is: Vec<_> = got.iter().map(|r| r.i.as_ref().unwrap().as_i64().unwrap()).collect();
        assert_eq!(is, vec![1, 2]);
    }

    #[test]
    fn last_line_handles_cr_and_skips_status() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.log");
        let mut f = File::create(&p).unwrap();
        writeln!(f, "[ 1] slide one").unwrap();
        writeln!(f, "{PREFIX}{{\"job\":\"j\",\"i\":1}}").unwrap();
        assert_eq!(last_line(&p, 8192), "[ 1] slide one");

        let p2 = dir.path().join("b.log");
        let mut f2 = File::create(&p2).unwrap();
        write!(f2, "Loading:  10%\rLoading:  50%\rLoading: 100%\n").unwrap();
        assert_eq!(last_line(&p2, 8192), "Loading: 100%");
    }

    #[test]
    fn human_secs_glance() {
        assert_eq!(human_secs(Some(0.0)), "0s");
        assert_eq!(human_secs(Some(45.0)), "45s");
        assert_eq!(human_secs(Some(90.0)), "1m");
        assert_eq!(human_secs(Some(3600.0)), "1h00");
        assert_eq!(human_secs(Some(11160.0)), "3h06");
        assert_eq!(human_secs(None), "--");
    }

    #[test]
    fn braille_idle_fixed_scale() {
        let idle = [0.0, 1.0, 0.0, 2.0, 1.0, 0.0, 1.0, 0.0];
        let fixed = braille_plot(&idle, 8, 4, Some(0.0), Some(100.0));
        assert!(fixed[0].chars().all(|c| c == '\u{2800}'));
    }

    #[test]
    fn v2_optional_fields_parse() {
        let line = format!(
            "{PREFIX}{}",
            r#"{"job":"j","schema":"mqtop/2","run_id":"r1","stage":"train","parent_stage":"prep","state":"running","i":1,"n":10}"#
        );
        let r = parse_line(&line).unwrap();
        assert_eq!(r.schema.as_deref(), Some("mqtop/2"));
        assert_eq!(r.run_id.as_deref(), Some("r1"));
        assert_eq!(r.stage.as_deref(), Some("train"));
        assert_eq!(r.parent_stage.as_deref(), Some("prep"));
        assert_eq!(r.state.as_deref(), Some("running"));
    }
}
