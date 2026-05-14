use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;

pub struct DashboardComponent;

fn format_speed(bps: u64) -> String {
    let bytes = bps / 8;
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;

    if bytes >= MB {
        format!("{:.2} MB/s ({:.1} Mbps)", bytes as f64 / MB as f64, bps as f64 / 1_000_000.0)
    } else if bytes >= KB {
        format!("{:.2} KB/s ({:.1} Kbps)", bytes as f64 / KB as f64, bps as f64 / 1_000.0)
    } else {
        format!("{} B/s ({} bps)", bytes, bps)
    }
}


impl DashboardComponent {
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let status_span = match state.status.as_str().to_lowercase().as_str() {
            "connected" | "active" => Span::styled(" CONNECTED ", Style::default().fg(Color::Black).bg(Color::LightGreen).add_modifier(ratatui::style::Modifier::BOLD)),
            "connecting" | "handshaking" => Span::styled(" CONNECTING ", Style::default().fg(Color::Black).bg(Color::LightYellow).add_modifier(ratatui::style::Modifier::BOLD)),
            _ => Span::styled(" DISCONNECTED ", Style::default().fg(Color::Black).bg(Color::LightRed).add_modifier(ratatui::style::Modifier::BOLD)),
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("● Status:       ", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
                status_span,
                Span::raw("   |   "),
                Span::styled("⚡ RTT: ", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
                Span::styled(format!("{:.1} ms", state.rtt_ms), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("▲ Throughput:   ", Style::default().fg(Color::Green)),
                Span::styled(format_speed(state.throughput_bps), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("🎭 Profile:      ", Style::default().fg(Color::Magenta)),
                Span::styled(format!("{:?}", state.active_profile), Style::default().fg(Color::LightMagenta)),
                Span::raw("   |   "),
                Span::styled("🔒 XOR Headers: ", Style::default().fg(Color::LightCyan)),
                Span::styled("ACTIVE", Style::default().fg(Color::LightGreen).add_modifier(ratatui::style::Modifier::BOLD)),
            ]),
        ];

        let widget = Paragraph::new(lines).block(Block::default()
            .title(" OSTP CLIENT DASHBOARD ")
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(Color::LightCyan)));
        frame.render_widget(widget, area);
    }
}
