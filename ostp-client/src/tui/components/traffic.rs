use ratatui::layout::Rect;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Sparkline};
use ratatui::Frame;

use crate::app::AppState;

pub struct TrafficComponent;

impl TrafficComponent {
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let incoming = Sparkline::default()
            .block(Block::default()
                .title(" ▼ INCOMING TRAFFIC DISTRIBUTION ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green)))
            .data(&state.incoming_history)
            .style(Style::default().fg(Color::LightGreen));

        let outgoing = Sparkline::default()
            .block(Block::default()
                .title(" ▲ OUTGOING TRAFFIC DISTRIBUTION ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Magenta)))
            .data(&state.outgoing_history)
            .style(Style::default().fg(Color::LightMagenta));

        frame.render_widget(incoming, rows[0]);
        frame.render_widget(outgoing, rows[1]);
    }
}
