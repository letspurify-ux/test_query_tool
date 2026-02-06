pub mod connection_dialog;
pub mod find_replace;
pub mod intellisense;
pub mod main_window;
pub mod menu;
pub mod object_browser;
pub mod query_history;
pub mod result_table;
pub mod result_tabs;
pub mod sql_editor;
pub mod syntax_highlight;
pub mod theme;

use fltk::{app, prelude::WindowExt, window::Window};

pub use connection_dialog::*;
pub use find_replace::*;
pub use intellisense::*;
pub use main_window::*;
pub use menu::*;
pub use object_browser::*;
pub use query_history::*;
pub use result_table::*;
pub use result_tabs::*;
pub use sql_editor::*;
pub use syntax_highlight::*;

pub fn center_on_main(window: &mut Window) {
    if let Some(main) = app::first_window() {
        window.center_of(&main);
    } else {
        window.center_screen();
    }
}
