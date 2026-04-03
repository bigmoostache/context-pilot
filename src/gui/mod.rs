//! Graphical frontend — egui/eframe integration.
//!
//! Provides [`GuiApp`], an [`eframe::App`] implementation that drives
//! the same engine loop as the TUI.  Every frame:
//!
//! 1. [`App::tick()`] advances engine state (streaming, tools, spine, reverie).
//! 2. [`build_frame()`] converts [`State`] → IR [`Frame`].
//! 3. [`cp_egui::layout::render_frame()`] paints the IR into egui widgets.

use std::sync::mpsc;

use crate::app::App;
use crate::app::actions::Action;
use crate::app::run::lifecycle::EventChannels;
use crate::infra::api::StreamEvent;
use crate::state::cache::CacheUpdate;
use crate::ui::ir::build_frame;

/// Owned channel endpoints for the GUI event loop.
///
/// The TUI path stores these on the stack in `main()` and borrows them
/// into [`EventChannels`].  Here we own them so they live alongside
/// [`GuiApp`] inside eframe.
#[derive(Debug)]
struct Channels {
    /// Send side of the LLM streaming channel.
    tx: mpsc::Sender<StreamEvent>,
    /// Receive side of the LLM streaming channel.
    rx: mpsc::Receiver<StreamEvent>,
    /// Receive side of the background cache-hasher channel.
    cache_rx: mpsc::Receiver<CacheUpdate>,
}

/// Graphical application shell — wraps the engine in an eframe window.
pub(crate) struct GuiApp {
    /// The shared engine (same [`App`] struct the TUI uses).
    app: App,
    /// Owned channel storage (borrowed into [`EventChannels`] each frame).
    ch: Channels,
}

impl GuiApp {
    /// Create a new GUI wrapper from pre-booted state.
    pub(crate) const fn new(
        app: App,
        tx: mpsc::Sender<StreamEvent>,
        rx: mpsc::Receiver<StreamEvent>,
        cache_rx: mpsc::Receiver<CacheUpdate>,
    ) -> Self {
        Self { app, ch: Channels { tx, rx, cache_rx } }
    }

    /// Map egui keyboard shortcuts to engine [`Action`]s and dispatch them.
    fn poll_egui_shortcuts(&mut self, ctx: &eframe::egui::Context) {
        let mut actions: Vec<Action> = Vec::new();

        ctx.input(|input| {
            // Ctrl+B → cycle sidebar mode.
            if input.modifiers.ctrl && input.key_pressed(eframe::egui::Key::B) {
                actions.push(Action::CycleSidebarMode);
            }

            // Tab / Shift+Tab → cycle panels.
            if input.key_pressed(eframe::egui::Key::Tab) && !input.modifiers.ctrl {
                if input.modifiers.shift {
                    actions.push(Action::SelectPrevContext);
                } else {
                    actions.push(Action::SelectNextContext);
                }
            }

            // Escape → stop streaming.
            if input.key_pressed(eframe::egui::Key::Escape) {
                actions.push(Action::StopStreaming);
            }
        });

        for action in actions {
            self.app.handle_action(action, &self.ch.tx);
        }
    }
}

impl eframe::App for GuiApp {
    /// Called every frame (~60 fps) by the eframe event loop.
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // ── Engine tick ─────────────────────────────────────────────
        let channels = EventChannels { tx: &self.ch.tx, rx: &self.ch.rx, cache_rx: &self.ch.cache_rx };
        let tick = self.app.tick(&channels);

        if tick.should_break {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            return;
        }

        // ── Build IR frame from state ───────────────────────────────
        let ir_frame = build_frame(&self.app.state);

        // ── Paint via cp-egui adapters ──────────────────────────────
        let response = cp_egui::layout::render_frame(ctx, &ir_frame);

        // ── Handle interaction events ───────────────────────────────
        // Sidebar click → switch active panel.
        if let Some(idx) = response.sidebar_click
            && idx < self.app.state.context.len()
        {
            self.app.state.selected_context = idx;
            self.app.state.flags.ui.dirty = true;
        }

        // Keyboard shortcuts (Ctrl+B, Tab, etc.).
        self.poll_egui_shortcuts(ctx);

        // Request continuous repainting while streaming so we don't
        // stall on an idle event loop waiting for user input.
        if self.app.state.flags.stream.phase.is_streaming() || self.app.state.flags.ui.dirty {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn persist_egui_memory(&self) -> bool {
        true
    }

    fn raw_input_hook(&mut self, _ctx: &eframe::egui::Context, _raw_input: &mut eframe::egui::RawInput) {}
}
