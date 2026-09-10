use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::domain::status::human_secs;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![Line::from(
        "fleet view — reconnect shows last-seen age (read-only)",
    )];
    if app.fleet_hosts.is_empty() {
        lines.push(Line::from(Span::styled(
            "no remote hosts; run mqtop-agent on boxes and point snapshot URL here later",
            Style::new().fg(Color::DarkGray),
        )));
        lines.push(Line::from(format!(
            " local · {} · {} jobs",
            app.sample.host.hostname,
            app.sample.jobs.len()
        )));
    } else {
        for h in &app.fleet_hosts {
            let age = human_secs(h.last_seen_age_s);
            lines.push(Line::from(format!(
                " {}  {}  jobs={}  last-seen {}",
                h.name, h.status, h.job_count, age
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title("fleet")),
        area,
    );
}
