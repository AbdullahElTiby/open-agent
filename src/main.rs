// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod app;
mod icons;
mod import;
mod markdown;
mod model;
mod model_icons;
mod providers;
mod resolve;
mod theme;

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result {
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|img| {
            let img = img.into_rgba8();
            let (width, height) = img.dimensions();
            Arc::new(egui::IconData {
                rgba: img.into_raw(),
                width,
                height,
            })
        });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Open Agent")
        .with_inner_size([1320.0, 860.0])
        .with_min_inner_size([860.0, 560.0])
        .with_decorations(false)
        .with_resizable(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    eframe::run_native(
        "Open Agent",
        eframe::NativeOptions {
            viewport,
            centered: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
