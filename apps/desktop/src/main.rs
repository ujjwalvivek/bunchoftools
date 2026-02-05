#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod assets;
mod settings;
mod theme;
mod views;

fn main() {
    app::run();
}
