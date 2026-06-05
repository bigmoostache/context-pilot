//! Thin attach client for the headless daemon.
//!
//! Connects to a running daemon's Unix socket, receives IR frames, and
//! renders them locally via the existing IR→ratatui adapters. Terminal
//! input is captured and forwarded to the daemon as [`ClientMessage::Input`].
//!
//! The client is stateless with respect to application logic — it owns
//! only the terminal, a socket connection, and minimal scroll state for
//! rendering. All business logic lives in the daemon.

use super::protocol::{self, ClientMessage, DaemonMessage, ProtocolError};
use cp_render::frame::Frame as IrFrame;
use crossterm::ExecutableCommand as _;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::prelude::{Constraint, Direction, Layout, Line, Rect, Span, Style};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use crate::ui::ir::{blocks_to_lines, render_sidebar, render_status_bar, semantic_to_style};
use crate::ui::theme;
use cp_base::cast::Safe as _;
use cp_render::Semantic;

/// Max seconds to poll for the daemon socket during reconnection.
const RECONNECT_TIMEOUT_SECS: u64 = 30;

/// When supervised by `run.sh` (`CP_RUN_SH=1`), the client should NOT
/// attempt reconnection on daemon shutdown — the supervisor rebuilds
/// from source and relaunches the whole headless stack. Reconnection
/// is only useful in production where the daemon `exec()`-restarts itself.
fn should_reconnect() -> bool {
    std::env::var_os("CP_RUN_SH").is_none()
}

// ── Scroll state ────────────────────────────────────────────────

/// Minimal per-region scroll tracking for the client.
struct ScrollState {
    /// Conversation scroll offset (lines from top).
    conversation: f32,
    /// Active panel scroll offset.
    panel: f32,
}

impl ScrollState {
    const fn new() -> Self {
        Self { conversation: 0.0, panel: 0.0 }
    }
}

// ── Client ──────────────────────────────────────────────────────

/// A thin TUI client that attaches to a running headless daemon.
pub(crate) struct HeadlessClient {
    writer: BufWriter<UnixStream>,
    rx: Receiver<DaemonMessage>,
    scroll: ScrollState,
    /// Socket path for reconnection after daemon reload.
    socket_path: PathBuf,
}

impl HeadlessClient {
    /// Connect to a daemon at the given socket path.
    ///
    /// Sends an `Attach` handshake with the current terminal dimensions,
    /// then spawns a background reader thread for incoming daemon messages.
    pub(crate) fn connect(socket_path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket_path)?;
        let reader_stream = stream.try_clone()?;

        let mut writer = BufWriter::new(stream);

        // Send attach handshake
        let (cols, rows) = terminal::size()?;
        protocol::write_message(&mut writer, &ClientMessage::Attach { cols, rows })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        writer.flush()?;

