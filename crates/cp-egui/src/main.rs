//! Entry point for the Context Pilot egui frontend.

use cp_render as _;

/// Run the native desktop application.
///
/// # Errors
///
/// Returns an error if the native window fails to initialize or the
/// event loop encounters an unrecoverable fault.
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title("Context Pilot"),
        ..eframe::NativeOptions::default()
    };

    eframe::run_native("Context Pilot", options, Box::new(|cc| Ok(Box::new(cp_egui::app::App::new(cc)))))
}
