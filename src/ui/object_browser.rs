use fltk::{
    enums::{Color, Event, Key},
    prelude::*,
    tree::{Tree, TreeItem, TreeSelect},
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::{ObjectBrowser, SharedConnection};

/// Callback type for executing SQL from object browser
pub type SqlExecuteCallback = Rc<RefCell<Option<Box<dyn FnMut(String)>>>>;

#[derive(Clone)]
pub struct ObjectBrowserWidget {
    tree: Tree,
    connection: SharedConnection,
    sql_callback: SqlExecuteCallback,
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

        let sql_callback: SqlExecuteCallback = Rc::new(RefCell::new(None));

        let mut widget = Self {
            tree,
            connection,
            sql_callback,
        };
        widget.setup_callbacks();
        widget
    }

    fn setup_callbacks(&mut self) {
        let connection = self.connection.clone();
        let sql_callback = self.sql_callback.clone();

        self.tree.handle(move |t, ev| {
            match ev {
                Event::Push => {
                    // Right-click for context menu
                    if fltk::app::event_mouse_button() == fltk::app::MouseButton::Right {
                        if let Some(item) = t.first_selected_item() {
                            Self::show_context_menu(&connection, &item, &sql_callback);
                        }
                        return true;
                    }
                    false
                }
                Event::KeyUp => {
                    // Enter key to generate SELECT
                    if fltk::app::event_key() == Key::Enter {
                        if let Some(item) = t.first_selected_item() {
                            if let Some((parent_type, object_name)) = Self::get_item_info(&item) {
                                if parent_type == "Tables" || parent_type == "Views" {
                                    let sql = format!(
                                        "SELECT * FROM {} WHERE ROWNUM <= 100",
                                        object_name
                                    );
                                    if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                                        cb(sql);
                                    }
                                }
                            }
                        }
                        return true;
                    }
                    false
                }
                _ => false,
            }
        });

        // Double-click callback using set_callback
        let sql_callback_dbl = self.sql_callback.clone();
        self.tree.set_callback(move |t| {
            if let Some(item) = t.callback_item() {
                if let Some((parent_type, object_name)) = Self::get_item_info(&item) {
                    if parent_type == "Tables" || parent_type == "Views" {
                        let sql = format!("SELECT * FROM {} WHERE ROWNUM <= 100", object_name);
                        if let Some(ref mut cb) = *sql_callback_dbl.borrow_mut() {
                            cb(sql);
                        }
                    }
                }
            }
        });
    }

    fn get_item_info(item: &TreeItem) -> Option<(String, String)> {
        let object_name = item.label()?;
        let parent = item.parent()?;
        let parent_type = parent.label()?;

        // Make sure this is not a category item
        if parent_type == "Tables"
            || parent_type == "Views"
            || parent_type == "Procedures"
            || parent_type == "Functions"
            || parent_type == "Sequences"
        {
            Some((parent_type, object_name))
        } else {
            None
        }
    }

    fn show_context_menu(
        connection: &SharedConnection,
        item: &TreeItem,
        sql_callback: &SqlExecuteCallback,
    ) {
        if let Some((parent_type, object_name)) = Self::get_item_info(item) {
            let mut menu = fltk::menu::MenuButton::default();
            menu.set_color(Color::from_rgb(45, 45, 48));
            menu.set_text_color(Color::White);

            match parent_type.as_str() {
                "Tables" => {
                    menu.add_choice("Select Data (Top 100)|View Structure|View Indexes|View Constraints|Generate DDL");
                }
                "Views" => {
                    menu.add_choice("Select Data (Top 100)|Generate DDL");
                }
                "Procedures" | "Functions" | "Sequences" => {
                    menu.add_choice("Generate DDL");
                }
                _ => return,
            }

            if let Some(choice_item) = menu.popup() {
                let choice_label = choice_item.label().unwrap_or_default();

                let conn_guard = connection.lock().unwrap();
                if !conn_guard.is_connected() {
                    fltk::dialog::alert_default("Not connected to database");
                    return;
                }

                if let Some(db_conn) = conn_guard.get_connection() {
                    match choice_label.as_str() {
                        "Select Data (Top 100)" => {
                            let sql = format!(
                                "SELECT * FROM {} WHERE ROWNUM <= 100",
                                object_name
                            );
                            drop(conn_guard);
                            if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                                cb(sql);
                            }
                        }
                        "View Structure" => {
                            Self::show_table_structure(db_conn, &object_name);
                        }
                        "View Indexes" => {
                            Self::show_table_indexes(db_conn, &object_name);
                        }
                        "View Constraints" => {
                            Self::show_table_constraints(db_conn, &object_name);
                        }
                        "Generate DDL" => {
                            let obj_type = match parent_type.as_str() {
                                "Tables" => "TABLE",
                                "Views" => "VIEW",
                                "Procedures" => "PROCEDURE",
                                "Functions" => "FUNCTION",
                                "Sequences" => "SEQUENCE",
                                _ => return,
                            };
                            Self::show_ddl(db_conn, obj_type, &object_name, sql_callback);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn show_table_structure(conn: &oracle::Connection, table_name: &str) {
        match ObjectBrowser::get_table_structure(conn, table_name) {
            Ok(columns) => {
                let mut info = format!("=== Table Structure: {} ===\n\n", table_name);
                info.push_str(&format!(
                    "{:<30} {:<20} {:<10} {:<10}\n",
                    "Column Name", "Data Type", "Nullable", "PK"
                ));
                info.push_str(&format!("{}\n", "-".repeat(70)));

                for col in columns {
                    info.push_str(&format!(
                        "{:<30} {:<20} {:<10} {:<10}\n",
                        col.name,
                        col.get_type_display(),
                        if col.nullable { "YES" } else { "NO" },
                        if col.is_primary_key { "PK" } else { "" }
                    ));
                }

                Self::show_info_dialog("Table Structure", &info);
            }
            Err(e) => {
                fltk::dialog::alert_default(&format!("Failed to get table structure: {}", e));
            }
        }
    }

    fn show_table_indexes(conn: &oracle::Connection, table_name: &str) {
        match ObjectBrowser::get_table_indexes(conn, table_name) {
            Ok(indexes) => {
                let mut info = format!("=== Indexes: {} ===\n\n", table_name);
                info.push_str(&format!(
                    "{:<30} {:<10} {:<40}\n",
                    "Index Name", "Unique", "Columns"
                ));
                info.push_str(&format!("{}\n", "-".repeat(80)));

                for idx in indexes {
                    info.push_str(&format!(
                        "{:<30} {:<10} {:<40}\n",
                        idx.name,
                        if idx.is_unique { "YES" } else { "NO" },
                        idx.columns
                    ));
                }

                Self::show_info_dialog("Table Indexes", &info);
            }
            Err(e) => {
                fltk::dialog::alert_default(&format!("Failed to get indexes: {}", e));
            }
        }
    }

    fn show_table_constraints(conn: &oracle::Connection, table_name: &str) {
        match ObjectBrowser::get_table_constraints(conn, table_name) {
            Ok(constraints) => {
                let mut info = format!("=== Constraints: {} ===\n\n", table_name);
                info.push_str(&format!(
                    "{:<30} {:<15} {:<25} {:<20}\n",
                    "Constraint Name", "Type", "Columns", "Ref Table"
                ));
                info.push_str(&format!("{}\n", "-".repeat(90)));

                for con in constraints {
                    info.push_str(&format!(
                        "{:<30} {:<15} {:<25} {:<20}\n",
                        con.name,
                        con.constraint_type,
                        con.columns,
                        con.ref_table.unwrap_or_default()
                    ));
                }

                Self::show_info_dialog("Table Constraints", &info);
            }
            Err(e) => {
                fltk::dialog::alert_default(&format!("Failed to get constraints: {}", e));
            }
        }
    }

    fn show_ddl(
        conn: &oracle::Connection,
        object_type: &str,
        object_name: &str,
        sql_callback: &SqlExecuteCallback,
    ) {
        let result = match object_type {
            "TABLE" => ObjectBrowser::get_table_ddl(conn, object_name),
            "VIEW" => ObjectBrowser::get_view_ddl(conn, object_name),
            "PROCEDURE" => ObjectBrowser::get_procedure_ddl(conn, object_name),
            "FUNCTION" => ObjectBrowser::get_function_ddl(conn, object_name),
            "SEQUENCE" => ObjectBrowser::get_sequence_ddl(conn, object_name),
            _ => return,
        };

        match result {
            Ok(ddl) => {
                // Put DDL in editor
                if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                    cb(ddl);
                }
            }
            Err(e) => {
                fltk::dialog::alert_default(&format!("Failed to generate DDL: {}", e));
            }
        }
    }

    fn show_info_dialog(title: &str, content: &str) {
        use fltk::{prelude::*, text::TextDisplay, window::Window};

        let mut dialog = Window::default()
            .with_size(700, 500)
            .with_label(title);
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut display = TextDisplay::default()
            .with_pos(10, 10)
            .with_size(680, 440);
        display.set_color(Color::from_rgb(30, 30, 30));
        display.set_text_color(Color::from_rgb(220, 220, 220));
        display.set_text_font(fltk::enums::Font::Courier);
        display.set_text_size(12);

        let mut buffer = fltk::text::TextBuffer::default();
        buffer.set_text(content);
        display.set_buffer(buffer);

        let mut close_btn = fltk::button::Button::default()
            .with_pos(300, 460)
            .with_size(100, 30)
            .with_label("Close");
        close_btn.set_color(Color::from_rgb(0, 122, 204));
        close_btn.set_label_color(Color::White);

        let mut dialog_clone = dialog.clone();
        close_btn.set_callback(move |_| {
            dialog_clone.hide();
        });

        dialog.end();
        dialog.show();

        while dialog.shown() {
            fltk::app::wait();
        }
    }

    pub fn set_sql_callback<F>(&mut self, callback: F)
    where
        F: FnMut(String) + 'static,
    {
        *self.sql_callback.borrow_mut() = Some(Box::new(callback));
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
