// Hide the console window on Windows in release builds (keep it in debug).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod app;
mod assets;
mod audio;
mod auth;
mod download;
mod github;
mod theme;

use app::PhiLauncher;

fn main() -> eframe::Result<()> {
    let assets_dir = assets::assets_dir();

    let mut viewport = egui::ViewportBuilder::default()
        // 16:9 window.
        .with_inner_size([1280.0, 720.0])
        .with_min_inner_size([960.0, 540.0])
        .with_title("Phi Launcher")
        .with_app_id("phi_launcher");

    if let Some(icon) = assets::load_icon(&assets_dir) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Phi Launcher",
        options,
        Box::new(|cc| Ok(Box::new(PhiLauncher::new(cc)))),
    )
}
