use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline};
use ratatui::Frame;

use crate::app::App;
use crate::domain::status::human_secs;
use crate::ui::state_style;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let Some(ji) = app.selected_job_index() else {
        frame.render_widget(
            Paragraph::new("select a job on Overview (j/k)").block(
                Block::new().borders(Borders::ALL).title("job detail"),
            ),
            area,
        );
        return;
    };
    let j = &app.sample.jobs[ji];
    let [meta, charts, log] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(6),
        Constraint::Fill(1),
    ])
    .areas(area);

    let rec = j.records.last();
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {} ", j.job.name), Style::new().fg(Color::Cyan)),
        Span::styled(j.state.as_str(), state_style(j.state)),
        Span::raw(format!("  quiet {}", human_secs(j.since))),
    ])];
    lines.push(Line::from(format!(" log {}", j.job.log.display())));
    lines.push(Line::from(format!(
        " match {}{}",
        j.job.match_pat,
        if j.job.match_is_guess { " (guess)" } else { "" }
    )));
    if let Some(r) = rec {
        let prog = r
            .progress_pair()
            .map(|(i, n)| format!("{i}/{n}"))
            .unwrap_or_default();
        lines.push(Line::from(format!(
            " phase {}  {prog}  eta {}",
            r.phase.as_deref().unwrap_or("-"),
            human_secs(r.eta_s)
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title("status")),
        meta,
    );

    // Progress spark from record i values
    let series: Vec<u64> = j
        .records
        .iter()
        .filter_map(|r| r.i.as_ref().and_then(|v| v.as_f64()).map(|x| x as u64))
        .collect();
    let spark = Sparkline::default()
        .data(&series)
        .style(Style::new().fg(Color::Cyan))
        .block(Block::new().borders(Borders::ALL).title("progress (i)"));
    frame.render_widget(spark, charts);

    let mut log_lines: Vec<Line> = Vec::new();
    if let Some(h) = &j.malformed_hint {
        log_lines.push(Line::from(Span::styled(
            h.clone(),
            Style::new().fg(Color::Yellow),
        )));
    }
    for r in j.records.iter().rev().take(20) {
        let phase = r.phase.as_deref().unwrap_or("");
        let prog = r
            .progress_pair()
            .map(|(i, n)| format!("{i}/{n}"))
            .unwrap_or_default();
        log_lines.push(Line::from(format!("##ST {phase} {prog}")));
    }
    if j.records.is_empty() && !j.tail.is_empty() {
        log_lines.push(Line::from(j.tail.clone()));
    }
    if !app.log_search.is_empty() {
        log_lines.retain(|l| {
            l.spans
                .iter()
                .any(|s| s.content.to_lowercase().contains(&app.log_search.to_lowercase()))
        });
    }
    let follow = if app.log_follow { "follow" } else { "paused" };
    frame.render_widget(
        Paragraph::new(log_lines)
            .block(Block::new().borders(Borders::ALL).title(format!("log [{follow}]")))
            .scroll((app.log_scroll, 0)),
        log,
    );
}
