use fltk::{
    app,
    button::Button,
    dialog::{FileDialog, FileDialogType},
    enums::FrameType,
    frame::Frame,
    group::{Flex, FlexType},
    menu::MenuBar,
    prelude::*,
    text::TextBuffer,
    window::Window,
};
use std::cell::RefCell;
use std::collections::HashMap;
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
use crate::ui::theme;
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
    pub result_tab_offset: usize,
    pub object_browser: ObjectBrowserWidget,
    pub status_bar: Frame,
    pub fetch_row_counts: HashMap<usize, usize>,
    pub current_file: Rc<RefCell<Option<PathBuf>>>,
    pub last_result: Rc<RefCell<Option<QueryResult>>>,
    pub popups: Rc<RefCell<Vec<Window>>>,
    pub window: Window,
    pub right_flex: Flex,
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
    pub fn new() -> Self {
        let connection = create_shared_connection();

        fltk::group::Group::set_current(None::<&fltk::group::Group>);
        
        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition")
            .center_screen();
        window.set_color(theme::window_bg());

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

        let mut result_toolbar = Flex::default();
        result_toolbar.set_type(FlexType::Row);
        result_toolbar.set_margin(6);
        result_toolbar.set_spacing(5);

        let spacer = Frame::default();
        result_toolbar.resizable(&spacer);

        let mut clear_tabs_btn = Button::default().with_size(110, 25).with_label("Clear Tabs");
        clear_tabs_btn.set_color(theme::button_subtle());
        clear_tabs_btn.set_label_color(theme::text_secondary());
        clear_tabs_btn.set_frame(FrameType::RFlatBox);
        clear_tabs_btn.set_tooltip("Remove all result tabs");
        result_toolbar.fixed(&clear_tabs_btn, 110);

        result_toolbar.end();
        right_flex.fixed(&result_toolbar, 34);

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
        status_bar.set_color(theme::accent());
        status_bar.set_label_color(theme::text_primary());
        main_flex.fixed(&status_bar, 25);
        main_flex.end();
        window.end();
        window.make_resizable(true);

        let sql_buffer = sql_editor.get_buffer();
        let result_tabs_for_clear = result_tabs.clone();
        let sql_editor_for_clear = sql_editor.clone();
        clear_tabs_btn.set_callback(move |_| {
            if sql_editor_for_clear.is_query_running() {
                fltk::dialog::alert_default("A query is running. Stop it before clearing tabs.");
                return;
            }
            let mut tabs = result_tabs_for_clear.clone();
            tabs.clear();
        });

        let state = Rc::new(RefCell::new(AppState {
            connection,
            sql_editor: sql_editor.clone(),
            sql_buffer,
            result_tabs,
            result_tab_offset: 0,
            object_browser,
            status_bar,
            fetch_row_counts: HashMap::new(),
            current_file: Rc::new(RefCell::new(None)),
            last_result: Rc::new(RefCell::new(None)),
            popups: Rc::new(RefCell::new(Vec::new())),
            window,
            right_flex: right_flex.clone(),
        }));

        {
            let mut state_borrow = state.borrow_mut();
            Self::adjust_query_layout(&mut state_borrow);
        }

        Self { state }
    }

    fn adjust_query_layout(state: &mut AppState) {
        let mut right_flex = state.right_flex.clone();
        let sql_group = state.sql_editor.get_group();
        Self::adjust_query_layout_with(&mut right_flex, &sql_group);
    }

    fn adjust_query_layout_with(right_flex: &mut fltk::group::Flex, sql_group: &fltk::group::Flex) {
        let right_height = right_flex.h();
        if right_height <= 0 {
            return;
        }
        let desired_height = ((right_height as f32) * 0.4).round() as i32;
        right_flex.fixed(sql_group, desired_height);
        right_flex.layout();
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

        let state_for_status = state.clone();
        state_borrow.sql_editor.set_status_callback(move |message| {
            state_for_status.borrow_mut().status_bar.set_label(message);
        });

        let state_for_find = state.clone();
        state_borrow.sql_editor.set_find_callback(move || {
            let (mut editor, mut buffer, popups) = {
                let s = state_for_find.borrow_mut();
                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
            };
            FindReplaceDialog::show_find_with_registry(&mut editor, &mut buffer, popups);
        });

        let state_for_replace = state.clone();
        state_borrow.sql_editor.set_replace_callback(move || {
            let (mut editor, mut buffer, popups) = {
                let s = state_for_replace.borrow_mut();
                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
            };
            FindReplaceDialog::show_replace_with_registry(&mut editor, &mut buffer, popups);
        });

        // Setup object browser callback
        let state_for_browser = state.clone();
        state_borrow.object_browser.set_sql_callback(move |action| {
            let mut s = state_for_browser.borrow_mut();
            match action {
                SqlAction::Set(sql) => {
                    s.sql_buffer.set_text(&sql);
                    s.sql_editor.refresh_highlighting();
                }
                SqlAction::Insert(text) => {
                    let mut editor = s.sql_editor.get_editor();
                    let insert_pos = editor.insert_position();
                    s.sql_buffer.insert(insert_pos, &text);
                    editor.set_insert_position(insert_pos + text.len() as i32);
                    s.sql_editor.refresh_highlighting();
                }
                SqlAction::Execute(sql) => {
                    s.sql_editor.execute_sql_text(&sql);
                }
            }
        });

        let state_for_window = state.clone();
        state_borrow.window.handle(move |_w, ev| match ev {
            fltk::enums::Event::KeyDown => {
                if app::event_key() == fltk::enums::Key::Escape {
                    return true;
                }
                false
            }
            fltk::enums::Event::Push => {
                let sql_editor = {
                    let s = state_for_window.borrow();
                    s.sql_editor.clone()
                };
                sql_editor.hide_intellisense_if_outside(app::event_x_root(), app::event_y_root());
                false
            }
            fltk::enums::Event::Resize => {
                if let Ok(s) = state_for_window.try_borrow() {
                    let mut right_flex = s.right_flex.clone();
                    let sql_group = s.sql_editor.get_group().clone();
                    MainWindow::adjust_query_layout_with(&mut right_flex, &sql_group);
                }
                false
            }
            _ => false,
        });

        let state_for_progress = state.clone();
        state_borrow.sql_editor.set_progress_callback(move |progress| {
            let mut s = state_for_progress.borrow_mut();
            match progress {
                QueryProgress::BatchStart => {
                    s.result_tab_offset = s.result_tabs.tab_count();
                    s.fetch_row_counts.clear();
                }
                QueryProgress::StatementStart { index } => {
                    let tab_index = s.result_tab_offset + index;
                    s.result_tabs
                        .start_statement(tab_index, &format!("Result {}", tab_index + 1));
                    s.fetch_row_counts.remove(&index);
                }
                QueryProgress::SelectStart { index, columns } => {
                    let tab_index = s.result_tab_offset + index;
                    s.result_tabs.start_streaming(tab_index, &columns);
                    s.fetch_row_counts.insert(index, 0);
                    s.status_bar.set_label("Fetching rows: 0");
                }
                QueryProgress::Rows { index, rows } => {
                    let tab_index = s.result_tab_offset + index;
                    let rows_len = rows.len();
                    s.result_tabs.append_rows(tab_index, rows);
                    let new_count = {
                        let count = s.fetch_row_counts.entry(index).or_insert(0);
                        *count += rows_len;
                        *count
                    };
                    s.status_bar
                        .set_label(&format!("Fetching rows: {}", new_count));
                }
                QueryProgress::ScriptOutput { lines } => {
                    s.result_tabs.append_script_output_lines(&lines);
                }
                QueryProgress::PromptInput { .. } => {}
                QueryProgress::StatementFinished { index, result, .. } => {
                    let tab_index = s.result_tab_offset + index;
                    if result.is_select {
                        s.result_tabs.finish_streaming(tab_index);
                    } else {
                        s.result_tabs.display_result(tab_index, &result);
                    }
                    s.fetch_row_counts.remove(&index);
                }
                QueryProgress::BatchFinished => {
                    s.result_tabs.finish_all_streaming();
                    s.fetch_row_counts.clear();
                }
            }
        });

        self.setup_menu_callbacks();
    }

    fn setup_menu_callbacks(&mut self) {
        let state = self.state.clone();
        let (schema_sender, schema_receiver) = std::sync::mpsc::channel::<SchemaUpdate>();
        let (conn_sender, conn_receiver) = std::sync::mpsc::channel::<ConnectionResult>();

        // Wrap receivers in Rc<RefCell> to share across timeout callbacks
        let schema_receiver: Rc<RefCell<std::sync::mpsc::Receiver<SchemaUpdate>>> =
            Rc::new(RefCell::new(schema_receiver));
        let conn_receiver: Rc<RefCell<std::sync::mpsc::Receiver<ConnectionResult>>> =
            Rc::new(RefCell::new(conn_receiver));

        fn schedule_poll(
            schema_receiver: Rc<RefCell<std::sync::mpsc::Receiver<SchemaUpdate>>>,
            conn_receiver: Rc<RefCell<std::sync::mpsc::Receiver<ConnectionResult>>>,
            state: Rc<RefCell<AppState>>,
            schema_sender: std::sync::mpsc::Sender<SchemaUpdate>,
        ) {
            let mut schema_disconnected = false;
            let mut conn_disconnected = false;

            // Check for schema updates
            {
                let r = schema_receiver.borrow();
                loop {
                    match r.try_recv() {
                        Ok(update) => {
                            let s = state.borrow();
                            let mut data = update.data;
                            data.rebuild_indices();
                            *s.sql_editor.get_intellisense_data().borrow_mut() = data;
                            s.sql_editor.get_highlighter().borrow_mut().set_highlight_data(update.highlight_data);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            schema_disconnected = true;
                            break;
                        }
                    }
                }
            }

            // Check for connection results
            {
                let r = conn_receiver.borrow();
                loop {
                    match r.try_recv() {
                        Ok(result) => {
                            let mut s = state.borrow_mut();
                            match result {
                                ConnectionResult::Success(info) => {
                                    s.status_bar.set_label(&format!("Connected: {} | Ctrl+Space for autocomplete", info.display_string()));
                                    s.object_browser.refresh();
                                    s.sql_editor.focus();

                                    // Start schema update after successful connection
                                    let schema_sender = schema_sender.clone();
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
                                            data.rebuild_indices();
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
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            conn_disconnected = true;
                            break;
                        }
                    }
                }
            }

            // Stop polling if both channels are disconnected
            if schema_disconnected && conn_disconnected {
                return;
            }

            // Reschedule for next poll
            app::add_timeout3(0.05, move |_| {
                schedule_poll(
                    Rc::clone(&schema_receiver),
                    Rc::clone(&conn_receiver),
                    Rc::clone(&state),
                    schema_sender.clone(),
                );
            });
        }

        // Start polling
        let state_for_poll = state.clone();
        let schema_sender_for_poll = schema_sender.clone();
        schedule_poll(schema_receiver, conn_receiver, state_for_poll, schema_sender_for_poll);

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
                            dialog.set_filter("SQL Files\t*.sql\nAll Files\t*.*");
                            dialog.show();
                            let filename = dialog.filename();
                            if !filename.as_os_str().is_empty() {
                                if let Ok(content) = fs::read_to_string(&filename) {
                                    let mut s = state_for_menu.borrow_mut();
                                    s.sql_buffer.set_text(&content);
                                    *s.current_file.borrow_mut() = Some(filename.clone());
                                    s.window.set_label(&format!("Oracle Query Tool - {}", filename.file_name().unwrap_or_default().to_string_lossy()));
                                    s.sql_editor.refresh_highlighting();
                                    s.sql_editor.focus();
                                }
                            }
                        }
                        "File/Save SQL File..." => {
                            let (current_file, sql_text) = {
                                let s = state_for_menu.borrow();
                                let current_file = s.current_file.borrow().clone();
                                let sql_text = s.sql_buffer.text();
                                (current_file, sql_text)
                            };

                            let target_path = if let Some(path) = current_file {
                                Some(path)
                            } else {
                                let mut dialog = FileDialog::new(FileDialogType::BrowseSaveFile);
                                dialog.set_filter("SQL Files\t*.sql\nAll Files\t*.*");
                                dialog.show();
                                let filename = dialog.filename();
                                if filename.as_os_str().is_empty() {
                                    None
                                } else {
                                    Some(filename)
                                }
                            };

                            if let Some(path) = target_path {
                                if let Err(err) = fs::write(&path, sql_text) {
                                    fltk::dialog::alert_default(&format!(
                                        "Failed to save SQL file: {}",
                                        err
                                    ));
                                } else {
                                    let mut s = state_for_menu.borrow_mut();
                                    *s.current_file.borrow_mut() = Some(path.clone());
                                    let file_label = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy();
                                    s.window.set_label(&format!(
                                        "Oracle Query Tool - {}",
                                        file_label
                                    ));
                                    s.status_bar.set_label(&format!("Saved {}", file_label));
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
                        "Edit/Copy with Headers" => {
                            let mut s = state_for_menu.borrow_mut();
                            let result_tabs_widget = s.result_tabs.get_widget();
                            let focus_in_results = if let Some(focus) = app::focus() {
                                focus.as_widget_ptr() == result_tabs_widget.as_widget_ptr() ||
                                focus.inside(&result_tabs_widget)
                            } else {
                                false
                            };

                            if focus_in_results {
                                s.result_tabs.copy_with_headers();
                                s.status_bar.set_label("Copied selection with headers");
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
                        "Query/Execute Statement" => state_for_menu
                            .borrow_mut()
                            .sql_editor
                            .execute_statement_at_cursor(),
                        "Query/Execute Selected" => state_for_menu.borrow_mut().sql_editor.execute_selected(),
                        "Query/Quick Describe" => {
                            state_for_menu.borrow_mut().sql_editor.quick_describe_at_cursor();
                        }
                        "Query/Explain Plan" => state_for_menu.borrow_mut().sql_editor.explain_current(),
                        "Query/Commit" => state_for_menu.borrow_mut().sql_editor.commit(),
                        "Query/Rollback" => state_for_menu.borrow_mut().sql_editor.rollback(),
                        "Tools/Refresh Objects" => state_for_menu.borrow_mut().object_browser.refresh(),
                        "Tools/Export Results..." => {
                            let has_data = state_for_menu.borrow().result_tabs.has_data();
                            if !has_data {
                                fltk::dialog::alert_default("No results to export");
                                return;
                            }

                            let mut dialog = FileDialog::new(FileDialogType::BrowseSaveFile);
                            dialog.set_filter("CSV Files\t*.csv");
                            dialog.show();
                            let filename = dialog.filename();
                            if filename.as_os_str().is_empty() {
                                return;
                            }

                            let csv = state_for_menu.borrow().result_tabs.export_to_csv();
                            if let Err(err) = fs::write(&filename, csv) {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to export CSV: {}",
                                    err
                                ));
                            } else {
                                let row_count = state_for_menu.borrow().result_tabs.row_count();
                                let mut s = state_for_menu.borrow_mut();
                                let file_label = filename
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                s.status_bar.set_label(&format!(
                                    "Exported {} rows to {}",
                                    row_count, file_label
                                ));
                            }
                        }
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
                        "Edit/Format SQL" => {
                            state_for_menu.borrow_mut().sql_editor.format_selected_sql();
                        }
                        "Edit/Toggle Comment" => {
                            state_for_menu.borrow_mut().sql_editor.toggle_comment();
                        }
                        "Edit/Uppercase Selection" => {
                            state_for_menu
                                .borrow_mut()
                                .sql_editor
                                .convert_selection_case(true);
                        }
                        "Edit/Lowercase Selection" => {
                            state_for_menu
                                .borrow_mut()
                                .sql_editor
                                .convert_selection_case(false);
                        }
                        "Edit/Intellisense" => {
                            state_for_menu.borrow().sql_editor.show_intellisense();
                        }
                        "Tools/Query History..." => {
                            let (mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_buffer.clone(), s.popups.clone())
                            };
                            if let Some(sql) = QueryHistoryDialog::show_with_registry(popups) {
                                buffer.set_text(&sql);
                            }
                        }
                        "Tools/Feature Catalog..." => {
                            FeatureCatalogDialog::show();
                        }
                        "Tools/Auto-Commit" => {
                            let enabled = m
                                .find_item("&Tools/&Auto-Commit\t")
                                .map(|item| item.value())
                                .unwrap_or(false);
                            let status = if enabled {
                                "Auto-commit enabled"
                            } else {
                                "Auto-commit disabled"
                            };
                            {
                                let s = state_for_menu.borrow_mut();
                                let mut connection = lock_connection(&s.connection);
                                connection.set_auto_commit(enabled);
                            }
                            state_for_menu.borrow_mut().status_bar.set_label(status);
                        }
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
        let sql_editor = s.sql_editor.clone();
        s.window.set_callback(move |w| {
            let mut popups = popups.borrow_mut();
            for mut popup in popups.drain(..) {
                popup.hide();
            }
            sql_editor.hide_intellisense();
            w.hide();
            app::quit();
        });
        s.window.show();
        app::flush();
        let _ = app::wait();
        MainWindow::adjust_query_layout(&mut s);
        s.window.redraw();
        s.sql_editor.focus();
    }

    pub fn run() {
        let app = app::App::default().with_scheme(app::Scheme::Gtk);
        app::set_font_size(14);
        fltk::misc::Tooltip::set_font_size(14);

        // Set default colors for Windows 11-inspired theme
        let (bg_r, bg_g, bg_b) = theme::app_background().to_rgb();
        app::background(bg_r, bg_g, bg_b);
        let (fg_r, fg_g, fg_b) = theme::app_foreground().to_rgb();
        app::foreground(fg_r, fg_g, fg_b);

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
