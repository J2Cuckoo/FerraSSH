#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use eframe::egui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("ferrassh=info,ferra_core=info")
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0])
            .with_decorations(false)
            .with_title("FerraSSH")
            .with_app_id("com.ferra.ssh"),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "FerraSSH",
        options,
        Box::new(|cc| Ok(Box::new(app::FerraApp::new(cc)))),
    )
}
