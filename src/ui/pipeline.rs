use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::state_style;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let view = app.pipeline_view();
    let mut lines = vec![Line::from(format!(
        "stages: {}  runs: {}",
        view.stages.len(),
        view.run_ids.len()
    ))];
    if view.stages.is_empty() {
        lines.push(Line::from(Span::styled(
            "no stage/run_id fields yet — emit v2 records or use job names",
            Style::new().fg(Color::DarkGray),
        )));
        for j in &app.sample.jobs {
            lines.push(Line::from(vec![
                Span::raw(format!(" • {} ", j.job.name)),
                Span::styled(j.state.as_str(), state_style(j.state)),
            ]));
        }
    } else {
        for s in &view.stages {
            let marker = if s.critical { "!" } else { " " };
            lines.push(Line::from(vec![
                Span::raw(format!("{marker} {} ", s.name)),
                Span::styled(s.state_label.clone(), state_style(s.job_state)),
                Span::raw(
                    s.parent
                        .as_ref()
                        .map(|p| format!("  ← {p}"))
                        .unwrap_or_default(),
                ),
            ]));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title("pipeline")),
        area,
    );
}
