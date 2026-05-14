use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub struct ControlsComponent;

impl ControlsComponent {
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let text = vec![
            Line::from(vec![
                Span::styled(" [Space] ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Toggle Tunnel   "),
                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Obfuscation Profile   "),
                Span::styled(" [K] ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Edit Config "),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" [B] ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Detach (Background)   "),
                Span::styled(" [Up/Down] ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Scroll Logs   "),
                Span::styled(" [Esc/Q] ", Style::default().fg(Color::Red).add_modifier(ratatui::style::Modifier::REVERSED)),
                Span::raw(" Exit "),
            ]),
        ];

        let widget = Paragraph::new(text)
            .alignment(ratatui::layout::Alignment::Center)
            .block(Block::default()
                .title(" CONTROLS ")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(Color::Gray)));
        frame.render_widget(widget, area);
    }
}
