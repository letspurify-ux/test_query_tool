mod app;
mod db;
mod ui;
mod utils;

use app::App;

fn main() {
    let app = App::new();
    app.run();
}
