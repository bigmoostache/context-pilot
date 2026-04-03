//! Core application struct and egui integration.

use eframe::egui;

use crate::demo::build_demo_frame;
use crate::input::{self, AppAction, State as InputState};
use crate::layout::render_frame;

/// Main application state for the egui frontend.
#[derive(Debug, Default)]
pub struct App {
    /// Interactive input state (text buffer, history, focus).
    input: InputState,
    /// Current sidebar mode index (0=Normal, 1=Collapsed, 2=Hidden).
    sidebar_mode: u8,
}

impl App {
    /// Create a new application instance with default state.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_visuals(&cc.egui_ctx);
        Self::default()
    }

    /// Process application-level actions from keyboard shortcuts.
    fn handle_actions(&mut self, actions: &[AppAction]) {
        for action in actions {
            match action {
                AppAction::Submit => {
                    if let Some(msg) = self.input.submit() {
                        // Phase 6 will wire this to the LLM pipeline.
                        drop(msg);
                    }
                }
                AppAction::ClearInput => self.input.clear(),
                AppAction::HistoryBack => self.input.history_back(),
                AppAction::HistoryForward => self.input.history_forward(),
                AppAction::ToggleSidebar => {
                    self.sidebar_mode = match self.sidebar_mode {
                        0 => 1,
                        1 => 2,
                        _ => 0,
                    };
                }
                AppAction::NextPanel | AppAction::PreviousPanel | AppAction::JumpToPanel(_) | AppAction::ToggleHelp => {
                    // Phase 6 will wire panel switching and help overlay.
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll keyboard shortcuts.
        let actions = input::poll_actions(ctx);
        self.handle_actions(&actions);

        // Build the demo frame (Phase 6 replaces with live State).
        let mut frame = build_demo_frame();

        // Override sidebar mode from our toggle state.
        frame.sidebar.mode = match self.sidebar_mode {
            1 => cp_render::frame::SidebarMode::Collapsed,
            2 => cp_render::frame::SidebarMode::Hidden,
            _ => cp_render::frame::SidebarMode::Normal,
        };

        // Sync input text into the frame's conversation input area.
        frame.conversation.input.text.clone_from(&self.input.text);

        // Render the full frame layout.
        render_frame(ctx, &frame);

        // Input area — rendered as a bottom panel so it stays fixed.
        render_input_panel(ctx, &mut self.input);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn persist_egui_memory(&self) -> bool {
        true
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw_input: &mut egui::RawInput) {}
}

/// Render the interactive input area as a bottom panel.
fn render_input_panel(ctx: &egui::Context, input: &mut InputState) {
    drop(egui::TopBottomPanel::bottom("input_panel").exact_height(36.0).show(ctx, |ui| {
        drop(ui.horizontal_centered(|ui| {
            let prompt =
                egui::RichText::new("› ").color(crate::theme::semantic_color(cp_render::Semantic::Accent)).strong();
            drop(ui.label(prompt));

            let response = ui.add(
                egui::TextEdit::singleline(&mut input.text)
                    .desired_width(ui.available_width())
                    .hint_text("Ask me anything, captain...")
                    .font(egui::FontId::proportional(14.0))
                    .text_color(crate::theme::semantic_color(cp_render::Semantic::Default))
                    .frame(false),
            );

            // Auto-focus on first frame.
            if input.request_focus {
                response.request_focus();
                input.request_focus = false;
            }

            // Enter → submit (when TextEdit has focus and Enter pressed).
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(msg) = input.submit() {
                    // Phase 6: send to LLM.
                    drop(msg);
                }
                response.request_focus();
            }
        }));
    }));
}

/// Apply dark-mode visuals and default font configuration.
fn configure_visuals(ctx: &egui::Context) {
    crate::theme::configure_visuals(ctx);
}
