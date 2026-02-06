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
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::thread;

use crate::db::{
    lock_connection, ConstraintInfo, IndexInfo, ObjectBrowser, ProcedureArgument, SequenceInfo,
    SharedConnection, TableColumnDetail,
};
use crate::ui::theme;

#[derive(Clone)]
pub enum SqlAction {
    Set(String),
    Insert(String),
    Append(String),
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
enum ObjectActionResult {
    TableStructure {
        table_name: String,
        result: Result<Vec<TableColumnDetail>, String>,
    },
    TableIndexes {
        table_name: String,
        result: Result<Vec<IndexInfo>, String>,
    },
    TableConstraints {
        table_name: String,
        result: Result<Vec<ConstraintInfo>, String>,
    },
    SequenceInfo(Result<SequenceInfo, String>),
    Ddl(Result<String, String>),
    ProcedureScript {
        qualified_name: String,
        result: Result<String, String>,
    },
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
    action_sender: std::sync::mpsc::Sender<ObjectActionResult>,
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
        let (action_sender, action_receiver) = std::sync::mpsc::channel::<ObjectActionResult>();

        let mut widget = Self {
            flex,
            tree,
            connection,
            filter_input,
            object_cache,
            sql_callback,
            refresh_sender,
            action_sender,
        };
        widget.setup_callbacks();
        widget.setup_filter_callback();
        widget.setup_refresh_handler(refresh_receiver);
        widget.setup_action_handler(action_receiver);
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

