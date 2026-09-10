pub mod alerts;
pub mod diagnostics;
pub mod fleet;
pub mod help;
pub mod host;
pub mod job_detail;
pub mod overview;
pub mod pipeline;
pub mod runs;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    JobDetail,
    Host,
    Alerts,
    Pipeline,
    Runs,
    Fleet,
    Diagnostics,
    Help,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Self::Overview,
        Self::JobDetail,
        Self::Host,
        Self::Alerts,
        Self::Pipeline,
        Self::Runs,
        Self::Fleet,
        Self::Diagnostics,
        Self::Help,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::JobDetail => "Job",
            Self::Host => "Host",
            Self::Alerts => "Alerts",
            Self::Pipeline => "Pipeline",
            Self::Runs => "Runs",
            Self::Fleet => "Fleet",
            Self::Diagnostics => "Diag",
            Self::Help => "Help",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        let i = (self.index() + 1) % Self::ALL.len();
        Self::ALL[i]
    }

    pub fn prev(self) -> Self {
        let i = if self.index() == 0 {
            Self::ALL.len() - 1
        } else {
            self.index() - 1
        };
        Self::ALL[i]
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(frame, app, header);
    render_tabs(frame, app, tabs);
    match app.tab {
        Tab::Overview => overview::render(frame, app, body),
        Tab::JobDetail => job_detail::render(frame, app, body),
        Tab::Host => host::render(frame, app, body),
        Tab::Alerts => alerts::render(frame, app, body),
        Tab::Pipeline => pipeline::render(frame, app, body),
        Tab::Runs => runs::render(frame, app, body),
        Tab::Fleet => fleet::render(frame, app, body),
        Tab::Diagnostics => diagnostics::render(frame, app, body),
        Tab::Help => help::render(frame, app, body),
    }
    render_footer(frame, app, footer);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let host = &app.sample.host.hostname;
    let now = chrono::Local::now().format("%H:%M:%S");
    let left = format!("mqtop-rs · {host}");
    let right = format!("{now}  q quit");
    let line = Line::from(vec![
        Span::styled("mqtop-rs", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" · {host}")),
        Span::raw("  "),
        Span::styled(right, Style::new().fg(Color::DarkGray)),
    ]);
    let _ = left;
    frame.render_widget(Paragraph::new(line), area);
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(titles)
        .block(Block::new().borders(Borders::ALL).title("views"))
        .select(app.tab.index())
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" │ ");
    frame.render_widget(tabs, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let src = &app.sample.source;
    let mode = match app.input_mode {
        crate::app::InputMode::Normal => "",
        crate::app::InputMode::Filter => " [filter] ",
        crate::app::InputMode::Search => " [search] ",
    };
    let text = format!("jobs from {src}{mode}  Tab/h/l views  j/k select  / filter  f follow");
    frame.render_widget(
        Paragraph::new(Span::styled(text, Style::new().fg(Color::DarkGray))),
        area,
    );
}

pub fn state_style(state: crate::domain::JobState) -> Style {
    use crate::domain::JobState::*;
    match state {
        Running => Style::new().fg(Color::Green),
        Stalled => Style::new().fg(Color::Yellow),
        Ended => Style::new().fg(Color::DarkGray),
        Quiet | NotRunning | NoLog => Style::new().fg(Color::DarkGray),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::sample::Sample;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    #[test]
    fn renders_empty_overview() {
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        let app = App::new(Sample::empty("test", PathBuf::from(".")));
        term.draw(|f| render(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let flat: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(flat.contains("mqtop-rs"));
        assert!(flat.contains("Overview") || flat.contains("views"));
    }
}
