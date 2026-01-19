use fltk::{
    enums::Color,
    prelude::*,
    tree::{Tree, TreeSelect},
};

use crate::db::{ObjectBrowser, SharedConnection};

#[derive(Clone)]
pub struct ObjectBrowserWidget {
    tree: Tree,
    connection: SharedConnection,
}

impl ObjectBrowserWidget {
    pub fn new(x: i32, y: i32, w: i32, h: i32, connection: SharedConnection) -> Self {
        let mut tree = Tree::default()
            .with_pos(x, y)
            .with_size(w, h);

        tree.set_color(Color::from_rgb(37, 37, 38));
        tree.set_selection_color(Color::from_rgb(38, 79, 120));
        tree.set_item_label_fgcolor(Color::from_rgb(220, 220, 220));
        tree.set_connector_color(Color::from_rgb(100, 100, 100));
        tree.set_select_mode(TreeSelect::Single);

        // Initialize tree structure
        tree.set_show_root(false);
        tree.add("Tables");
        tree.add("Views");
        tree.add("Procedures");
        tree.add("Functions");
        tree.add("Sequences");

        // Close all items by default
        if let Some(mut item) = tree.find_item("Tables") {
            item.close();
        }
        if let Some(mut item) = tree.find_item("Views") {
            item.close();
        }
        if let Some(mut item) = tree.find_item("Procedures") {
            item.close();
        }
        if let Some(mut item) = tree.find_item("Functions") {
            item.close();
        }
        if let Some(mut item) = tree.find_item("Sequences") {
            item.close();
        }

        let mut widget = Self { tree, connection };
        widget.setup_callbacks();
        widget
    }

    fn setup_callbacks(&mut self) {
        self.tree.set_callback(move |t| {
            if let Some(item) = t.callback_item() {
                let path = item.label().unwrap_or_default();

                // Check if this is a table item (double-click to generate SELECT)
                if let Some(parent) = item.parent() {
                    if let Some(parent_label) = parent.label() {
                        if parent_label == "Tables" {
                            // Generate SELECT statement for the table
                            let sql = format!("SELECT * FROM {} WHERE ROWNUM <= 100;", path);
                            // Show in message dialog for now
                            fltk::dialog::message_default(&format!("SQL:\n{}", sql));
                        }
                    }
                }
            }
        });
    }

    pub fn refresh(&mut self) {
        // First clear items
        self.clear_items();

        let conn_guard = self.connection.lock().unwrap();

        if !conn_guard.is_connected() {
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            // Load tables
            if let Ok(tables) = ObjectBrowser::get_tables(db_conn) {
                for table in tables {
                    self.tree.add(&format!("Tables/{}", table));
                }
            }

            // Load views
            if let Ok(views) = ObjectBrowser::get_views(db_conn) {
                for view in views {
                    self.tree.add(&format!("Views/{}", view));
                }
            }

            // Load procedures
            if let Ok(procedures) = ObjectBrowser::get_procedures(db_conn) {
                for proc in procedures {
                    self.tree.add(&format!("Procedures/{}", proc));
                }
            }

            // Load functions
            if let Ok(functions) = ObjectBrowser::get_functions(db_conn) {
                for func in functions {
                    self.tree.add(&format!("Functions/{}", func));
                }
            }

            // Load sequences
            if let Ok(sequences) = ObjectBrowser::get_sequences(db_conn) {
                for seq in sequences {
                    self.tree.add(&format!("Sequences/{}", seq));
                }
            }
        }

        drop(conn_guard);
        self.tree.redraw();
    }

    fn clear_items(&mut self) {
        // Remove all children from each category
        let categories = vec!["Tables", "Views", "Procedures", "Functions", "Sequences"];

        for category in categories {
            if let Some(item) = self.tree.find_item(category) {
                // Remove all children
                while item.has_children() {
                    if let Some(child) = item.child(0) {
                        let _ = self.tree.remove(&child);
                    } else {
                        break;
                    }
                }
            }
        }
    }

    pub fn get_selected_item(&self) -> Option<String> {
        self.tree.first_selected_item().and_then(|item| item.label())
    }
}
