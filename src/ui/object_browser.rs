use fltk::{
    app,
    enums::{Event, Key},
    group::{Flex, FlexType},
    input::Input,
    prelude::*,
    tree::{Tree, TreeItem, TreeSelect},
    widget::Widget,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;

use crate::db::{lock_connection, ObjectBrowser, SharedConnection};
use crate::ui::theme;

#[derive(Clone)]
pub enum SqlAction {
    Set(String),
    Insert(String),
    Execute(String),
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
    refresh_sender: std::sync::mpsc::Sender<ObjectCache>,
}

impl ObjectBrowserWidget {
    pub fn new(x: i32, y: i32, w: i32, h: i32, connection: SharedConnection) -> Self {
        // Create a flex container for the filter input and tree
        let mut flex = Flex::default().with_pos(x, y).with_size(w, h);
        flex.set_type(FlexType::Column);
        flex.set_spacing(5);

        // Filter input with modern styling
        let mut filter_input = Input::default();
        filter_input.set_color(theme::input_bg());
        filter_input.set_text_color(theme::text_primary());
        filter_input.set_tooltip("Type to filter objects...");
        flex.fixed(&filter_input, 28);

        // Tree view with modern styling
        let mut tree = Tree::default();

        tree.set_color(theme::panel_bg());
        tree.set_selection_color(theme::selection_soft());
        tree.set_item_label_fgcolor(theme::text_secondary());
        tree.set_connector_color(theme::tree_connector());
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

        let (refresh_sender, refresh_receiver) = std::sync::mpsc::channel::<ObjectCache>();

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

    fn setup_refresh_handler(&mut self, refresh_receiver: std::sync::mpsc::Receiver<ObjectCache>) {
        let tree = self.tree.clone();
        let object_cache = self.object_cache.clone();
        let filter_input = self.filter_input.clone();

        // Wrap receiver in Rc<RefCell> to share across timeout callbacks
        let receiver: Rc<RefCell<std::sync::mpsc::Receiver<ObjectCache>>> =
            Rc::new(RefCell::new(refresh_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<std::sync::mpsc::Receiver<ObjectCache>>>,
            mut tree: Tree,
            object_cache: Rc<RefCell<ObjectCache>>,
            filter_input: Input,
        ) {
            let mut disconnected = false;
            // Process any pending messages
            {
                let r = receiver.borrow();
                loop {
                    match r.try_recv() {
                        Ok(cache) => {
                            *object_cache.borrow_mut() = cache.clone();
                            let filter_text = filter_input.value().to_lowercase();
                            ObjectBrowserWidget::populate_tree(&mut tree, &cache, &filter_text);
                            tree.redraw();
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
            }

            if disconnected {
                return;
            }

            // Reschedule for next poll
            app::add_timeout3(0.05, move |_| {
                schedule_poll(
                    Rc::clone(&receiver),
                    tree.clone(),
                    Rc::clone(&object_cache),
                    filter_input.clone(),
                );
            });
        }

        // Start polling
        schedule_poll(receiver, tree, object_cache, filter_input);
    }

    fn setup_callbacks(&mut self) {
        let connection = self.connection.clone();
        let sql_callback = self.sql_callback.clone();

        self.tree.handle(move |t, ev| {
            if !t.active() {
                return false;
            }
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
                                // Take the callback out, call it, then put it back
                                // This ensures the RefCell is not borrowed during callback execution
                                let cb_opt = sql_callback.borrow_mut().take();
                                if let Some(mut cb) = cb_opt {
                                    cb(SqlAction::Insert(insert_text));
                                    *sql_callback.borrow_mut() = Some(cb);
                                }
                                return true;
                            }
                        }
                    }

                    false
                }
                Event::KeyUp => {
                    // Enter key to generate SELECT - only if tree has focus
                    if fltk::app::event_key() == Key::Enter && t.has_focus() {
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
                                    // Take the callback out, call it, then put it back
                                    let cb_opt = sql_callback.borrow_mut().take();
                                    if let Some(mut cb) = cb_opt {
                                        cb(SqlAction::Set(sql));
                                        *sql_callback.borrow_mut() = Some(cb);
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
        let object_name = match item.label() {
            Some(label) => label,
            None => return None,
        };
        let parent = match item.parent() {
            Some(parent) => parent,
            None => return None,
        };
        let parent_type = match parent.label() {
            Some(label) => label,
            None => return None,
        };

        // Make sure this is not a category item
        if parent_type == "Procedures" {
            if let Some(grandparent) = parent.parent() {
                let package_name = match grandparent.label() {
                    Some(label) => label,
                    None => return None,
                };
                let root = match grandparent.parent() {
                    Some(root) => root,
                    None => return None,
                };
                let root_label = match root.label() {
                    Some(label) => label,
                    None => return None,
                };
                if root_label == "Packages" {
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
        match Self::get_item_info(item) {
            Some(ObjectItem::Simple { object_name, .. }) => Some(object_name),
            Some(ObjectItem::PackageProcedure {
                package_name,
                procedure_name,
            }) => Some(format!("{}.{}", package_name, procedure_name)),
            None => None,
        }
    }

    fn show_context_menu(
        connection: &SharedConnection,
        item: &TreeItem,
        sql_callback: &SqlExecuteCallback,
    ) {
        if let Some(item_info) = Self::get_item_info(item) {
            let menu_choices = match &item_info {
                ObjectItem::Simple { object_type, .. } if object_type == "Tables" => {
                    "Select Data (Top 100)|View Structure|View Indexes|View Constraints|Generate DDL"
                }
                ObjectItem::Simple { object_type, .. } if object_type == "Views" => {
                    "Select Data (Top 100)|Generate DDL"
                }
                ObjectItem::Simple { object_type, .. }
                    if object_type == "Procedures"
                        || object_type == "Functions"
                        || object_type == "Sequences" =>
                {
                    "Generate DDL"
                }
                _ => return,
            };

            // Get mouse position for proper popup placement
            let mouse_x = fltk::app::event_x();
            let mouse_y = fltk::app::event_y();

            let mut menu = fltk::menu::MenuButton::new(mouse_x, mouse_y, 0, 0, None);
            menu.set_color(theme::panel_raised());
            menu.set_text_color(theme::text_primary());
            menu.add_choice(menu_choices);

            if let Some(choice_item) = menu.popup() {
                let choice_label = choice_item.label().unwrap_or_default();

                let conn_guard = lock_connection(&connection);
                if !conn_guard.is_connected() {
                    fltk::dialog::alert_default("Not connected to database");
                } else if let Some(db_conn) = conn_guard.get_connection() {
                    match (choice_label.as_str(), &item_info) {
                        ("Select Data (Top 100)", ObjectItem::Simple { object_name, .. }) => {
                            let sql = format!("SELECT * FROM {} WHERE ROWNUM <= 100", object_name);
                            drop(conn_guard);
                            // Take the callback out, call it, then put it back
                            let cb_opt = sql_callback.borrow_mut().take();
                            if let Some(mut cb) = cb_opt {
                                cb(SqlAction::Execute(sql));
                                *sql_callback.borrow_mut() = Some(cb);
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
                                "Tables" => Some("TABLE"),
                                "Views" => Some("VIEW"),
                                "Procedures" => Some("PROCEDURE"),
                                "Functions" => Some("FUNCTION"),
                                "Sequences" => Some("SEQUENCE"),
                                _ => None,
                            };
                            if let Some(obj_type) = obj_type {
                                Self::show_ddl(db_conn.as_ref(), obj_type, object_name, sql_callback);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // FLTK memory management: widgets created without a parent must be deleted.
            unsafe {
                let widget = Widget::from_widget_ptr(menu.as_widget_ptr());
                Widget::delete(widget);
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
                // Take the callback out, call it, then put it back
                let cb_opt = sql_callback.borrow_mut().take();
                if let Some(mut cb) = cb_opt {
                    cb(SqlAction::Set(ddl));
                    *sql_callback.borrow_mut() = Some(cb);
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
            .with_label(title)
            .center_screen();
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);

        let mut display = TextDisplay::default().with_pos(10, 10).with_size(680, 440);
        display.set_color(theme::editor_bg());
        display.set_text_color(theme::text_primary());
        display.set_text_font(fltk::enums::Font::Courier);
        display.set_text_size(12);

        let mut buffer = fltk::text::TextBuffer::default();
        buffer.set_text(content);
        display.set_buffer(buffer);

        let mut close_btn = fltk::button::Button::default()
            .with_pos(300, 460)
            .with_size(100, 20)
            .with_label("Close");
        close_btn.set_color(theme::button_secondary());
        close_btn.set_label_color(theme::text_primary());

        let (sender, receiver) = std::sync::mpsc::channel::<()>();
        close_btn.set_callback(move |_| {
            let _ = sender.send(());
        });

        dialog.end();
        dialog.show();

        while dialog.shown() {
            fltk::app::wait();
            if receiver.try_recv().is_ok() {
                dialog.hide();
            }
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
            app::awake();
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
