use fltk::{
    app,
    dialog::{FileDialog, FileDialogType},
    enums::{Color, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    menu::MenuBar,
    prelude::*,
    text::TextBuffer,
    window::Window,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;

use crate::db::{
    create_shared_connection, lock_connection, ObjectBrowser, QueryResult, SharedConnection,
};
use crate::ui::{
    ConnectionDialog, FeatureCatalogDialog, FindReplaceDialog, HighlightData, IntellisenseData,
    MenuBarBuilder, ObjectBrowserWidget, QueryHistoryDialog, QueryProgress, ResultTabsWidget,
    SqlAction, SqlEditorWidget,
};
use crate::utils::QueryHistory;

pub struct MainWindow {
    window: Window,
    connection: SharedConnection,
    sql_editor: SqlEditorWidget,
    sql_buffer: TextBuffer,
    result_tabs: ResultTabsWidget,
    object_browser: ObjectBrowserWidget,
    status_bar: Frame,
    current_file: Rc<RefCell<Option<PathBuf>>>,
    last_result: Rc<RefCell<Option<QueryResult>>>,
    #[allow(dead_code)]
    query_history: Rc<RefCell<QueryHistory>>,
}

#[derive(Clone)]
struct SchemaUpdate {
    data: IntellisenseData,
    highlight_data: HighlightData,
}

impl MainWindow {
    pub fn new() -> Self {
        let connection = create_shared_connection();

        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition");
        // Modern dark theme - primary background
        window.set_color(Color::from_rgb(30, 30, 30));

        let mut main_flex = Flex::default_fill();
        main_flex.set_type(FlexType::Column);
        main_flex.set_margin(0);

        // Menu bar
        let menu_bar = MenuBarBuilder::build();
        main_flex.fixed(&menu_bar, 30);

        // Main content area with horizontal flex for panels
        let mut content_flex = Flex::default();
        content_flex.set_type(FlexType::Row);
        content_flex.set_margin(0);
        content_flex.set_spacing(0);

        // Left panel - Object Browser
        let object_browser = ObjectBrowserWidget::new(0, 0, 250, 600, connection.clone());
        let obj_browser_widget = object_browser.get_widget();
        content_flex.fixed(&obj_browser_widget, 250);

        // Right panel - Editor and Results
        let mut right_flex = Flex::default();
        right_flex.set_type(FlexType::Column);
        right_flex.set_margin(0);
        right_flex.set_pad(0);
        right_flex.set_spacing(0);

        // SQL Editor
        let sql_editor = SqlEditorWidget::new(connection.clone());
        let sql_group = sql_editor.get_group().clone();
        right_flex.fixed(&sql_group, 250);

        // Result Tabs - use explicit size to avoid default_fill() panic
        let result_tabs = ResultTabsWidget::new(0, 0, 900, 400);
        let result_widget = result_tabs.get_widget();
        right_flex.add(&result_widget);
        right_flex.resizable(&result_widget);

        right_flex.end();

        // Make right_flex resizable within content_flex
        content_flex.resizable(&right_flex);
        content_flex.end();

        // Make content_flex resizable within main_flex (takes remaining space)
        main_flex.resizable(&content_flex);

        // Status bar - modern accent color
        let mut status_bar =
            Frame::default().with_label("Not connected | Ctrl+Space for autocomplete");
        status_bar.set_frame(FrameType::FlatBox);
        status_bar.set_color(Color::from_rgb(0, 120, 212)); // Modern blue accent
        status_bar.set_label_color(Color::White);
        status_bar.set_label_font(Font::Helvetica);
        status_bar.set_label_size(12);
        main_flex.fixed(&status_bar, 25);

        main_flex.end();
        window.end();

        window.make_resizable(true);

        let sql_buffer = sql_editor.get_buffer();
        let current_file = Rc::new(RefCell::new(None));
        let last_result = Rc::new(RefCell::new(None));
        let query_history = Rc::new(RefCell::new(QueryHistory::load()));

        Self {
            window,
            connection,
            sql_editor,
            sql_buffer,
            result_tabs,
            object_browser,
            status_bar,
            current_file,
            last_result,
            query_history,
        }
    }

    pub fn setup_callbacks(&mut self) {
        let mut status_bar = self.status_bar.clone();
        let last_result = self.last_result.clone();

        // Setup SQL editor execute callback
        self.sql_editor.set_execute_callback(move |query_result| {
            *last_result.borrow_mut() = Some(query_result.clone());

            // Update status bar
            let status_text = format!(
                "{} | Time: {:.3}s",
                query_result.message,
                query_result.execution_time.as_secs_f64()
            );
            status_bar.set_label(&status_text);

            // Note: Query history is saved in SqlEditorWidget::execute_sql()
            // to avoid duplicate saves and UI blocking
        });

        // Setup object browser callback to set SQL in editor
        let mut sql_buffer = self.sql_buffer.clone();
        let mut editor = self.sql_editor.get_editor();
        let highlighter = self.sql_editor.get_highlighter();
        let style_buffer = self.sql_editor.get_style_buffer();
        let sql_editor = self.sql_editor.clone();

        self.object_browser
            .set_sql_callback(move |action| match action {
                SqlAction::Set(sql) => {
                    sql_buffer.set_text(&sql);
                    // Refresh highlighting
                    highlighter
                        .borrow()
                        .highlight(&sql, &mut style_buffer.clone());
                }
                SqlAction::Insert(text) => {
                    let insert_pos = editor.insert_position();
                    sql_buffer.insert(insert_pos, &text);
                    editor.set_insert_position(insert_pos + text.len() as i32);
                    sql_editor.refresh_highlighting();
                }
            });

        let mut result_tabs_stream = self.result_tabs.clone();
        let streaming_indices = Rc::new(RefCell::new(HashSet::new()));
        let streaming_indices_for_cb = streaming_indices.clone();
        self.sql_editor
            .set_progress_callback(move |progress| match progress {
                QueryProgress::BatchStart => {
                    result_tabs_stream.clear();
                    streaming_indices_for_cb.borrow_mut().clear();
                }
                QueryProgress::StatementStart { index } => {
                    result_tabs_stream.start_statement(index, &format!("Result {}", index + 1));
                }
                QueryProgress::SelectStart { index, columns } => {
                    streaming_indices_for_cb.borrow_mut().insert(index);
                    result_tabs_stream.start_streaming(index, &columns);
                }
                QueryProgress::Rows { index, rows } => {
                    result_tabs_stream.append_rows(index, rows);
                }
                QueryProgress::StatementFinished { index, result } => {
                    if result.is_select {
                        let was_streaming =
                            streaming_indices_for_cb.borrow_mut().remove(&index);
                        if was_streaming {
                            // Flush any remaining buffered rows for SELECT queries
                            result_tabs_stream.finish_streaming(index);
                        } else {
                            result_tabs_stream.display_result(index, &result);
                        }
                    } else {
                        result_tabs_stream.display_result(index, &result);
                    }
                }
                QueryProgress::BatchFinished => {}
            });

        // Setup menu callbacks
        self.setup_menu_callbacks();
    }

    fn setup_menu_callbacks(&mut self) {
        let connection = self.connection.clone();
        let mut status_bar = self.status_bar.clone();
        let mut object_browser = self.object_browser.clone();
        let intellisense_data = self.sql_editor.get_intellisense_data();
        let highlighter = self.sql_editor.get_highlighter();
        let mut sql_buffer = self.sql_buffer.clone();
        let current_file = self.current_file.clone();
        let mut window = self.window.clone();
        let style_buffer = self.sql_editor.get_style_buffer();
        let highlighter_for_file = highlighter.clone();
        let mut editor = self.sql_editor.get_editor();
        let mut editor_buffer = self.sql_buffer.clone();
        let mut sql_editor = self.sql_editor.clone();
        let result_table_export = self.result_tabs.clone();
        let mut status_bar_export = self.status_bar.clone();
        let (schema_sender, schema_receiver) = app::channel::<SchemaUpdate>();

        let intellisense_data_for_schema = intellisense_data.clone();
        let highlighter_for_schema = highlighter.clone();
        app::add_idle3(move |_| {
            while let Some(update) = schema_receiver.recv() {
                *intellisense_data_for_schema.borrow_mut() = update.data;
                highlighter_for_schema
                    .borrow_mut()
                    .set_highlight_data(update.highlight_data);
            }
        });

        // Find menu bar and set callbacks
        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            menu.set_callback(move |m| {
                let menu_path = m
                    .item_pathname(None)
                    .ok()
                    .or_else(|| m.choice().map(|path| path.to_string()));
                if let Some(path) = menu_path {
                    let choice = path
                        .split('\t')
                        .next()
                        .unwrap_or(path.as_str())
                        .trim()
                        .replace('&', "");
                    match choice.as_str() {
                        "File/Connect..." => {
                            if let Some(info) = ConnectionDialog::show() {
                                let mut db_conn = lock_connection(&connection);
                                match db_conn.connect(info.clone()) {
                                    Ok(_) => {
                                        status_bar.set_label(&format!(
                                            "Connected: {} | Ctrl+Space for autocomplete",
                                            info.display_string()
                                        ));
                                        drop(db_conn);
                                        object_browser.refresh();
                                        sql_editor.focus();

                                        let schema_sender = schema_sender.clone();
                                        let connection_for_schema = connection.clone();
                                        thread::spawn(move || {
                                            let conn_guard =
                                                lock_connection(&connection_for_schema);
                                            if !conn_guard.is_connected() {
                                                return;
                                            }

                                            let Some(conn) = conn_guard.get_connection() else {
                                                return;
                                            };
                                            drop(conn_guard);

                                            let mut data = IntellisenseData::new();
                                            let mut highlight_data = HighlightData::new();

                                            if let Ok(tables) =
                                                ObjectBrowser::get_tables(conn.as_ref())
                                            {
                                                highlight_data.tables = tables.clone();
                                                data.tables = tables;
                                            }

                                            if let Ok(views) =
                                                ObjectBrowser::get_views(conn.as_ref())
                                            {
                                                highlight_data.views = views.clone();
                                                data.views = views;
                                            }

                                            if let Ok(procs) =
                                                ObjectBrowser::get_procedures(conn.as_ref())
                                            {
                                                data.procedures = procs;
                                            }

                                            if let Ok(funcs) =
                                                ObjectBrowser::get_functions(conn.as_ref())
                                            {
                                                data.functions = funcs;
                                            }

                                            let _ = schema_sender.send(SchemaUpdate {
                                                data,
                                                highlight_data,
                                            });
                                        });
                                    }
                                    Err(e) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Connection failed: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        "File/Disconnect" => {
                            let mut db_conn = lock_connection(&connection);
                            db_conn.disconnect();
                            status_bar.set_label("Disconnected | Ctrl+Space for autocomplete");

                            // Clear intellisense and highlight data
                            *intellisense_data.borrow_mut() = IntellisenseData::new();
                            highlighter
                                .borrow_mut()
                                .set_highlight_data(HighlightData::new());
                        }
                        "File/Open SQL File..." => {
                            let mut dialog = FileDialog::new(FileDialogType::BrowseFile);
                            dialog.set_title("Open SQL File");
                            dialog.set_filter("SQL Files\t*.sql\nAll Files\t*");
                            dialog.show();

                            let filename = dialog.filename();
                            if !filename.as_os_str().is_empty() {
                                match fs::read_to_string(&filename) {
                                    Ok(content) => {
                                        sql_buffer.set_text(&content);
                                        *current_file.borrow_mut() = Some(filename.clone());

                                        // Update window title
                                        let title = format!(
                                            "Oracle Query Tool - {}",
                                            filename
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                        );
                                        window.set_label(&title);

                                        // Refresh highlighting
                                        let text = sql_buffer.text();
                                        highlighter_for_file
                                            .borrow()
                                            .highlight(&text, &mut style_buffer.clone());

                                        // Focus on editor
                                        sql_editor.focus();

                                        status_bar
                                            .set_label(&format!("Opened: {}", filename.display()));
                                    }
                                    Err(e) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to open file: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        "File/Save SQL File..." => {
                            let current = current_file.borrow().clone();
                            let save_path = if let Some(path) = current {
                                Some(path)
                            } else {
                                let mut dialog = FileDialog::new(FileDialogType::BrowseSaveFile);
                                dialog.set_title("Save SQL File");
                                dialog.set_filter("SQL Files\t*.sql\nAll Files\t*");
                                dialog.set_preset_file("query.sql");
                                dialog.show();

                                let filename = dialog.filename();
                                if !filename.as_os_str().is_empty() {
                                    // Add .sql extension if not present
                                    let path = if filename.extension().is_none() {
                                        filename.with_extension("sql")
                                    } else {
                                        filename
                                    };
                                    Some(path)
                                } else {
                                    None
                                }
                            };

                            if let Some(path) = save_path {
                                let content = sql_buffer.text();
                                match fs::write(&path, &content) {
                                    Ok(_) => {
                                        *current_file.borrow_mut() = Some(path.clone());

                                        // Update window title
                                        let title = format!(
                                            "Oracle Query Tool - {}",
                                            path.file_name().unwrap_or_default().to_string_lossy()
                                        );
                                        window.set_label(&title);

                                        status_bar.set_label(&format!("Saved: {}", path.display()));
                                    }
                                    Err(e) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to save file: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        "File/Exit" => {
                            app::quit();
                        }
                        "Edit/Undo" => {
                            editor.undo();
                        }
                        "Edit/Redo" => {
                            editor.redo();
                        }
                        "Edit/Cut" => {
                            editor.cut();
                        }
                        "Edit/Copy" => {
                            editor.copy();
                        }
                        "Edit/Paste" => {
                            editor.paste();
                        }
                        "Edit/Select All" => {
                            let buffer_len = editor_buffer.length();
                            editor_buffer.select(0, buffer_len);
                        }
                        "Edit/Find Next" => {
                            if editor_buffer.selected() {
                                let search_text = editor_buffer.selection_text();
                                if !FindReplaceDialog::find_next(
                                    &mut editor,
                                    &mut editor_buffer,
                                    &search_text,
                                    false,
                                ) {
                                    fltk::dialog::message_default("Text not found");
                                }
                            } else {
                                fltk::dialog::message_default("Select text to search for");
                            }
                        }
                        "Query/Execute" => {
                            sql_editor.execute_current();
                        }
                        "Query/Execute Selected" => {
                            sql_editor.execute_selected();
                        }
                        "Query/Explain Plan" => {
                            sql_editor.explain_current();
                        }
                        "Query/Commit" => {
                            sql_editor.commit();
                        }
                        "Query/Rollback" => {
                            sql_editor.rollback();
                        }
                        "Tools/Refresh Objects" => {
                            object_browser.refresh();
                        }
                        "Tools/Export Results..." => {
                            if !result_table_export.has_data() {
                                fltk::dialog::alert_default("No data to export");
                                return;
                            }

                            let mut dialog = FileDialog::new(FileDialogType::BrowseSaveFile);
                            dialog.set_title("Export Results to CSV");
                            dialog.set_filter("CSV Files\t*.csv\nAll Files\t*");
                            dialog.set_preset_file("results.csv");
                            dialog.show();

                            let filename = dialog.filename();
                            if !filename.as_os_str().is_empty() {
                                let path = if filename.extension().is_none() {
                                    filename.with_extension("csv")
                                } else {
                                    filename
                                };

                                let csv_content = result_table_export.export_to_csv();
                                match fs::write(&path, &csv_content) {
                                    Ok(_) => {
                                        status_bar_export.set_label(&format!(
                                            "Exported {} rows to {}",
                                            result_table_export.row_count(),
                                            path.display()
                                        ));
                                    }
                                    Err(e) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to export: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        "Edit/Find..." => {
                            FindReplaceDialog::show_find(&mut editor, &mut editor_buffer);
                        }
                        "Edit/Replace..." => {
                            FindReplaceDialog::show_replace(&mut editor, &mut editor_buffer);
                        }
                        "Tools/Query History..." => {
                            if let Some(sql) = QueryHistoryDialog::show() {
                                sql_buffer.set_text(&sql);
                                // Refresh highlighting
                                highlighter_for_file
                                    .borrow()
                                    .highlight(&sql, &mut style_buffer.clone());
                            }
                        }
                        "Tools/Feature Catalog..." => {
                            FeatureCatalogDialog::show();
                        }
                        "Tools/Auto-Commit" => {
                            if let Some(item) = m.find_item("Tools/Auto-Commit") {
                                let enabled = item.value();
                                let mut db_conn = lock_connection(&connection);
                                db_conn.set_auto_commit(enabled);
                                let status = if enabled {
                                    "Auto-commit enabled"
                                } else {
                                    "Auto-commit disabled"
                                };
                                status_bar.set_label(status);
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    pub fn show(&mut self) {
        self.window.show();
        self.sql_editor.focus();
    }

    pub fn run() {
        let app = app::App::default().with_scheme(app::Scheme::Gtk);

        // Set default colors for dark theme
        app::background(45, 45, 48);
        app::foreground(220, 220, 220);

        let mut main_window = MainWindow::new();
        main_window.setup_callbacks();
        main_window.show();

        app.run().unwrap();
    }

    #[allow(dead_code)]
    fn export_results_csv(
        path: &PathBuf,
        result: &QueryResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut output = String::new();

        let headers: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
        output.push_str(&Self::csv_row(&headers));
        output.push('\n');

        for row in &result.rows {
            output.push_str(&Self::csv_row(row));
            output.push('\n');
        }

        fs::write(path, output)?;
        Ok(())
    }

    #[allow(dead_code)]
    fn csv_row(values: &[String]) -> String {
        values
            .iter()
            .map(|value| Self::csv_escape(value))
            .collect::<Vec<String>>()
            .join(",")
    }

    #[allow(dead_code)]
    fn csv_escape(value: &str) -> String {
        if value.contains(',') || value.contains('"') || value.contains('\n') {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }

    #[allow(dead_code)]
    fn format_query_history(history: &QueryHistory) -> String {
        if history.queries.is_empty() {
            return "No query history yet.".to_string();
        }

        let mut lines = vec!["Recent Queries (latest first):".to_string()];
        for entry in history.queries.iter().take(20) {
            lines.push(format!(
                "[{}] {} | {} ms | {} rows",
                entry.timestamp, entry.connection_name, entry.execution_time_ms, entry.row_count
            ));
            lines.push(entry.sql.trim().to_string());
            lines.push(String::new());
        }

        lines.join("\n")
    }
}

impl Default for MainWindow {
    fn default() -> Self {
        Self::new()
    }
}
