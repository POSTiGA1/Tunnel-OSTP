use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use ratatui::style::{Color, Style};

use crate::app::AppState;

pub struct LogsComponent;

impl LogsComponent {
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let lines: Vec<ratatui::text::Line<'_>> = state.logs.iter().map(|l| ratatui::text::Line::from(l.as_str())).collect();
        let widget = Paragraph::new(lines)
            .block(Block::default()
                .title(" SYSTEM LOGS ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow)))
            .scroll((state.log_scroll, 0));
        frame.render_widget(widget, area);
    }
}
