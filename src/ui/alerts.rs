use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::{AlertLevel, App};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = if app.alerts.is_empty() {
        vec![ListItem::new(Span::styled(
            "no alerts",
            Style::new().fg(Color::DarkGray),
        ))]
    } else {
        app.alerts
            .iter()
            .map(|a| {
                let (tag, color) = match a.level {
                    AlertLevel::Info => ("info", Color::Cyan),
                    AlertLevel::Warn => ("warn", Color::Yellow),
                    AlertLevel::Error => ("err", Color::Red),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{tag}] "), Style::new().fg(color)),
                    Span::raw(a.message.clone()),
                ]))
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(Block::new().borders(Borders::ALL).title("alerts")),
        area,
    );
}
