#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod db;
mod ui;
mod utils;

use app::App;

fn main() {
    let app = App::new();
    app.run();
}