    fn setup_action_handler(
        &mut self,
        action_receiver: std::sync::mpsc::Receiver<ObjectActionResult>,
    ) {
        let sql_callback = self.sql_callback.clone();

        let receiver: Rc<RefCell<std::sync::mpsc::Receiver<ObjectActionResult>>> =
            Rc::new(RefCell::new(action_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<std::sync::mpsc::Receiver<ObjectActionResult>>>,
            sql_callback: SqlExecuteCallback,
        ) {
            let mut disconnected = false;
            loop {
                let message = {
                    let r = receiver.borrow();
                    r.try_recv()
                };

                match message {
                    Ok(action) => match action {
                        ObjectActionResult::TableStructure { table_name, result } => match result
                        {
                            Ok(columns) => {
                                let mut info = format!(
                                    "=== Table Structure: {} ===\n\n",
                                    table_name
                                );
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

                                ObjectBrowserWidget::show_info_dialog("Table Structure", &info);
                            }
                            Err(err) => {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to get table structure: {}",
                                    err
                                ));
                            }
                        },
                        ObjectActionResult::TableIndexes { table_name, result } => match result {
                            Ok(indexes) => {
                                let mut info =
                                    format!("=== Indexes: {} ===\n\n", table_name);
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

                                ObjectBrowserWidget::show_info_dialog("Table Indexes", &info);
                            }
                            Err(err) => {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to get indexes: {}",
                                    err
                                ));
                            }
                        },
                        ObjectActionResult::TableConstraints { table_name, result } => match result
                        {
                            Ok(constraints) => {
                                let mut info =
                                    format!("=== Constraints: {} ===\n\n", table_name);
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

                                ObjectBrowserWidget::show_info_dialog("Table Constraints", &info);
                            }
                            Err(err) => {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to get constraints: {}",
                                    err
                                ));
                            }
                        },
                        ObjectActionResult::SequenceInfo(result) => match result {
                            Ok(info) => {
                                let mut details =
                                    format!("=== Sequence Info: {} ===\n\n", info.name);
                                details.push_str(&format!("{:<18} {}\n", "Min Value", info.min_value));
                                details.push_str(&format!("{:<18} {}\n", "Max Value", info.max_value));
                                details.push_str(&format!(
                                    "{:<18} {}\n",
                                    "Increment By", info.increment_by
                                ));
                                details.push_str(&format!("{:<18} {}\n", "Cycle", info.cycle_flag));
                                details.push_str(&format!("{:<18} {}\n", "Order", info.order_flag));
                                details.push_str(&format!(
                                    "{:<18} {}\n",
                                    "Cache Size", info.cache_size
                                ));
                                details.push_str(&format!(
                                    "{:<18} {}\n",
                                    "Last Number", info.last_number
                                ));
                                details.push_str(
                                    "\nNote: LAST_NUMBER is the next value to be generated.\n",
                                );

                                ObjectBrowserWidget::show_info_dialog("Sequence Info", &details);
                            }
                            Err(err) => {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to get sequence info: {}",
                                    err
                                ));
                            }
                        },
                        ObjectActionResult::Ddl(result) => match result {
                            Ok(ddl) => {
                                let cb_opt = sql_callback.borrow_mut().take();
                                if let Some(mut cb) = cb_opt {
                                    cb(SqlAction::Append(ddl));
                                    *sql_callback.borrow_mut() = Some(cb);
                                }
                            }
                            Err(err) => {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to generate DDL: {}",
                                    err
                                ));
                            }
                        },
                        ObjectActionResult::ProcedureScript {
                            qualified_name,
                            result,
                        } => {
                            let sql = match result {
                                Ok(sql) => sql,
                                Err(err) => {
                                    fltk::dialog::alert_default(&format!(
                                        "Failed to load procedure arguments: {}",
                                        err
                                    ));
                                    ObjectBrowserWidget::build_simple_procedure_script(
                                        &qualified_name,
                                    )
                                }
                            };
                            let cb_opt = sql_callback.borrow_mut().take();
                            if let Some(mut cb) = cb_opt {
                                cb(SqlAction::Append(sql));
                                *sql_callback.borrow_mut() = Some(cb);
                            }
                        }
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                return;
            }

            app::add_timeout3(0.05, move |_| {
                schedule_poll(Rc::clone(&receiver), sql_callback.clone());
            });
        }

        schedule_poll(receiver, sql_callback);
    }

    fn setup_callbacks(&mut self) {
        let connection = self.connection.clone();
        let sql_callback = self.sql_callback.clone();
        let action_sender = self.action_sender.clone();

        self.tree.handle(move |t, ev| {
            if !t.active() {
                return false;
            }
            match ev {
                Event::Push => {
                    let mouse_button = fltk::app::event_mouse_button();
                    if mouse_button == fltk::app::MouseButton::Right {
                        let clicked_item = t
                            .find_clicked(false)
                            .or_else(|| t.find_clicked(true))
                            .or_else(|| Self::item_at_mouse(t));

                        if let Some(item) = clicked_item {
                            let _ = t.select_only(&item, false);
                            t.set_item_focus(&item);
                            Self::show_context_menu(
                                &connection,
                                &item,
                                &sql_callback,
                                &action_sender,
                            );
                        } else if let Some(item) = t.first_selected_item() {
                            Self::show_context_menu(
                                &connection,
                                &item,
                                &sql_callback,
                                &action_sender,
                            );
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
                                if object_type == "TABLES" || object_type == "VIEWS" {
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

    fn item_at_mouse(tree: &Tree) -> Option<TreeItem> {
        let mouse_y = fltk::app::event_y();
        let mut current = tree.first_visible_item();
        while let Some(item) = current {
            let item_y = item.y();
            let item_h = item.h();
            if mouse_y >= item_y && mouse_y < item_y + item_h {
                return Some(item);
            }
            current = tree.next_visible_item(&item, Key::Down);
        }
        None
    }

    fn get_item_info(item: &TreeItem) -> Option<ObjectItem> {
        let object_name = match item.label() {
            Some(label) => label.trim().to_string(),
            None => return None,
        };
        let parent = match item.parent() {
            Some(parent) => parent,
            None => return None,
        };
        let parent_label = match parent.label() {
            Some(label) => label.trim().to_string(),
            None => return None,
        };
        let parent_type_upper = parent_label.to_uppercase();

        // Make sure this is not a category item
        if parent_type_upper == "PROCEDURES" {
            if let Some(grandparent) = parent.parent() {
                if let Some(package_label) = grandparent.label() {
                    if let Some(root) = grandparent.parent() {
                        if let Some(root_label) = root.label() {
                            if root_label.trim().eq_ignore_ascii_case("Packages") {
                                return Some(ObjectItem::PackageProcedure {
                                    package_name: package_label.trim().to_string(),
                                    procedure_name: object_name,
                                });
                            }
                        }
                    }
                }
            }
        }

        match parent_type_upper.as_str() {
            "TABLES" | "VIEWS" | "PROCEDURES" | "FUNCTIONS" | "SEQUENCES" | "PACKAGES" => {
                Some(ObjectItem::Simple {
                    object_type: parent_type_upper,
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

    fn build_simple_procedure_script(qualified_name: &str) -> String {
        format!("BEGIN\n  {};\nEND;\n/\n", qualified_name)
    }

    fn build_procedure_script(qualified_name: &str, arguments: &[ProcedureArgument]) -> String {
        if arguments.is_empty() {
            return Self::build_simple_procedure_script(qualified_name);
        }

        let selected_args = Self::select_overload_arguments(arguments);
        if selected_args.is_empty() {
            return Self::build_simple_procedure_script(qualified_name);
        }

        let mut used_names: HashSet<String> = HashSet::new();
        let mut local_decls: Vec<String> = Vec::new();
        let mut call_args: Vec<String> = Vec::new();
        let mut cursor_binds: Vec<String> = Vec::new();

        for arg in &selected_args {
            let arg_label = arg.name.clone();
            let var_base = arg_label
                .as_deref()
                .unwrap_or("ARG");
            let var_name = Self::unique_var_name(var_base, arg.position, &mut used_names);
            let direction = arg
                .in_out
                .clone()
                .unwrap_or_else(|| "IN".to_string())
                .replace('/', " ")
                .to_uppercase();
            let is_out = direction.contains("OUT");
            let is_in = direction.contains("IN");

            if is_out && Self::is_ref_cursor(arg) {
                cursor_binds.push(var_name.clone());
                let target = format!(":{}", var_name);
                let call_expr = match &arg_label {
                    Some(label) => format!("{} => {}", label, target),
                    None => target,
                };
                call_args.push(call_expr);
            } else {
                let type_str = Self::format_argument_type(arg);
                if is_in {
                    let default_expr = Self::default_value_for_argument(arg, &type_str);
                    local_decls.push(format!("  {} {} := {};", var_name, type_str, default_expr));
                } else {
                    local_decls.push(format!("  {} {};", var_name, type_str));
                }
                let call_expr = match &arg_label {
                    Some(label) => format!("{} => {}", label, var_name),
                    None => var_name,
                };
                call_args.push(call_expr);
            }
        }

        let mut script = String::new();
        for cursor in &cursor_binds {
            script.push_str(&format!("VAR {} REFCURSOR\n", cursor));
        }

        if !local_decls.is_empty() {
            script.push_str("DECLARE\n");
            for decl in &local_decls {
                script.push_str(decl);
                script.push('\n');
            }
        }

        script.push_str("BEGIN\n");
        if call_args.is_empty() {
            script.push_str(&format!("  {};\n", qualified_name));
        } else {
            script.push_str(&format!("  {}(\n", qualified_name));
            for (idx, arg) in call_args.iter().enumerate() {
                let suffix = if idx + 1 == call_args.len() { "" } else { "," };
                script.push_str(&format!("    {}{}\n", arg, suffix));
            }
            script.push_str("  );\n");
        }
        script.push_str("END;\n/\n");

        for cursor in cursor_binds {
            script.push_str(&format!("PRINT {}\n", cursor));
        }

        script
    }

    fn select_overload_arguments(arguments: &[ProcedureArgument]) -> Vec<ProcedureArgument> {
        let mut selected: Vec<ProcedureArgument> = Vec::new();
        let mut selected_overload: Option<i32> = None;
        for arg in arguments {
            if selected_overload.is_none() {
                selected_overload = arg.overload;
            }
            if arg.overload == selected_overload {
                selected.push(arg.clone());
            } else {
                break;
            }
        }
        selected
    }

    fn is_ref_cursor(arg: &ProcedureArgument) -> bool {
        let data_type = arg.data_type.as_deref().unwrap_or("").to_uppercase();
        if data_type.contains("REF CURSOR") || data_type.contains("REFCURSOR") {
            return true;
        }
        if data_type == "SYS_REFCURSOR" {
            return true;
        }
        if let Some(pls_type) = arg.pls_type.as_deref() {
            let upper = pls_type.to_uppercase();
            if upper.contains("REF CURSOR") || upper.contains("REFCURSOR") {
                return true;
            }
        }
        if let Some(type_name) = arg.type_name.as_deref() {
            if type_name.eq_ignore_ascii_case("REFCURSOR") {
                return true;
            }
        }
        false
    }

    fn format_argument_type(arg: &ProcedureArgument) -> String {
        if let Some(pls_type) = arg.pls_type.as_deref() {
            let trimmed = pls_type.trim();
            if !trimmed.is_empty() {
                if trimmed.contains('%') {
                    return trimmed.to_string();
                }
                let upper = trimmed.to_uppercase();
                if Self::is_string_type_without_length(&upper) {
                    let len = Self::clamp_string_length(arg.data_length);
                    return format!("{}({})", upper, len);
                }
                return trimmed.to_string();
            }
        }
        if let Some(data_type) = arg.data_type.as_deref() {
            let upper = data_type.to_uppercase();
            if upper.contains("REF CURSOR") || upper.contains("REFCURSOR") {
                return "SYS_REFCURSOR".to_string();
            }
            if upper.starts_with("NUMBER") {
                if let Some(precision) = arg.data_precision {
                    if let Some(scale) = arg.data_scale {
                        return format!("NUMBER({}, {})", precision, scale);
                    }
                    return format!("NUMBER({})", precision);
                }
                return "NUMBER".to_string();
            }
            if upper.starts_with("VARCHAR2")
                || upper.starts_with("NVARCHAR2")
                || upper.starts_with("CHAR")
                || upper.starts_with("NCHAR")
                || upper.starts_with("RAW")
            {
                let len = Self::clamp_string_length(arg.data_length);
                return format!("{}({})", upper, len);
            }
            return upper;
        }

        if let Some(type_name) = arg.type_name.as_deref() {
            if let Some(owner) = arg.type_owner.as_deref() {
                return format!("{}.{}", owner, type_name);
            }
            return type_name.to_string();
        }

        "VARCHAR2(4000)".to_string()
    }

    fn is_string_type_without_length(upper: &str) -> bool {
        if upper.contains('(') {
            return false;
        }
        matches!(
            upper,
            "VARCHAR2" | "NVARCHAR2" | "VARCHAR" | "CHAR" | "NCHAR" | "RAW"
        )
    }

    fn clamp_string_length(length: Option<i32>) -> i32 {
        let fallback = 32767;
        let len = length.unwrap_or(fallback);
        let len = if len <= 0 { fallback } else { len };
        len.clamp(1, 32767)
    }

    fn default_value_for_argument(arg: &ProcedureArgument, type_str: &str) -> String {
        if let Some(default_value) = arg.default_value.as_deref() {
            let trimmed = default_value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if Self::is_ref_cursor(arg) {
            return "NULL".to_string();
        }

        let base = Self::normalize_type_base(type_str);
        if base.contains('.') {
            return "NULL".to_string();
        }

        match base.as_str() {
            "NUMBER" | "NUMERIC" | "DECIMAL" | "INTEGER" | "INT" | "PLS_INTEGER"
            | "BINARY_INTEGER" | "NATURAL" | "NATURALN" | "POSITIVE" | "POSITIVEN"
            | "SIMPLE_INTEGER" => "0".to_string(),
            "FLOAT" | "BINARY_FLOAT" | "BINARY_DOUBLE" => "0".to_string(),
            "VARCHAR2" | "NVARCHAR2" | "VARCHAR" | "CHAR" | "NCHAR" => "''".to_string(),
            "CLOB" | "NCLOB" => "EMPTY_CLOB()".to_string(),
            "BLOB" => "EMPTY_BLOB()".to_string(),
            "RAW" => "HEXTORAW('')".to_string(),
            "DATE" => "SYSDATE".to_string(),
            "TIMESTAMP" => "SYSTIMESTAMP".to_string(),
            "BOOLEAN" => "FALSE".to_string(),
            _ => "NULL".to_string(),
        }
    }

    fn normalize_type_base(type_str: &str) -> String {
        let mut upper = type_str.trim().to_uppercase();
        if let Some(idx) = upper.find('(') {
            upper.truncate(idx);
        }
        if let Some(idx) = upper.find(' ') {
            upper.truncate(idx);
        }
        upper
    }

    fn unique_var_name(
        base_name: &str,
        position: i32,
        used: &mut HashSet<String>,
    ) -> String {
        let mut cleaned = base_name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        if cleaned.is_empty() {
            cleaned = format!("arg{}", position.max(1));
        }
        if cleaned.chars().next().map(|ch| ch.is_ascii_digit()).unwrap_or(false) {
            cleaned.insert(0, '_');
        }
        let candidate = format!("v_{}", cleaned);
        if used.insert(candidate.clone()) {
            return candidate;
        }

        let mut suffix = 2;
        loop {
            let next = format!("{}_{}", candidate, suffix);
            if used.insert(next.clone()) {
                return next;
            }
            suffix += 1;
        }
    }

    fn show_context_menu(
        connection: &SharedConnection,
        item: &TreeItem,
        sql_callback: &SqlExecuteCallback,
        action_sender: &std::sync::mpsc::Sender<ObjectActionResult>,
    ) {
        if let Some(item_info) = Self::get_item_info(item) {
            let menu_choices = match &item_info {
                ObjectItem::Simple { object_type, .. } if object_type == "TABLES" => {
                    "Select Data (Top 100)|View Structure|View Indexes|View Constraints|Generate DDL"
                }
                ObjectItem::Simple { object_type, .. } if object_type == "VIEWS" => {
                    "Select Data (Top 100)|Generate DDL"
                }
                ObjectItem::Simple { object_type, .. }
                    if object_type == "PROCEDURES" || object_type == "FUNCTIONS" =>
                {
                    if object_type == "PROCEDURES" {
                        "Execute Procedure|Generate DDL"
                    } else {
                        "Generate DDL"
                    }
                }
                ObjectItem::Simple { object_type, .. } if object_type == "SEQUENCES" => {
                    "View Info|Generate DDL"
                }
                ObjectItem::PackageProcedure { .. } => {
                    "Execute Procedure"
                }
                ObjectItem::Simple { object_type, .. } if object_type == "PACKAGES" => {
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

                match (choice_label.as_str(), &item_info) {
                    ("Select Data (Top 100)", ObjectItem::Simple { object_name, .. }) => {
                        let sql = format!("SELECT * FROM {} WHERE ROWNUM <= 100", object_name);
                        let cb_opt = sql_callback.borrow_mut().take();
                        if let Some(mut cb) = cb_opt {
                            cb(SqlAction::Execute(sql));
                            *sql_callback.borrow_mut() = Some(cb);
                        }
                    }
                    ("Execute Procedure", ObjectItem::Simple { object_name, .. }) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let object_name = object_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_procedure_arguments(
                                    db_conn.as_ref(),
                                    &object_name,
                                )
                                .map(|arguments| {
                                    ObjectBrowserWidget::build_procedure_script(
                                        &object_name,
                                        &arguments,
                                    )
                                })
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };

                            let _ = sender.send(ObjectActionResult::ProcedureScript {
                                qualified_name: object_name,
                                result,
                            });
                            app::awake();
                        });
                    }
                    (
                        "Execute Procedure",
                        ObjectItem::PackageProcedure {
                            package_name,
                            procedure_name,
                        },
                    ) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let qualified_name = format!("{}.{}", package_name, procedure_name);
                        let package_name = package_name.clone();
                        let procedure_name = procedure_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_package_procedure_arguments(
                                    db_conn.as_ref(),
                                    &package_name,
                                    &procedure_name,
                                )
                                .map(|arguments| {
                                    ObjectBrowserWidget::build_procedure_script(
                                        &qualified_name,
                                        &arguments,
                                    )
                                })
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };

                            let _ = sender.send(ObjectActionResult::ProcedureScript {
                                qualified_name,
                                result,
                            });
                            app::awake();
                        });
                    }
                    ("View Structure", ObjectItem::Simple { object_name, .. }) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let table_name = object_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_table_structure(
                                    db_conn.as_ref(),
                                    &table_name,
                                )
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };
                            let _ = sender.send(ObjectActionResult::TableStructure {
                                table_name,
                                result,
                            });
                            app::awake();
                        });
                    }
                    ("View Indexes", ObjectItem::Simple { object_name, .. }) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let table_name = object_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_table_indexes(
                                    db_conn.as_ref(),
                                    &table_name,
                                )
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };
                            let _ = sender.send(ObjectActionResult::TableIndexes {
                                table_name,
                                result,
                            });
                            app::awake();
                        });
                    }
                    ("View Constraints", ObjectItem::Simple { object_name, .. }) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let table_name = object_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_table_constraints(
                                    db_conn.as_ref(),
                                    &table_name,
                                )
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };
                            let _ = sender.send(ObjectActionResult::TableConstraints {
                                table_name,
                                result,
                            });
                            app::awake();
                        });
                    }
                    ("View Info", ObjectItem::Simple { object_name, .. }) => {
                        let connection = connection.clone();
                        let sender = action_sender.clone();
                        let sequence_name = object_name.clone();
                        thread::spawn(move || {
                            let conn = {
                                let conn_guard = lock_connection(&connection);
                                if !conn_guard.is_connected() {
                                    None
                                } else {
                                    conn_guard.get_connection()
                                }
                            };
                            let result = if let Some(db_conn) = conn {
                                ObjectBrowser::get_sequence_info(
                                    db_conn.as_ref(),
                                    &sequence_name,
                                )
                                .map_err(|err| err.to_string())
                            } else {
                                Err("Not connected to database".to_string())
                            };
                            let _ = sender.send(ObjectActionResult::SequenceInfo(result));
                            app::awake();
                        });
                    }
                    (
                        "Generate DDL",
                        ObjectItem::Simple {
                            object_type,
                            object_name,
                        },
                    ) => {
                        let obj_type = match object_type.as_str() {
                            "TABLES" => Some("TABLE"),
                            "VIEWS" => Some("VIEW"),
                            "PROCEDURES" => Some("PROCEDURE"),
                            "FUNCTIONS" => Some("FUNCTION"),
                            "SEQUENCES" => Some("SEQUENCE"),
                            "PACKAGES" => Some("PACKAGE"),
                            _ => None,
                        };
                        if let Some(obj_type) = obj_type {
                            let connection = connection.clone();
                            let sender = action_sender.clone();
                            let object_type = obj_type.to_string();
                            let object_name = object_name.clone();
                            thread::spawn(move || {
                                let conn = {
                                    let conn_guard = lock_connection(&connection);
                                    if !conn_guard.is_connected() {
                                        None
                                    } else {
                                        conn_guard.get_connection()
                                    }
                                };
                                let result = if let Some(db_conn) = conn {
                                    match object_type.as_str() {
                                        "TABLE" => ObjectBrowser::get_table_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        "VIEW" => ObjectBrowser::get_view_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        "PROCEDURE" => ObjectBrowser::get_procedure_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        "FUNCTION" => ObjectBrowser::get_function_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        "SEQUENCE" => ObjectBrowser::get_sequence_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        "PACKAGE" => ObjectBrowser::get_package_spec_ddl(
                                            db_conn.as_ref(),
                                            &object_name,
                                        ),
                                        _ => return,
                                    }
                                    .map_err(|err| err.to_string())
                                } else {
                                    Err("Not connected to database".to_string())
                                };
                                let _ = sender.send(ObjectActionResult::Ddl(result));
                                app::awake();
                            });
                        }
                    }
                    _ => {}
                }
            }

            // FLTK memory management: widgets created without a parent must be deleted.
            unsafe {
                let widget = Widget::from_widget_ptr(menu.as_widget_ptr());
                Widget::delete(widget);
            }
        }
    }

    fn show_info_dialog(title: &str, content: &str) {
        use fltk::{prelude::*, text::TextDisplay, window::Window};

        fltk::group::Group::set_current(None::<&fltk::group::Group>);
        
        let mut dialog = Window::default()
            .with_size(700, 500)
            .with_label(title);
        crate::ui::center_on_main(&mut dialog);
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);

        let mut display = TextDisplay::default().with_pos(10, 10).with_size(680, 440);
        display.set_color(theme::editor_bg());
        display.set_text_color(theme::text_primary());
        display.set_text_font(fltk::enums::Font::Courier);
        display.set_text_size(14);

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
