use fltk::{
    app,
    dialog::{FileDialog, FileDialogType},
    enums::{Color, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    menu::MenuBar,
    prelude::*,
    text::TextBuffer,
    window::Window,
};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;

use crate::db::{
    create_shared_connection, lock_connection, ObjectBrowser, QueryResult, SharedConnection,
};
use crate::ui::{
    ConnectionDialog, FindReplaceDialog, HighlightData, IntellisenseData,
    MenuBarBuilder, ObjectBrowserWidget, QueryHistoryDialog, QueryProgress, ResultTabsWidget,
    SqlAction, SqlEditorWidget,
};
use crate::utils::QueryHistory;

#[derive(Clone)]
struct SchemaUpdate {
    data: IntellisenseData,
    highlight_data: HighlightData,
}

pub struct AppState {
    pub connection: SharedConnection,
    pub sql_editor: SqlEditorWidget,
    pub sql_buffer: TextBuffer,
    pub result_tabs: ResultTabsWidget,
    pub object_browser: ObjectBrowserWidget,
    pub status_bar: Frame,
    pub current_file: Rc<RefCell<Option<PathBuf>>>,
    pub last_result: Rc<RefCell<Option<QueryResult>>>,
    pub query_history: Rc<RefCell<QueryHistory>>,
    pub popups: Rc<RefCell<Vec<Window>>>,
    pub window: Window,
}

pub struct MainWindow {
    state: Rc<RefCell<AppState>>,
}

#[derive(Clone)]
enum ConnectionResult {
    Success(crate::db::ConnectionInfo),
    Failure(String),
}

impl MainWindow {
    fn schedule_awake(handle: app::TimeoutHandle) {
        app::awake();
        app::repeat_timeout3(0.1, handle);
    }

    pub fn new() -> Self {
        let connection = create_shared_connection();
        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition");
        window.set_color(Color::from_rgb(30, 30, 30));

        let mut main_flex = Flex::default_fill();
        main_flex.set_type(FlexType::Column);

        let menu_bar = MenuBarBuilder::build();
        main_flex.fixed(&menu_bar, 30);

        let mut content_flex = Flex::default();
        content_flex.set_type(FlexType::Row);

        let object_browser = ObjectBrowserWidget::new(0, 0, 250, 600, connection.clone());
        let obj_browser_widget = object_browser.get_widget();
        content_flex.fixed(&obj_browser_widget, 250);

        let mut right_flex = Flex::default();
        right_flex.set_type(FlexType::Column);

        let sql_editor = SqlEditorWidget::new(connection.clone());
        let sql_group = sql_editor.get_group().clone();
        right_flex.fixed(&sql_group, 250);

        let result_tabs = ResultTabsWidget::new(0, 0, 900, 400);
        let result_widget = result_tabs.get_widget();
        right_flex.add(&result_widget);
        right_flex.resizable(&result_widget);
        right_flex.end();

        content_flex.resizable(&right_flex);
        content_flex.end();
        main_flex.resizable(&content_flex);

        let mut status_bar = Frame::default().with_label("Not connected | Ctrl+Space for autocomplete");
        status_bar.set_frame(FrameType::FlatBox);
        status_bar.set_color(Color::from_rgb(0, 120, 212));
        status_bar.set_label_color(Color::White);
        main_flex.fixed(&status_bar, 25);
        main_flex.end();
        window.end();
        window.make_resizable(true);

        let sql_buffer = sql_editor.get_buffer();
        let state = Rc::new(RefCell::new(AppState {
            connection,
            sql_editor: sql_editor.clone(),
            sql_buffer,
            result_tabs,
            object_browser,
            status_bar,
            current_file: Rc::new(RefCell::new(None)),
            last_result: Rc::new(RefCell::new(None)),
            query_history: Rc::new(RefCell::new(QueryHistory::load())),
            popups: Rc::new(RefCell::new(Vec::new())),
            window,
        }));

        Self { state }
    }

    pub fn setup_callbacks(&mut self) {
        let state = self.state.clone();
        let mut state_borrow = state.borrow_mut();
        
        // Setup SQL editor execute callback
        let state_for_execute = state.clone();
        state_borrow.sql_editor.set_execute_callback(move |query_result| {
            let mut s = state_for_execute.borrow_mut();
            *s.last_result.borrow_mut() = Some(query_result.clone());
            let status_text = format!(
                "{} | Time: {:.3}s",
                query_result.message,
                query_result.execution_time.as_secs_f64()
            );
            s.status_bar.set_label(&status_text);
        });

        // Setup object browser callback
        let state_for_browser = state.clone();
        state_borrow.object_browser.set_sql_callback(move |action| {
            let mut s = state_for_browser.borrow_mut();
            match action {
                SqlAction::Set(sql) => {
                    s.sql_buffer.set_text(&sql);
                    s.sql_editor.get_highlighter().borrow().highlight(&sql, &mut s.sql_editor.get_style_buffer().clone());
                }
                SqlAction::Insert(text) => {
                    let mut editor = s.sql_editor.get_editor();
                    let insert_pos = editor.insert_position();
                    s.sql_buffer.insert(insert_pos, &text);
                    editor.set_insert_position(insert_pos + text.len() as i32);
                    s.sql_editor.refresh_highlighting();
                }
            }
        });

        let state_for_progress = state.clone();
        state_borrow.sql_editor.set_progress_callback(move |progress| {
            let mut s = state_for_progress.borrow_mut();
            match progress {
                QueryProgress::BatchStart => { s.result_tabs.clear(); }
                QueryProgress::StatementStart { index } => { s.result_tabs.start_statement(index, &format!("Result {}", index + 1)); }
                QueryProgress::SelectStart { index, columns } => { s.result_tabs.start_streaming(index, &columns); }
                QueryProgress::Rows { index, rows } => { s.result_tabs.append_rows(index, rows); }
                QueryProgress::StatementFinished { index, result, .. } => {
                    if result.is_select {
                        s.result_tabs.finish_streaming(index);
                    } else {
                        s.result_tabs.display_result(index, &result);
                    }
                }
                QueryProgress::BatchFinished => { s.result_tabs.finish_all_streaming(); }
            }
        });

        self.setup_menu_callbacks();
    }

    fn setup_menu_callbacks(&mut self) {
        let state = self.state.clone();
        let (schema_sender, schema_receiver) = std::sync::mpsc::channel::<SchemaUpdate>();
        let (conn_sender, conn_receiver) = std::sync::mpsc::channel::<ConnectionResult>();

        let state_for_schema = state.clone();
        let schema_sender_for_conn = schema_sender.clone();
        app::add_idle3(move |_| {
            // Check for schema updates
            while let Ok(update) = schema_receiver.try_recv() {
                let s = state_for_schema.borrow();
                *s.sql_editor.get_intellisense_data().borrow_mut() = update.data;
                s.sql_editor.get_highlighter().borrow_mut().set_highlight_data(update.highlight_data);
            }

            // Check for connection results
            while let Ok(result) = conn_receiver.try_recv() {
                let mut s = state_for_schema.borrow_mut();
                match result {
                    ConnectionResult::Success(info) => {
                        s.status_bar.set_label(&format!("Connected: {} | Ctrl+Space for autocomplete", info.display_string()));
                        s.object_browser.refresh();
                        s.sql_editor.focus();

                        // Start schema update after successful connection
                        let schema_sender = schema_sender_for_conn.clone();
                        let connection = s.connection.clone();
                        thread::spawn(move || {
                            let conn_guard = lock_connection(&connection);
                            if let Some(conn) = conn_guard.get_connection() {
                                let mut data = IntellisenseData::new();
                                let mut highlight_data = HighlightData::new();
                                if let Ok(tables) = ObjectBrowser::get_tables(conn.as_ref()) {
                                    highlight_data.tables = tables.clone();
                                    data.tables = tables;
                                }
                                if let Ok(views) = ObjectBrowser::get_views(conn.as_ref()) {
                                    highlight_data.views = views.clone();
                                    data.views = views;
                                }
                                let _ = schema_sender.send(SchemaUpdate { data, highlight_data });
                                app::awake();
                            }
                        });
                    }
                    ConnectionResult::Failure(err) => {
                        s.status_bar.set_label("Connection failed");
                        fltk::dialog::alert_default(&format!("Connection failed: {}", err));
                    }
                }
            }
        });

        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            let state_for_menu = state.clone();
            menu.set_callback(move |m| {
                let menu_path = m.item_pathname(None).ok().or_else(|| m.choice().map(|p| p.to_string()));
                if let Some(path) = menu_path {
                    let choice = path.split('\t').next().unwrap_or(&path).trim().replace('&', "");
                    match choice.as_str() {
                        "File/Connect..." => {
                            let (popups, connection) = {
                                let s = state_for_menu.borrow();
                                (s.popups.clone(), s.connection.clone())
                            };
                            if let Some(info) = ConnectionDialog::show_with_registry(popups) {
                                let conn_sender = conn_sender.clone();
                                {
                                    let mut s = state_for_menu.borrow_mut();
                                    s.status_bar.set_label(&format!("Connecting: {}...", info.display_string()));
                                }
                                thread::spawn(move || {
                                    let mut db_conn = lock_connection(&connection);
                                    match db_conn.connect(info.clone()) {
                                        Ok(_) => {
                                            let _ = conn_sender.send(ConnectionResult::Success(info));
                                            app::awake();
                                        }
                                        Err(e) => {
                                            let _ = conn_sender.send(ConnectionResult::Failure(e.to_string()));
                                            app::awake();
                                        }
                                    }
                                });
                            }
                        }
                        "File/Disconnect" => {
                            let connection = state_for_menu.borrow().connection.clone();
                            let mut db_conn = lock_connection(&connection);
                            db_conn.disconnect();
                            drop(db_conn);
                            
                            let mut s = state_for_menu.borrow_mut();
                            s.status_bar.set_label("Disconnected | Ctrl+Space for autocomplete");
                            *s.sql_editor.get_intellisense_data().borrow_mut() = IntellisenseData::new();
                            s.sql_editor.get_highlighter().borrow_mut().set_highlight_data(HighlightData::new());
                        }
                        "File/Open SQL File..." => {
                            let mut dialog = FileDialog::new(FileDialogType::BrowseFile);
                            dialog.show();
                            let filename = dialog.filename();
                            if !filename.as_os_str().is_empty() {
                                if let Ok(content) = fs::read_to_string(&filename) {
                                    let mut s = state_for_menu.borrow_mut();
                                    s.sql_buffer.set_text(&content);
                                    *s.current_file.borrow_mut() = Some(filename.clone());
                                    s.window.set_label(&format!("Oracle Query Tool - {}", filename.file_name().unwrap_or_default().to_string_lossy()));
                                    s.sql_editor.get_highlighter().borrow().highlight(&content, &mut s.sql_editor.get_style_buffer().clone());
                                    s.sql_editor.focus();
                                }
                            }
                        }
                        "File/Exit" => app::quit(),
                        "Edit/Undo" => state_for_menu.borrow_mut().sql_editor.get_editor().undo(),
                        "Edit/Redo" => state_for_menu.borrow_mut().sql_editor.get_editor().redo(),
                        "Edit/Cut" => state_for_menu.borrow_mut().sql_editor.get_editor().cut(),
                        "Edit/Copy" => {
                            let mut s = state_for_menu.borrow_mut();
                            let result_tabs_widget = s.result_tabs.get_widget();
                            let focus_in_results = if let Some(focus) = app::focus() {
                                focus.as_widget_ptr() == result_tabs_widget.as_widget_ptr() || 
                                focus.inside(&result_tabs_widget)
                            } else {
                                false
                            };

                            if focus_in_results {
                                let cell_count = s.result_tabs.copy();
                                if cell_count > 0 {
                                    s.status_bar.set_label(&format!("Copied {} cells to clipboard", cell_count));
                                } else {
                                    s.status_bar.set_label("No cells selected to copy");
                                }
                            } else {
                                s.sql_editor.get_editor().copy();
                            }
                        }
                        "Edit/Paste" => state_for_menu.borrow_mut().sql_editor.get_editor().paste(),
                        "Edit/Select All" => {
                            let mut s = state_for_menu.borrow_mut();
                            let result_tabs_widget = s.result_tabs.get_widget();
                            let focus_in_results = if let Some(focus) = app::focus() {
                                focus.as_widget_ptr() == result_tabs_widget.as_widget_ptr() ||
                                focus.inside(&result_tabs_widget)
                            } else {
                                false
                            };

                            if focus_in_results {
                                s.result_tabs.select_all();
                            } else {
                                let len = s.sql_buffer.length();
                                s.sql_buffer.select(0, len);
                            }
                        }
                        "Query/Execute" => state_for_menu.borrow_mut().sql_editor.execute_current(),
                        "Query/Commit" => state_for_menu.borrow_mut().sql_editor.commit(),
                        "Query/Rollback" => state_for_menu.borrow_mut().sql_editor.rollback(),
                        "Tools/Refresh Objects" => state_for_menu.borrow_mut().object_browser.refresh(),
                        "Edit/Find..." => {
                            let (mut editor, mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
                            };
                            FindReplaceDialog::show_find_with_registry(&mut editor, &mut buffer, popups);
                        }
                        "Edit/Replace..." => {
                            let (mut editor, mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
                            };
                            FindReplaceDialog::show_replace_with_registry(&mut editor, &mut buffer, popups);
                        }
                        "Tools/Query History..." => {
                            let (mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_buffer.clone(), s.popups.clone())
                            };
                            if let Some(sql) = QueryHistoryDialog::show_with_registry(popups) {
                                buffer.set_text(&sql);
                            }
                        },
                        _ => {}
                    }
                }
            });
        }
    }

    pub fn show(&mut self) {
        let state = self.state.clone();
        let mut s = state.borrow_mut();
        let popups = s.popups.clone();
        s.window.set_callback(move |w| {
            let mut popups = popups.borrow_mut();
            for mut popup in popups.drain(..) {
                popup.hide();
            }
            w.hide();
        });
        s.window.show();
        s.sql_editor.focus();
    }

    pub fn run() {
        let app = app::App::default().with_scheme(app::Scheme::Gtk);

        // Set default colors for dark theme
        app::background(45, 45, 48);
        app::foreground(220, 220, 220);
        app::add_timeout3(0.1, Self::schedule_awake);

        let mut main_window = MainWindow::new();
        main_window.setup_callbacks();
        main_window.show();

        match app.run() {
            Ok(()) => {}
            Err(err) => {
                eprintln!("Failed to run app: {err}");
            }
        }
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

        match fs::write(path, output) {
            Ok(()) => {}
            Err(err) => {
                eprintln!("CSV export error: {err}");
                return Err(Box::new(err));
            }
        }
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
