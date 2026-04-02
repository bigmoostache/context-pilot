//! Core application struct and egui integration.

use eframe::egui;

/// Main application state for the egui frontend.
#[derive(Debug)]
pub struct App {
    /// Placeholder label displayed in the central panel.
    title: String,
}

impl Default for App {
    fn default() -> Self {
        Self { title: String::from("Context Pilot") }
    }
}

impl App {
    /// Create a new application instance with default state.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_visuals(&cc.egui_ctx);
        Self::default()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        drop(egui::CentralPanel::default().show(ctx, |ui| {
            drop(ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                drop(ui.heading(&self.title));
                ui.add_space(16.0);
                drop(ui.label("egui frontend — scaffold complete"));
            }));
        }));
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

/// Apply dark-mode visuals and default font configuration.
fn configure_visuals(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
}
