use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let h = &app.sample.host;
    let lines = vec![
        Line::from(format!("hostname: {}", h.hostname)),
        Line::from(format!("source:   {}", app.sample.source)),
        Line::from(format!("watch:    {}", app.sample.watch_dir.display())),
        Line::from(format!("jobs:     {}", app.sample.jobs.len())),
        Line::from(format!(
            "cpu:      {}",
            h.cpu_pct.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "--".into())
        )),
        Line::from(format!(
            "gpu:      {}",
            h.gpu_util
                .map(|v| format!("{v:.0}%"))
                .unwrap_or_else(|| "--".into())
        )),
        Line::from(format!(
            "notes:    {}",
            if h.notes.is_empty() {
                "(none)".into()
            } else {
                h.notes.join("; ")
            }
        )),
        Line::from("read-only monitor — no process control"),
        Line::from("Linux collectors: /proc, pgrep, nvidia-smi; else unavailable"),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title("diagnostics")),
        area,
    );
}