        // Spawn reader thread
        let (tx, rx) = mpsc::channel();
        drop(
            thread::Builder::new()
                .name("headless-client-reader".into())
                .spawn(move || {
                    let mut reader = BufReader::new(reader_stream);
                    loop {
                        match protocol::read_message::<_, DaemonMessage>(&mut reader) {
                            Ok(msg) => {
                                if tx.send(msg).is_err() {
                                    break;
                                }
                            }
                            Err(ProtocolError::ConnectionClosed) => break,
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
        );

        Ok(Self { writer, rx, scroll: ScrollState::new(), socket_path: socket_path.to_path_buf() })
    }

    /// Run the client event loop until detach or quit.
    ///
    /// Initialises the terminal, enters the render loop, and restores
    /// the terminal on exit — even on panic (via a drop guard).
    /// Handles auto-reconnection when the daemon restarts (reload).
    pub(crate) fn run(&mut self) -> io::Result<()> {
        // Terminal setup
        terminal::enable_raw_mode()?;
        let _r = io::stdout().execute(EnterAlternateScreen)?;
        let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let result = loop {
            match self.event_loop(&mut terminal) {
                Ok(false) => break Ok(()), // Clean quit or detach
                Ok(true) => {
                    // Daemon sent Shutdown — try to reconnect (reload)
                    if !self.reconnect_loop(&mut terminal)? {
                        break Ok(()); // Timeout or user cancelled
                    }
                    // Successfully reconnected — re-enter event loop
                }
                Err(e) => break Err(e),
            }
        };

        // Cleanup — always restore terminal
        terminal::disable_raw_mode()?;
        let _r = io::stdout().execute(LeaveAlternateScreen)?;

        result
    }

    /// Core event loop: poll terminal input, drain daemon messages, render.
    ///
    /// Returns `Ok(true)` if the daemon sent a `Shutdown` message (reconnect
    /// needed), or `Ok(false)` for a clean exit (detach / quit).
    fn event_loop(
        &mut self,
        terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    ) -> io::Result<bool> {
        let mut last_frame: Option<IrFrame> = None;
        let mut needs_render = true;

        loop {
            // 1. Poll terminal for input (8ms timeout for responsive rendering)
            if event::poll(Duration::from_millis(8))? {
                let ev = event::read()?;

                match &ev {
                    // Ctrl+Z → detach (client exits, daemon keeps running)
                    Event::Key(KeyEvent { code: KeyCode::Char('z'), modifiers, .. })
                        if modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        drop(protocol::write_message(&mut self.writer, &ClientMessage::Detach));
                        drop(self.writer.flush());
                        return Ok(false);
                    }
                    // Ctrl+Q → quit (tell daemon to shut down)
                    Event::Key(KeyEvent { code: KeyCode::Char('q'), modifiers, .. })
                        if modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        drop(protocol::write_message(&mut self.writer, &ClientMessage::Quit));
                        drop(self.writer.flush());
                        return Ok(false);
                    }
                    // Terminal resize → notify daemon
                    Event::Resize(cols, rows) => {
                        drop(protocol::write_message(
                            &mut self.writer,
                            &ClientMessage::Attach { cols: *cols, rows: *rows },
                        ));
                        drop(self.writer.flush());
                        needs_render = true;
                    }
                    _ => {}
                }

                // Forward all events to the daemon
                if protocol::write_message(&mut self.writer, &ClientMessage::Input { event: ev }).is_err() {
                    return Ok(should_reconnect()); // Socket closed — reconnect in prod, exit in dev
                }
                drop(self.writer.flush());
            }

            // 2. Drain daemon messages
            while let Ok(msg) = self.rx.try_recv() {
                match msg {
                    DaemonMessage::FrameUpdate { frame } => {
                        last_frame = Some(frame);
                        needs_render = true;
                    }
                    DaemonMessage::Shutdown => return Ok(should_reconnect()),
                    DaemonMessage::Pong => {}
                }
            }

            // 3. Render when dirty
            if needs_render {
                if let Some(ref frame) = last_frame {
                    let scroll = &mut self.scroll;
                    drop(terminal.draw(|f| {
                        Self::render_frame(f, frame, scroll);
                    })?);
                    needs_render = false;
                }
            }
        }
    }

    // ── Reconnection ──────────────────────────────────────────

    /// Poll for the daemon to come back after a reload, re-establish the
    /// socket connection, and resume normal operation.
    ///
    /// Shows a "Reconnecting…" screen while waiting. The user can press
    /// Ctrl+Q to abort. Returns `true` if reconnection succeeded.
    fn reconnect_loop(
        &mut self,
        terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    ) -> io::Result<bool> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(RECONNECT_TIMEOUT_SECS);
        let max_delay = Duration::from_millis(500);
        let mut delay = Duration::from_millis(100);

        loop {
            let elapsed = start.elapsed();

            // Render reconnecting screen
            let remaining = timeout.saturating_sub(elapsed);
            drop(terminal.draw(|f| {
                Self::render_reconnecting(f, remaining);
            })?);

            if elapsed > timeout {
                return Ok(false);
            }

            // Check for Ctrl+Q to abort reconnection
            if event::poll(Duration::ZERO)? {
                if let Event::Key(KeyEvent { code: KeyCode::Char('q'), modifiers, .. }) = event::read()? {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        return Ok(false);
                    }
                }
            }

            thread::sleep(delay);
            delay = delay.saturating_mul(2).min(max_delay);

            // Try to connect once the socket file reappears
            if !self.socket_path.exists() {
                continue;
            }

            if let Ok(new_client) = HeadlessClient::connect(&self.socket_path) {
                // Swap connection state — old writer/rx drop naturally
                self.writer = new_client.writer;
                self.rx = new_client.rx;
                self.scroll = ScrollState::new();
                return Ok(true);
            }
        }
    }

    /// Render a centered "Reconnecting…" screen with countdown.
    fn render_reconnecting(f: &mut ratatui::Frame<'_>, remaining: Duration) {
        let area = f.area();
        let secs = remaining.as_secs();

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("⚓ Daemon restarting…", Style::default().fg(theme::accent()).bold())),
            Line::from(""),
            Line::from(Span::styled(
                format!("Reconnecting in {secs}s — Ctrl+Q to quit"),
                Style::default().fg(theme::text_muted()),
            )),
        ];

        // Center vertically
        let box_height = 6u16.min(area.height);
        let y = area.height.saturating_sub(box_height) >> 1;
        let centered = Rect::new(0, y, area.width, box_height);

        let para = Paragraph::new(lines)
            .style(Style::default().bg(theme::bg_surface()))
            .alignment(ratatui::prelude::Alignment::Center);

        // Fill background
        f.render_widget(Block::default().style(Style::default().bg(theme::bg_surface())), area);
        f.render_widget(para, centered);
    }

    // ── Rendering ────────────────────────────────────────────────

    /// Render a full IR frame to the terminal.
    ///
    /// Reuses the existing sidebar and status bar IR adapters. The
    /// conversation and panel areas use simplified renderers with
    /// auto-scroll-to-bottom behaviour.
    fn render_frame(f: &mut ratatui::Frame<'_>, frame: &IrFrame, scroll: &mut ScrollState) {
        let area = f.area();

        // Top-level vertical split: [content | status bar (1 line)]
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let content_area = vertical[0];
        let status_area = vertical[1];

        // Horizontal split: [sidebar | main content]
        let sidebar_width = match frame.sidebar.mode {
            cp_render::frame::SidebarMode::Normal => 38,
            cp_render::frame::SidebarMode::Collapsed => 3,
            cp_render::frame::SidebarMode::Hidden => 0,
        };
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
            .split(content_area);

        let sidebar_area = horizontal[0];
        let main_area = horizontal[1];

        // Render sidebar (reuse existing adapter)
        render_sidebar::render_sidebar_from_ir(f, &frame.sidebar, sidebar_area);

        // Render main content: panel if it has content, otherwise conversation
        if !frame.active_panel.blocks.is_empty() {
            Self::render_panel(f, &frame.active_panel, main_area, &mut scroll.panel);
        } else {
            Self::render_conversation(f, &frame.conversation, main_area, &mut scroll.conversation);
        }

        // Render status bar (reuse existing adapter)
        render_status_bar::render_status_bar_from_ir(f, &frame.status_bar, status_area);
    }

    /// Render the active side panel with auto-scroll.
    fn render_panel(f: &mut ratatui::Frame<'_>, panel: &cp_render::frame::PanelContent, area: Rect, scroll: &mut f32) {
        let base_style = Style::default().bg(theme::bg_surface());
        let inner = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style)
            .title(Span::styled(format!(" {} ", panel.title), Style::default().fg(theme::accent()).bold()));

        if let Some(ref bottom) = panel.refreshed_ago {
            block = block.title_bottom(Span::styled(format!(" {bottom} "), Style::default().fg(theme::text_muted())));
        }

        let content_area = block.inner(inner);
        f.render_widget(block, inner);

        let lines: Vec<Line<'static>> =
            if panel.blocks.is_empty() { Vec::new() } else { blocks_to_lines(&panel.blocks) };

        Self::render_scrolled(f, content_area, lines, scroll, base_style);
    }

    /// Render the conversation area with auto-scroll.
    fn render_conversation(
        f: &mut ratatui::Frame<'_>,
        conversation: &cp_render::conversation::Conversation,
        area: Rect,
        scroll: &mut f32,
    ) {
        let base_style = Style::default().bg(theme::bg_surface());
        let inner = Rect::new(area.x.saturating_add(1), area.y, area.width.saturating_sub(2), area.height);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style);

        let content_area = block.inner(inner);
        f.render_widget(block, inner);

        // Build lines from conversation messages + input area
        let mut lines = Vec::new();
        for msg in &conversation.messages {
            let role_style = semantic_to_style(Semantic::Accent);
            lines.push(Line::from(Span::styled(msg.role.clone(), role_style)));
            lines.extend(blocks_to_lines(&msg.content));
            lines.push(Line::from(""));
        }

        // Render input area separator + content
        lines.push(Line::from(Span::styled(
            "─".repeat(content_area.width.to_usize()),
            semantic_to_style(Semantic::Border),
        )));
        let input_text =
            if conversation.input.text.is_empty() { &conversation.input.placeholder } else { &conversation.input.text };
        let input_semantic = if conversation.input.text.is_empty() { Semantic::Muted } else { Semantic::Default };
        lines.push(Line::from(Span::styled(input_text.clone(), semantic_to_style(input_semantic))));

        Self::render_scrolled(f, content_area, lines, scroll, base_style);
    }

    /// Render lines into an area with auto-scroll-to-bottom.
    fn render_scrolled(
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        lines: Vec<Line<'static>>,
        scroll: &mut f32,
        base_style: Style,
    ) {
        use crate::ui::helpers::count_wrapped_lines;

        let viewport_height = area.height.to_usize();
        let viewport_width = area.width.to_usize();
        let content_height: usize = lines.iter().map(|l| count_wrapped_lines(l, viewport_width)).sum();

        // Auto-scroll to bottom
        let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
        *scroll = max_scroll;

        let paragraph =
            Paragraph::new(lines).style(base_style).wrap(Wrap { trim: false }).scroll((scroll.round().to_u16(), 0));

        f.render_widget(paragraph, area);
    }
}
