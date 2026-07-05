#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod curseforge;
mod hashing;
mod local_mods;
mod models;
mod modrinth;
mod progress;
mod sftp;
mod state;
mod sync;
mod updater;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([900.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Minecraft Mod Updater",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
