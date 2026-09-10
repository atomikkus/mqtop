use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(frame: &mut Frame, _app: &App, area: Rect) {
    let text = vec![
        Line::from("mqtop-rs — read-only job monitor (##ST protocol)"),
        Line::from(""),
        Line::from("q          quit"),
        Line::from("Tab / h l  switch views"),
        Line::from("j k        select job"),
        Line::from("/          filter jobs"),
        Line::from("f          toggle log follow"),
        Line::from("s          cycle sort"),
        Line::from(""),
        Line::from("CLI: mqtop-rs --once | emit | init | doctor | snapshot"),
        Line::from("Agents: mqtop-rs init --agents universal,cursor,claude --yes"),
    ];
    frame.render_widget(
        Paragraph::new(text).block(Block::new().borders(Borders::ALL).title("help")),
        area,
    );
}
