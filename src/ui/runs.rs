use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let cmp = app.experiment_compare();
    let mut lines = vec![Line::from(format!("runs compared: {}", cmp.runs.len()))];
    if cmp.runs.is_empty() {
        lines.push(Line::from(Span::styled(
            "emit run_id on ##ST records to group experiments",
            Style::new().fg(Color::DarkGray),
        )));
    }
    for run in &cmp.runs {
        lines.push(Line::from(format!(
            " run {}  stages={}  last_i={}",
            run.run_id,
            run.stages.join(","),
            run.last_i
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into())
        )));
        for (k, v) in &run.metrics {
            lines.push(Line::from(format!("   {k}={v}")));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::new()
                .borders(Borders::ALL)
                .title("experiment compare"),
        ),
        area,
    );
}
