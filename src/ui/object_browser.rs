use fltk::{
    app,
    enums::{Color, Event, Key},
    group::{Flex, FlexType},
    input::Input,
    prelude::*,
    tree::{Tree, TreeItem, TreeSelect},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;

use crate::db::{lock_connection, ObjectBrowser, SharedConnection};

#[derive(Clone)]
pub enum SqlAction {
    Set(String),
    Insert(String),
}

/// Callback type for executing SQL from object browser
pub type SqlExecuteCallback = Rc<RefCell<Option<Box<dyn FnMut(SqlAction)>>>>;

#[derive(Clone)]
enum ObjectItem {
    Simple {
        object_type: String,
        object_name: String,
    },
    PackageProcedure {
        package_name: String,
        procedure_name: String,
    },
}

/// Stores original object lists for filtering
#[derive(Clone, Default)]
struct ObjectCache {
    tables: Vec<String>,
    views: Vec<String>,
    procedures: Vec<String>,
    functions: Vec<String>,
    sequences: Vec<String>,
    packages: Vec<String>,
    package_procedures: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
pub struct ObjectBrowserWidget {
    flex: Flex,
    tree: Tree,
    connection: SharedConnection,
    sql_callback: SqlExecuteCallback,
    filter_input: Input,
    object_cache: Rc<RefCell<ObjectCache>>,
    refresh_sender: app::Sender<ObjectCache>,
}

impl ObjectBrowserWidget {
    pub fn new(x: i32, y: i32, w: i32, h: i32, connection: SharedConnection) -> Self {
        // Create a flex container for the filter input and tree
        let mut flex = Flex::default().with_pos(x, y).with_size(w, h);
        flex.set_type(FlexType::Column);
        flex.set_spacing(2);

        // Filter input with modern styling
        let mut filter_input = Input::default();
        filter_input.set_color(Color::from_rgb(45, 45, 48)); // Modern input background
        filter_input.set_text_color(Color::from_rgb(212, 212, 212));
        filter_input.set_tooltip("Type to filter objects...");
        flex.fixed(&filter_input, 28);

        // Tree view with modern styling
        let mut tree = Tree::default();

        tree.set_color(Color::from_rgb(37, 37, 38)); // Modern tree background
        tree.set_selection_color(Color::from_rgb(38, 79, 120)); // Selection
        tree.set_item_label_fgcolor(Color::from_rgb(200, 200, 200)); // Item text
        tree.set_connector_color(Color::from_rgb(80, 80, 80)); // Subtle connectors
        tree.set_select_mode(TreeSelect::Single);

        // Initialize tree structure
        tree.set_show_root(false);
        tree.add("Tables");
        tree.add("Views");
        tree.add("Procedures");
        tree.add("Functions");
        tree.add("Sequences");
        tree.add("Packages");

        // Make tree resizable (takes remaining space after filter input)
        flex.resizable(&tree);
        flex.end();

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
        if let Some(mut item) = tree.find_item("Packages") {
            item.close();
        }

        let sql_callback: SqlExecuteCallback = Rc::new(RefCell::new(None));
        let object_cache = Rc::new(RefCell::new(ObjectCache::default()));

        let (refresh_sender, refresh_receiver) = app::channel::<ObjectCache>();

        let mut widget = Self {
            flex,
            tree,
            connection,
            filter_input,
            object_cache,
            sql_callback,
            refresh_sender,
        };
        widget.setup_callbacks();
        widget.setup_filter_callback();
        widget.setup_refresh_handler(refresh_receiver);
        widget
    }

    pub fn get_widget(&self) -> Flex {
        self.flex.clone()
    }

    fn setup_filter_callback(&mut self) {
        let mut tree = self.tree.clone();
        let object_cache = self.object_cache.clone();

        self.filter_input.set_callback(move |input| {
            let filter_text = input.value().to_lowercase();
            let cache = object_cache.borrow();
            ObjectBrowserWidget::populate_tree(&mut tree, &cache, &filter_text);
            tree.redraw();
        });
    }

    fn setup_refresh_handler(&mut self, refresh_receiver: app::Receiver<ObjectCache>) {
        let mut tree = self.tree.clone();
        let object_cache = self.object_cache.clone();
        let filter_input = self.filter_input.clone();

        app::add_idle3(move |_| {
            while let Some(cache) = refresh_receiver.recv() {
                *object_cache.borrow_mut() = cache.clone();
                let filter_text = filter_input.value().to_lowercase();
                ObjectBrowserWidget::populate_tree(&mut tree, &cache, &filter_text);
                tree.redraw();
            }
        });
    }

    fn setup_callbacks(&mut self) {
        let connection = self.connection.clone();
        let sql_callback = self.sql_callback.clone();

        self.tree.handle(move |t, ev| {
            match ev {
                Event::Push => {
                    let mouse_button = fltk::app::event_mouse_button();
                    if mouse_button == fltk::app::MouseButton::Right {
                        if let Some(item) = t.first_selected_item() {
                            Self::show_context_menu(&connection, &item, &sql_callback);
                        }
                        return true;
                    }

                    if mouse_button == fltk::app::MouseButton::Left && fltk::app::event_clicks() {
                        if let Some(item) = t.first_selected_item() {
                            if let Some(insert_text) = Self::get_insert_text(&item) {
                                if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                                    cb(SqlAction::Insert(insert_text));
                                }
                                return true;
                            }
                        }
                    }

                    false
                }
                Event::KeyUp => {
                    // Enter key to generate SELECT
                    if fltk::app::event_key() == Key::Enter {
                        if let Some(item) = t.first_selected_item() {
                            if let Some(ObjectItem::Simple {
                                object_type,
                                object_name,
                            }) = Self::get_item_info(&item)
                            {
                                if object_type == "Tables" || object_type == "Views" {
                                    let sql = format!(
                                        "SELECT * FROM {} WHERE ROWNUM <= 100",
                                        object_name
                                    );
                                    if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                                        cb(SqlAction::Set(sql));
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
    }

    fn get_item_info(item: &TreeItem) -> Option<ObjectItem> {
        let object_name = item.label()?;
        let parent = item.parent()?;
        let parent_type = parent.label()?;

        // Make sure this is not a category item
        if parent_type == "Procedures" {
            if let Some(grandparent) = parent.parent() {
                let package_name = grandparent.label()?;
                let root = grandparent.parent()?;
                if root.label()? == "Packages" {
                    return Some(ObjectItem::PackageProcedure {
                        package_name,
                        procedure_name: object_name,
                    });
                }
            }
        }

        match parent_type.as_str() {
            "Tables" | "Views" | "Procedures" | "Functions" | "Sequences" | "Packages" => {
                Some(ObjectItem::Simple {
                    object_type: parent_type,
                    object_name,
                })
            }
            _ => None,
        }
    }

    fn get_insert_text(item: &TreeItem) -> Option<String> {
        match Self::get_item_info(item)? {
            ObjectItem::Simple { object_name, .. } => Some(object_name),
            ObjectItem::PackageProcedure {
                package_name,
                procedure_name,
            } => Some(format!("{}.{}", package_name, procedure_name)),
        }
    }

    fn show_context_menu(
        connection: &SharedConnection,
        item: &TreeItem,
        sql_callback: &SqlExecuteCallback,
    ) {
        if let Some(item_info) = Self::get_item_info(item) {
            // Get mouse position for proper popup placement
            let mouse_x = fltk::app::event_x();
            let mouse_y = fltk::app::event_y();

            let mut menu = fltk::menu::MenuButton::new(mouse_x, mouse_y, 0, 0, None);
            menu.set_color(Color::from_rgb(45, 45, 48));
            menu.set_text_color(Color::White);

            match &item_info {
                ObjectItem::Simple { object_type, .. } if object_type == "Tables" => {
                    menu.add_choice("Select Data (Top 100)|View Structure|View Indexes|View Constraints|Generate DDL");
                }
                ObjectItem::Simple { object_type, .. } if object_type == "Views" => {
                    menu.add_choice("Select Data (Top 100)|Generate DDL");
                }
                ObjectItem::Simple { object_type, .. }
                    if object_type == "Procedures"
                        || object_type == "Functions"
                        || object_type == "Sequences" =>
                {
                    menu.add_choice("Generate DDL");
                }
                _ => return,
            }

            if let Some(choice_item) = menu.popup() {
                let choice_label = choice_item.label().unwrap_or_default();

                let conn_guard = lock_connection(&connection);
                if !conn_guard.is_connected() {
                    fltk::dialog::alert_default("Not connected to database");
                    return;
                }

                if let Some(db_conn) = conn_guard.get_connection() {
                    match (choice_label.as_str(), &item_info) {
                        ("Select Data (Top 100)", _) => {
                            let object_name = match &item_info {
                                ObjectItem::Simple { object_name, .. } => object_name,
                                _ => return,
                            };
                            let sql = format!("SELECT * FROM {} WHERE ROWNUM <= 100", object_name);
                            drop(conn_guard);
                            if let Some(ref mut cb) = *sql_callback.borrow_mut() {
                                cb(SqlAction::Set(sql));
                            }
                        }
                        ("View Structure", ObjectItem::Simple { object_name, .. }) => {
                            Self::show_table_structure(db_conn.as_ref(), object_name);
                        }
                        ("View Indexes", ObjectItem::Simple { object_name, .. }) => {
                            Self::show_table_indexes(db_conn.as_ref(), object_name);
                        }
                        ("View Constraints", ObjectItem::Simple { object_name, .. }) => {
                            Self::show_table_constraints(db_conn.as_ref(), object_name);
                        }
                        (
                            "Generate DDL",
                            ObjectItem::Simple {
                                object_type,
                                object_name,
                            },
                        ) => {
                            let obj_type = match object_type.as_str() {
                                "Tables" => "TABLE",
                                "Views" => "VIEW",
                                "Procedures" => "PROCEDURE",
                                "Functions" => "FUNCTION",
                                "Sequences" => "SEQUENCE",
                                _ => return,
                            };
                            Self::show_ddl(db_conn.as_ref(), obj_type, object_name, sql_callback);
                        }
                        _ => {}
                    }
                }
            }

            menu.hide();
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
                    cb(SqlAction::Set(ddl));
                }
            }
            Err(e) => {
                fltk::dialog::alert_default(&format!("Failed to generate DDL: {}", e));
            }
        }
    }

    fn show_info_dialog(title: &str, content: &str) {
        use fltk::{prelude::*, text::TextDisplay, window::Window};

        let mut dialog = Window::default().with_size(700, 500).with_label(title);
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut display = TextDisplay::default().with_pos(10, 10).with_size(680, 440);
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
        F: FnMut(SqlAction) + 'static,
    {
        *self.sql_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn refresh(&mut self) {
        // First clear items and filter
        self.clear_items();
        self.filter_input.set_value("");
        *self.object_cache.borrow_mut() = ObjectCache::default();

        let conn_guard = lock_connection(&self.connection);

        if !conn_guard.is_connected() {
            // Clear cache
            *self.object_cache.borrow_mut() = ObjectCache::default();
            return;
        }

        drop(conn_guard);

        let sender = self.refresh_sender.clone();
        let connection = self.connection.clone();

        thread::spawn(move || {
            let conn_guard = lock_connection(&connection);
            if !conn_guard.is_connected() {
                return;
            }

            let Some(db_conn) = conn_guard.get_connection() else {
                return;
            };
            drop(conn_guard);

            let mut cache = ObjectCache::default();

            if let Ok(tables) = ObjectBrowser::get_tables(db_conn.as_ref()) {
                cache.tables = tables;
            }

            if let Ok(views) = ObjectBrowser::get_views(db_conn.as_ref()) {
                cache.views = views;
            }

            if let Ok(procedures) = ObjectBrowser::get_procedures(db_conn.as_ref()) {
                cache.procedures = procedures;
            }

            if let Ok(functions) = ObjectBrowser::get_functions(db_conn.as_ref()) {
                cache.functions = functions;
            }

            if let Ok(sequences) = ObjectBrowser::get_sequences(db_conn.as_ref()) {
                cache.sequences = sequences;
            }

            if let Ok(packages) = ObjectBrowser::get_packages(db_conn.as_ref()) {
                for package in &packages {
                    if let Ok(procs) =
                        ObjectBrowser::get_package_procedures(db_conn.as_ref(), package)
                    {
                        cache.package_procedures.insert(package.clone(), procs);
                    }
                }
                cache.packages = packages;
            }

            let _ = sender.send(cache);
        });
    }

    fn clear_items(&mut self) {
        Self::clear_tree_items(&mut self.tree);
    }

    fn clear_tree_items(tree: &mut Tree) {
        let categories = [
            "Tables",
            "Views",
            "Procedures",
            "Functions",
            "Sequences",
            "Packages",
        ];

        for category in categories {
            if let Some(item) = tree.find_item(category) {
                while item.has_children() {
                    if let Some(child) = item.child(0) {
                        let _ = tree.remove(&child);
                    } else {
                        break;
                    }
                }
            }
        }
    }

    fn populate_tree(tree: &mut Tree, cache: &ObjectCache, filter_text: &str) {
        Self::clear_tree_items(tree);

        for table in &cache.tables {
            if filter_text.is_empty() || table.to_lowercase().contains(filter_text) {
                tree.add(&format!("Tables/{}", table));
            }
        }
        for view in &cache.views {
            if filter_text.is_empty() || view.to_lowercase().contains(filter_text) {
                tree.add(&format!("Views/{}", view));
            }
        }
        for proc in &cache.procedures {
            if filter_text.is_empty() || proc.to_lowercase().contains(filter_text) {
                tree.add(&format!("Procedures/{}", proc));
            }
        }
        for func in &cache.functions {
            if filter_text.is_empty() || func.to_lowercase().contains(filter_text) {
                tree.add(&format!("Functions/{}", func));
            }
        }
        for seq in &cache.sequences {
            if filter_text.is_empty() || seq.to_lowercase().contains(filter_text) {
                tree.add(&format!("Sequences/{}", seq));
            }
        }

        for package in &cache.packages {
            let procedures = cache
                .package_procedures
                .get(package)
                .cloned()
                .unwrap_or_default();
            let package_matches =
                filter_text.is_empty() || package.to_lowercase().contains(filter_text);
            let matching_procs: Vec<String> = procedures
                .into_iter()
                .filter(|proc_name| {
                    filter_text.is_empty()
                        || proc_name.to_lowercase().contains(filter_text)
                        || package_matches
                })
                .collect();

            if package_matches || !matching_procs.is_empty() {
                tree.add(&format!("Packages/{}", package));
                for proc_name in matching_procs {
                    tree.add(&format!("Packages/{}/Procedures/{}", package, proc_name));
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_selected_item(&self) -> Option<String> {
        self.tree
            .first_selected_item()
            .and_then(|item| item.label())
    }
}
