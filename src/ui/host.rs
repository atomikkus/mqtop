use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::domain::status::braille_plot;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let h = &app.sample.host;
    let [cpu, mem, gpu, disk] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_series(
        frame,
        cpu,
        "cpu",
        h.cpu_pct,
        app.sample.hist_cpu.iter().copied().collect::<Vec<_>>(),
    );
    render_series(
        frame,
        mem,
        "mem",
        h.mem_pct,
        app.sample.hist_mem.iter().copied().collect::<Vec<_>>(),
    );
    render_series(
        frame,
        gpu,
        "gpu",
        h.gpu_util,
        app.sample.hist_gpu.iter().copied().collect::<Vec<_>>(),
    );

    let gpu_mem = match (h.gpu_mem_used_gb, h.gpu_mem_total_gb) {
        (Some(u), Some(t)) => format!("{u:.1}/{t:.0} GB"),
        _ => "--".into(),
    };
    let ram = h
        .mem_used_gb
        .map(|g| format!("{g:.0} GB"))
        .unwrap_or_else(|| "--".into());
    let disk_line = match (h.disk_free_gb, h.disk_used_pct) {
        (Some(f), Some(p)) => format!("{f:.0} GB free  {p:.0}% used"),
        _ => "disk unavailable".into(),
    };
    let notes = if h.notes.is_empty() {
        String::new()
    } else {
        format!("\n {}", h.notes.join("; "))
    };
    frame.render_widget(
        Paragraph::new(format!(
            " gpu mem {gpu_mem:<14} ram {ram}\n disk    {disk_line}{notes}"
        ))
        .block(Block::new().borders(Borders::ALL).title("host summary")),
        disk,
    );
}

fn render_series(frame: &mut Frame, area: Rect, label: &str, current: Option<f64>, hist: Vec<f64>) {
    let title = match current {
        Some(v) => format!("{label} {v:.0}%"),
        None => format!("{label} --"),
    };
    let width = area.width.saturating_sub(2) as usize;
    let rows = if hist.is_empty() {
        vec![" ".repeat(width.max(1)); 4]
    } else {
        braille_plot(&hist, width.max(1), 4, Some(0.0), Some(100.0))
    };
    let lines: Vec<Line> = rows.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(Color::Cyan))
            .block(Block::new().borders(Borders::ALL).title(title)),
        area,
    );
}
