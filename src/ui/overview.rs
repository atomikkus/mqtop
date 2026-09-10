use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::domain::status::human_secs;
use crate::ui::state_style;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let idxs = app.visible_indices();
    if idxs.is_empty() {
        let msg = format!(" no logs found in {}", app.sample.watch_dir.display());
        frame.render_widget(
            Paragraph::new(Span::styled(msg, Style::new().fg(Color::DarkGray)))
                .block(Block::new().borders(Borders::ALL).title("jobs")),
            area,
        );
        return;
    }

    let header = Row::new(["", "name", "state", "phase", "progress", "eta", "quiet"])
        .style(Style::new().add_modifier(Modifier::BOLD));

    let rows = idxs.iter().enumerate().map(|(vis_i, &ji)| {
        let j = &app.sample.jobs[ji];
        let rec = j.records.last();
        let phase = rec
            .and_then(|r| r.phase.clone())
            .unwrap_or_default();
        let progress = rec
            .and_then(|r| r.progress_pair())
            .map(|(i, n)| format!("{i}/{n}"))
            .or_else(|| {
                rec.and_then(|r| r.i.as_ref().map(|v| v.to_string()))
            })
            .unwrap_or_else(|| {
                if !j.tail.is_empty() {
                    truncate(&j.tail, 24)
                } else {
                    String::new()
                }
            });
        let eta = rec
            .and_then(|r| r.eta_s)
            .map(|s| human_secs(Some(s)))
            .unwrap_or_else(|| "--".into());
        let quiet = human_secs(j.since);
        let marker = if vis_i == app.selected { ">" } else { " " };
        let style = if vis_i == app.selected {
            Style::new().bg(Color::Indexed(236))
        } else {
            Style::new()
        };
        Row::new([
            Cell::from(marker),
            Cell::from(j.job.name.clone()),
            Cell::from(j.state.as_str()).style(state_style(j.state)),
            Cell::from(phase),
            Cell::from(progress),
            Cell::from(eta),
            Cell::from(quiet),
        ])
        .style(style)
    });

    let [table_area, filter_area] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .areas(area);

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Fill(1),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title("jobs"));

    frame.render_widget(table, table_area);
    if !app.filter.is_empty() || matches!(app.input_mode, crate::app::InputMode::Filter) {
        frame.render_widget(
            Paragraph::new(format!("filter: {}", app.filter)),
            filter_area,
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}
