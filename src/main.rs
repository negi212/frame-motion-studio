#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod composite;
mod video;

use app::FrameMotionApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([980.0, 640.0])
            .with_title("FRAME MOTION STUDIO")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "FRAME MOTION STUDIO",
        options,
        Box::new(|cc| Ok(Box::new(FrameMotionApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    // Try to load embedded icon or fallback to default
    // For now, no icon file – return default (None would be handled by eframe, but we must return something)
    // Create a simple 32x32 transparent icon
    egui::IconData::default()
}
