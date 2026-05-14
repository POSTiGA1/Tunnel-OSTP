pub mod components;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::{AppState, BridgeCommand, UiEvent};
use crate::config::ClientConfig;
use crate::tui::components::controls::ControlsComponent;
use crate::tui::components::dashboard::DashboardComponent;
use crate::tui::components::logs::LogsComponent;
use crate::tui::components::traffic::TrafficComponent;

struct KeyEditorState {
    open: bool,
    focus: KeyEditorField,
    server_addr: String,
    access_key: String,
}

#[derive(Clone, Copy)]
enum KeyEditorField {
    ServerAddr,
    AccessKey,
}

enum KeyEditorAction {
    Noop,
    Saved,
    Canceled,
}

pub struct TuiRuntime {
    state: AppState,
    config: ClientConfig,
    dashboard: DashboardComponent,
    logs: LogsComponent,
    traffic: TrafficComponent,
    controls: ControlsComponent,
    key_editor: KeyEditorState,
}

pub enum TuiExit {
    Exit,
    Background,
}

impl TuiRuntime {
    pub fn new(config: ClientConfig) -> Self {
        let key_editor = KeyEditorState {
            open: false,
            focus: KeyEditorField::ServerAddr,
            server_addr: config.ostp.server_addr.clone(),
            access_key: config.ostp.access_key.clone(),
        };

        Self {
            state: AppState::new(),
            config,
            dashboard: DashboardComponent,
            logs: LogsComponent,
            traffic: TrafficComponent,
            controls: ControlsComponent,
            key_editor,
        }
    }

    pub async fn run(
        mut self,
        ui_rx: mpsc::Receiver<UiEvent>,
        cmd_tx: mpsc::Sender<BridgeCommand>,
    ) -> Result<TuiExit> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal, ui_rx, cmd_tx).await;

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        mut ui_rx: mpsc::Receiver<UiEvent>,
        cmd_tx: mpsc::Sender<BridgeCommand>,
    ) -> Result<TuiExit> {
        loop {
            while let Ok(ev) = ui_rx.try_recv() {
                self.state.apply_event(ev);
            }

            terminal.draw(|frame| {
                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(6),
                        Constraint::Length(12),
                        Constraint::Min(8),
                        Constraint::Length(6),
                    ])
                    .split(frame.area());

                self.dashboard.render(frame, root[0], &self.state);
                self.traffic.render(frame, root[1], &self.state);
                self.logs.render(frame, root[2], &self.state);
                self.controls.render(frame, root[3]);

                if self.key_editor.open {
                    render_key_editor(frame, frame.area(), &self.key_editor);
                }
            })?;

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        if self.key_editor.open {
                            match self.handle_key_editor_input(key.code) {
                                KeyEditorAction::Saved => {
                                    let _ = cmd_tx.send(BridgeCommand::ReloadConfig).await;
                                    continue;
                                }
                                KeyEditorAction::Canceled | KeyEditorAction::Noop => {
                                    continue;
                                }
                            }
                        }

                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                let _ = cmd_tx.send(BridgeCommand::Shutdown).await;
                                return Ok(TuiExit::Exit);
                            }
                            KeyCode::Char('b') | KeyCode::Char('B') => {
                                self.push_local_log("TUI detached; client continues in background".to_string());
                                return Ok(TuiExit::Background);
                            }
                            KeyCode::Char('k') | KeyCode::Char('K') => {
                                self.key_editor.open = true;
                                self.key_editor.focus = KeyEditorField::ServerAddr;
                                self.key_editor.server_addr = self.config.ostp.server_addr.clone();
                                self.key_editor.access_key = self.config.ostp.access_key.clone();
                            }
                            KeyCode::Char(' ') => {
                                let _ = cmd_tx.send(BridgeCommand::ToggleTunnel).await;
                            }
                            KeyCode::Tab => {
                                let _ = cmd_tx.send(BridgeCommand::NextProfile).await;
                            }
                            KeyCode::Up => {
                                self.state.log_scroll = self.state.log_scroll.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                self.state.log_scroll = self.state.log_scroll.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(16)).await;
        }

        #[allow(unreachable_code)]
        Ok(TuiExit::Exit)
    }

    fn handle_key_editor_input(&mut self, code: KeyCode) -> KeyEditorAction {
        match code {
            KeyCode::Esc => {
                self.key_editor.open = false;
                self.push_local_log("Key editor canceled".to_string());
                KeyEditorAction::Canceled
            }
            KeyCode::Tab => {
                self.key_editor.focus = match self.key_editor.focus {
                    KeyEditorField::ServerAddr => KeyEditorField::AccessKey,
                    KeyEditorField::AccessKey => KeyEditorField::ServerAddr,
                };
                KeyEditorAction::Noop
            }
            KeyCode::Backspace => {
                match self.key_editor.focus {
                    KeyEditorField::ServerAddr => {
                        self.key_editor.server_addr.pop();
                    }
                    KeyEditorField::AccessKey => {
                        self.key_editor.access_key.pop();
                    }
                }
                KeyEditorAction::Noop
            }
            KeyCode::Enter => {
                if self.key_editor.server_addr.trim().is_empty() {
                    self.push_local_log("Save failed: server address cannot be empty".to_string());
                    return KeyEditorAction::Noop;
                }
                if self.key_editor.access_key.trim().is_empty() {
                    self.push_local_log("Save failed: access key cannot be empty".to_string());
                    return KeyEditorAction::Noop;
                }

                self.config.ostp.server_addr = self.key_editor.server_addr.trim().to_string();
                self.config.ostp.access_key = self.key_editor.access_key.trim().to_string();

                match self.config.save_to_json_near_binary() {
                    Ok(()) => self.push_local_log(
                        "Config saved to config.json"
                            .to_string(),
                    ),
                    Err(err) => self.push_local_log(format!("Save failed: {err}")),
                }

                self.key_editor.open = false;
                KeyEditorAction::Saved
            }
            KeyCode::Char(c) => {
                match self.key_editor.focus {
                    KeyEditorField::ServerAddr => self.key_editor.server_addr.push(c),
                    KeyEditorField::AccessKey => self.key_editor.access_key.push(c),
                }
                KeyEditorAction::Noop
            }
            _ => KeyEditorAction::Noop,
        }
    }

    fn push_local_log(&mut self, line: String) {
        self.state.apply_event(UiEvent::Log(line));
    }
}

fn render_key_editor(frame: &mut ratatui::Frame<'_>, area: Rect, editor: &KeyEditorState) {
    let popup = centered_rect(80, 45, area);
    frame.render_widget(Clear, popup);

    let block = Block::default().title("Edit Keys").borders(Borders::ALL);
    frame.render_widget(block, popup);

    let inner = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Server Addr:",
                if matches!(editor.focus, KeyEditorField::ServerAddr) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                },
            ),
            Span::raw(" "),
            Span::raw(editor.server_addr.as_str()),
        ]),
        Line::from(vec![
            Span::styled(
                "Access Key:",
                if matches!(editor.focus, KeyEditorField::AccessKey) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                },
            ),
            Span::raw(" "),
            Span::raw(editor.access_key.as_str()),
        ]),
        Line::from(""),
        Line::from("Tab switch field, Enter save+reload, Esc cancel"),
    ];

    let widget = Paragraph::new(lines).alignment(Alignment::Left);
    frame.render_widget(widget, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
}
