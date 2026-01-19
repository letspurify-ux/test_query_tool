use fltk::{
    app,
    dialog::{FileDialog, FileDialogType},
    enums::{Color, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType, Pack, PackType, Tile},
    menu::MenuBar,
    prelude::*,
    text::TextBuffer,
    window::Window,
};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use crate::db::{create_shared_connection, ObjectBrowser, QueryResult, SharedConnection};
use crate::ui::{
    ConnectionDialog, HighlightData, IntellisenseData, MenuBarBuilder, ObjectBrowserWidget,
    ResultTableWidget, SqlEditorWidget,
};
use crate::utils::{QueryHistory, QueryHistoryEntry};
use chrono::Local;

pub struct MainWindow {
    window: Window,
    connection: SharedConnection,
    sql_editor: SqlEditorWidget,
    sql_buffer: TextBuffer,
    result_table: ResultTableWidget,
    object_browser: ObjectBrowserWidget,
    status_bar: Frame,
    current_file: Rc<RefCell<Option<PathBuf>>>,
    last_result: Rc<RefCell<Option<QueryResult>>>,
    query_history: Rc<RefCell<QueryHistory>>,
}

impl MainWindow {
    pub fn new() -> Self {
        let connection = create_shared_connection();

        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition");
        window.set_color(Color::from_rgb(45, 45, 48));

        let mut main_flex = Flex::default_fill();
        main_flex.set_type(FlexType::Column);
        main_flex.set_margin(0);

        // Menu bar
        let menu_bar = MenuBarBuilder::build();
        main_flex.fixed(&menu_bar, 30);

        // Toolbar
        let toolbar = Self::create_toolbar();
        main_flex.fixed(&toolbar, 35);

        // Main content area with Tile for resizable panels
        let mut content_tile = Tile::default();
        content_tile.set_color(Color::from_rgb(45, 45, 48));

        // Left panel - Object Browser (200px wide)
        let object_browser = ObjectBrowserWidget::new(0, 0, 200, 600, connection.clone());

        // Right panel - Editor and Results
        let mut right_flex = Flex::default().with_pos(200, 0).with_size(1000, 600);
        right_flex.set_type(FlexType::Column);
        right_flex.set_margin(5);

        // SQL Editor
        let sql_editor = SqlEditorWidget::new(connection.clone());
        let sql_group = sql_editor.get_group().clone();
        right_flex.fixed(&sql_group, 250);

        // Result Table
        let result_table = ResultTableWidget::new();

        right_flex.end();
        content_tile.end();

        // Status bar
        let mut status_bar =
            Frame::default().with_label("Not connected | Ctrl+Space for autocomplete");
        status_bar.set_frame(FrameType::FlatBox);
        status_bar.set_color(Color::from_rgb(0, 122, 204));
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
            result_table,
            object_browser,
            status_bar,
            current_file,
            last_result,
            query_history,
        }
    }

    fn create_toolbar() -> Pack {
        let mut toolbar = Pack::default().with_size(0, 35);
        toolbar.set_type(PackType::Horizontal);
        toolbar.set_spacing(5);
        toolbar.set_color(Color::from_rgb(60, 60, 63));

        // Spacer
        let mut spacer = Frame::default().with_size(10, 35);
        spacer.set_frame(FrameType::FlatBox);
        spacer.set_color(Color::from_rgb(60, 60, 63));

        toolbar.end();
        toolbar
    }

    pub fn setup_callbacks(&mut self) {
        let mut status_bar = self.status_bar.clone();
        let mut result_table = self.result_table.clone();
        let last_result = self.last_result.clone();
        let query_history = self.query_history.clone();
        let connection = self.connection.clone();

        // Setup SQL editor execute callback
        self.sql_editor.set_execute_callback(move |query_result| {
            result_table.display_result(&query_result);
            *last_result.borrow_mut() = Some(query_result.clone());

            let status_text = format!(
                "{} | Time: {:.3}s",
                query_result.message,
                query_result.execution_time.as_secs_f64()
            );
            status_bar.set_label(&status_text);

            let connection_name = {
                let conn_guard = connection.lock().unwrap();
                if conn_guard.is_connected() {
                    let info = conn_guard.get_info();
                    if info.name.is_empty() {
                        info.display_string()
                    } else {
                        info.name.clone()
                    }
                } else {
                    "Disconnected".to_string()
                }
            };

            let entry = QueryHistoryEntry {
                sql: query_result.sql.clone(),
                timestamp: Local::now().to_rfc3339(),
                execution_time_ms: query_result.execution_time.as_millis() as u64,
                row_count: query_result.row_count,
                connection_name,
            };

            let mut history = query_history.borrow_mut();
            history.add_entry(entry);
            if let Err(err) = history.save() {
                fltk::dialog::alert_default(&format!("Failed to save query history: {}", err));
            }
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
        let sql_editor = self.sql_editor.clone();
        let mut sql_buffer = self.sql_buffer.clone();
        let current_file = self.current_file.clone();
        let mut window = self.window.clone();
        let style_buffer = self.sql_editor.get_style_buffer();
        let highlighter_for_file = highlighter.clone();
        let last_result = self.last_result.clone();
        let query_history = self.query_history.clone();

        // Find menu bar and set callbacks
        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            menu.set_callback(move |m| {
                if let Some(path) = m.choice() {
                    match path.as_str() {
                        "&File/&Connect...\t" => {
                            if let Some(info) = ConnectionDialog::show() {
                                let mut db_conn = connection.lock().unwrap();
                                match db_conn.connect(info.clone()) {
                                    Ok(_) => {
                                        status_bar.set_label(&format!(
                                            "Connected: {} | Ctrl+Space for autocomplete",
                                            info.display_string()
                                        ));

                                        // Update intellisense and highlight data
                                        if let Some(conn) = db_conn.get_connection() {
                                            let mut data = IntellisenseData::new();
                                            let mut highlight_data = HighlightData::new();

                                            // Load tables
                                            if let Ok(tables) = ObjectBrowser::get_tables(conn) {
                                                highlight_data.tables = tables.clone();
                                                data.tables = tables;
                                            }

                                            // Load views
                                            if let Ok(views) = ObjectBrowser::get_views(conn) {
                                                highlight_data.views = views.clone();
                                                data.views = views;
                                            }

                                            // Load procedures
                                            if let Ok(procs) = ObjectBrowser::get_procedures(conn) {
                                                data.procedures = procs;
                                            }

                                            // Load functions
                                            if let Ok(funcs) = ObjectBrowser::get_functions(conn) {
                                                data.functions = funcs;
                                            }

                                            // Load columns for each table
                                            for table in &data.tables.clone() {
                                                if let Ok(cols) =
                                                    ObjectBrowser::get_table_columns(conn, table)
                                                {
                                                    let col_names: Vec<String> =
                                                        cols.into_iter().map(|c| c.name).collect();
                                                    highlight_data
                                                        .columns
                                                        .extend(col_names.clone());
                                                    data.columns.push((table.clone(), col_names));
                                                }
                                            }

                                            *intellisense_data.borrow_mut() = data;
                                            highlighter
                                                .borrow_mut()
                                                .set_highlight_data(highlight_data);
                                        }

                                        drop(db_conn);
                                        object_browser.refresh();
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
                        "&File/&Disconnect\t" => {
                            let mut db_conn = connection.lock().unwrap();
                            db_conn.disconnect();
                            status_bar.set_label("Disconnected | Ctrl+Space for autocomplete");

                            // Clear intellisense and highlight data
                            *intellisense_data.borrow_mut() = IntellisenseData::new();
                            highlighter
                                .borrow_mut()
                                .set_highlight_data(HighlightData::new());
                        }
                        "&File/&Open SQL File...\t" => {
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
                        "&File/&Save SQL File...\t" => {
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
                        "&File/E&xit\t" => {
                            app::quit();
                        }
                        "&Query/&Execute\t" => {
                            sql_editor.execute_current();
                        }
                        "&Query/Execute &Selected\t" => {
                            sql_editor.execute_selected();
                        }
                        "&Query/E&xplain Plan\t" => {
                            sql_editor.explain_current();
                        }
                        "&Query/&Commit\t" => {
                            sql_editor.commit();
                        }
                        "&Query/&Rollback\t" => {
                            sql_editor.rollback();
                        }
                        "&Tools/&Refresh Objects\t" => {
                            object_browser.refresh();
                        }
                        "&Tools/&Export Results...\t" => {
                            let result = last_result.borrow();
                            if let Some(result) = result.as_ref() {
                                if !result.is_select {
                                    fltk::dialog::alert_default(
                                        "No query results to export (last query was not a SELECT).",
                                    );
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

                                    if let Err(err) = Self::export_results_csv(&path, result) {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to export results: {}",
                                            err
                                        ));
                                    } else {
                                        fltk::dialog::message_default(&format!(
                                            "Exported results to {}",
                                            path.display()
                                        ));
                                    }
                                }
                            } else {
                                fltk::dialog::alert_default("No query results to export.");
                            }
                        }
                        "&Tools/&Query History...\t" => {
                            let history = query_history.borrow();
                            let formatted = Self::format_query_history(&history);
                            fltk::dialog::message_default(&formatted);
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    pub fn show(&mut self) {
        self.window.show();
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

    fn csv_row(values: &[String]) -> String {
        values
            .iter()
            .map(|value| Self::csv_escape(value))
            .collect::<Vec<String>>()
            .join(",")
    }

    fn csv_escape(value: &str) -> String {
        if value.contains(',') || value.contains('"') || value.contains('\n') {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }

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
