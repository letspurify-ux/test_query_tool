use crate::ui::MainWindow;
use crate::utils::AppConfig;

pub struct App {
    config: AppConfig,
}

impl App {
    pub fn new() -> Self {
        let config = AppConfig::load();
        Self { config }
    }

    pub fn run(&self) {
        MainWindow::run();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
