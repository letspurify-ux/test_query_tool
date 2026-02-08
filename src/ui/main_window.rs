use fltk::{
    app,
    button::Button,
    dialog::{FileDialog, FileDialogType},
    draw::set_cursor,
    enums::{Cursor, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    menu::MenuBar,
    prelude::*,
    text::TextBuffer,
    widget::Widget,
    window::Window,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::db::{
    create_shared_connection, lock_connection, ObjectBrowser, QueryResult, SharedConnection,
};
use crate::ui::constants::*;
use crate::ui::theme;
use crate::ui::{
    font_settings, show_settings_dialog, ConnectionDialog, FindReplaceDialog, HighlightData,
    IntellisenseData, MenuBarBuilder, ObjectBrowserWidget, QueryHistoryDialog, QueryProgress,
    ResultTabsWidget, SqlAction, SqlEditorWidget,
};
use crate::utils::{AppConfig, QueryHistory};

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
    pub popups: Rc<RefCell<Vec<Window>>>,
    pub window: Window,
    pub right_flex: Flex,
    pub query_split_adjusted: Rc<Cell<bool>>,
    pub connection_info: Rc<RefCell<Option<crate::db::ConnectionInfo>>>,
    pub config: Rc<RefCell<AppConfig>>,
    pub last_fetch_status_update: Instant,
}

const FETCH_STATUS_UPDATE_INTERVAL: Duration = Duration::from_millis(250);

/// 접속 정보를 상태 표시줄 메시지 끝에 붙는 헬퍼
fn format_status(msg: &str, conn_info: &Option<crate::db::ConnectionInfo>) -> String {
    match conn_info {
        Some(info) => format!("{} | {}", msg, info.display_string()),
        None => msg.to_string(),
    }
}

pub struct MainWindow {
    state: Rc<RefCell<AppState>>,
}

#[derive(Clone)]
enum ConnectionResult {
    Success(crate::db::ConnectionInfo),
    Failure(String),
}

enum FileActionResult {
    Open {
        path: PathBuf,
        result: Result<String, String>,
    },
    Save {
        path: PathBuf,
        result: Result<(), String>,
    },
    Export {
        path: PathBuf,
        row_count: usize,
        result: Result<(), String>,
    },
}

impl MainWindow {
    pub fn new() -> Self {
        let config = AppConfig::load();
        let connection = create_shared_connection();

        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition")
            .center_screen();
        window.set_id("main_window");
        window.set_color(theme::window_bg());

        let mut main_flex = Flex::default_fill();
        main_flex.set_type(FlexType::Column);

        let menu_bar = MenuBarBuilder::build();
        main_flex.fixed(&menu_bar, MENU_BAR_HEIGHT);

        let mut content_flex = Flex::default();
        content_flex.set_type(FlexType::Row);
        content_flex.set_spacing(0);

        let object_browser = ObjectBrowserWidget::new(0, 0, 250, 600, connection.clone());
        let obj_browser_widget = object_browser.get_widget();
        content_flex.fixed(&obj_browser_widget, 250);

        let splitter_width = MAIN_SPLITTER_WIDTH;
        let mut split_bar = Frame::default().with_size(splitter_width, 0);
        split_bar.set_frame(FrameType::FlatBox);
        split_bar.set_color(theme::border());
        split_bar.set_tooltip("Drag to resize panels");

        let drag_state = Rc::new(RefCell::new(None::<(i32, i32)>));
        let mut content_flex_for_split = content_flex.clone();
        let obj_browser_for_split = obj_browser_widget.clone();
        let drag_state_for_split = drag_state.clone();
        split_bar.handle(move |_bar, ev| match ev {
            fltk::enums::Event::Enter | fltk::enums::Event::Move => {
                set_cursor(Cursor::WE);
                true
            }
            fltk::enums::Event::Push => {
                *drag_state_for_split.borrow_mut() =
                    Some((app::event_x(), obj_browser_for_split.w()));
                true
            }
            fltk::enums::Event::Drag => {
                if let Some((start_x, start_w)) = *drag_state_for_split.borrow() {
                    let delta = app::event_x() - start_x;
                    let min_left = 180;
                    let min_right = 320;
                    let max_left =
                        (content_flex_for_split.w() - splitter_width - min_right).max(min_left);
                    let mut new_width = start_w + delta;
                    if new_width < min_left {
                        new_width = min_left;
                    } else if new_width > max_left {
                        new_width = max_left;
                    }
                    content_flex_for_split.fixed(&obj_browser_for_split, new_width);
                    content_flex_for_split.layout();
                    app::redraw();
                }
                true
            }
            fltk::enums::Event::Released => {
                *drag_state_for_split.borrow_mut() = None;
                set_cursor(Cursor::WE);
                true
            }
            fltk::enums::Event::Leave => {
                set_cursor(Cursor::Default);
                true
            }
            _ => false,
        });
        content_flex.fixed(&split_bar, splitter_width);

        let mut right_flex = Flex::default();
        right_flex.set_type(FlexType::Column);

        let sql_editor = SqlEditorWidget::new(connection.clone());
        let sql_group = sql_editor.get_group().clone();
        right_flex.fixed(&sql_group, 250);

        let query_split_adjusted = Rc::new(Cell::new(false));
        let mut query_split_bar = Frame::default().with_size(0, QUERY_SPLITTER_HEIGHT);
        query_split_bar.set_frame(FrameType::FlatBox);
        query_split_bar.set_color(theme::border());
        query_split_bar.set_tooltip("Drag to resize query/results");

        let query_drag_state = Rc::new(RefCell::new(None::<(i32, i32)>));
        let mut right_flex_for_query_split = right_flex.clone();
        let sql_group_for_split = sql_group.clone();
        let query_drag_state_for_split = query_drag_state.clone();
        let query_split_adjusted_for_split = query_split_adjusted.clone();
        query_split_bar.handle(move |_bar, ev| match ev {
            fltk::enums::Event::Enter | fltk::enums::Event::Move => {
                set_cursor(Cursor::NS);
                true
            }
            fltk::enums::Event::Push => {
                *query_drag_state_for_split.borrow_mut() =
                    Some((app::event_y(), sql_group_for_split.h()));
                true
            }
            fltk::enums::Event::Drag => {
                if let Some((start_y, start_h)) = *query_drag_state_for_split.borrow() {
                    let total_height = right_flex_for_query_split.h();
                    if total_height <= 0 {
                        return true;
                    }
                    let delta = app::event_y() - start_y;
                    let max_top = (total_height - QUERY_SPLITTER_HEIGHT - MIN_RESULTS_HEIGHT)
                        .max(MIN_QUERY_HEIGHT);
                    let mut new_height = start_h + delta;
                    if new_height < MIN_QUERY_HEIGHT {
                        new_height = MIN_QUERY_HEIGHT;
                    } else if new_height > max_top {
                        new_height = max_top;
                    }
                    right_flex_for_query_split.fixed(&sql_group_for_split, new_height);
                    right_flex_for_query_split.layout();
                    query_split_adjusted_for_split.set(true);
                    app::redraw();
                }
                true
            }
            fltk::enums::Event::Released => {
                *query_drag_state_for_split.borrow_mut() = None;
                set_cursor(Cursor::NS);
                true
            }
            fltk::enums::Event::Leave => {
                set_cursor(Cursor::Default);
                true
            }
            _ => false,
        });
        right_flex.fixed(&query_split_bar, QUERY_SPLITTER_HEIGHT);

        let mut result_toolbar = Flex::default();
        result_toolbar.set_type(FlexType::Row);
        result_toolbar.set_margin(TOOLBAR_SPACING);
        result_toolbar.set_spacing(TOOLBAR_SPACING);

        let mut close_tab_btn = Button::default()
            .with_size(BUTTON_WIDTH, BUTTON_HEIGHT)
            .with_label("Close Tab");
        close_tab_btn.set_color(theme::button_subtle());
        close_tab_btn.set_label_color(theme::text_secondary());
        close_tab_btn.set_frame(FrameType::RFlatBox);
        close_tab_btn.set_tooltip("Close the current result tab (Ctrl+W)");
        result_toolbar.fixed(&close_tab_btn, BUTTON_WIDTH);

        let mut clear_tabs_btn = Button::default()
            .with_size(BUTTON_WIDTH, BUTTON_HEIGHT)
            .with_label("Clear Tabs");
        clear_tabs_btn.set_color(theme::button_subtle());
        clear_tabs_btn.set_label_color(theme::text_secondary());
        clear_tabs_btn.set_frame(FrameType::RFlatBox);
        clear_tabs_btn.set_tooltip("Remove all result tabs");
        result_toolbar.fixed(&clear_tabs_btn, BUTTON_WIDTH);

        let mut query_history_btn = Button::default()
            .with_size(BUTTON_WIDTH, BUTTON_HEIGHT)
            .with_label("History");
        query_history_btn.set_color(theme::button_subtle());
        query_history_btn.set_label_color(theme::text_secondary());
        query_history_btn.set_frame(FrameType::RFlatBox);
        query_history_btn.set_tooltip("Open query history");
        result_toolbar.fixed(&query_history_btn, BUTTON_WIDTH);

        let spacer = Frame::default();
        result_toolbar.resizable(&spacer);

        result_toolbar.end();
        right_flex.fixed(&result_toolbar, RESULT_TOOLBAR_HEIGHT);

        let result_tabs = ResultTabsWidget::new(0, 0, 900, 400);
        let result_widget = result_tabs.get_widget();
        right_flex.add(&result_widget);
        right_flex.resizable(&result_widget);
        right_flex.end();

        content_flex.resizable(&right_flex);
        content_flex.end();
        main_flex.resizable(&content_flex);

        let mut status_bar = Frame::default().with_label("Not connected");
        status_bar.set_frame(FrameType::FlatBox);
        status_bar.set_color(theme::accent());
        status_bar.set_label_color(theme::text_primary());
        main_flex.fixed(&status_bar, STATUS_BAR_HEIGHT);
        main_flex.end();
        window.end();
        window.make_resizable(true);

        let sql_buffer = sql_editor.get_buffer();

        let result_tabs_for_close = result_tabs.clone();
        let sql_editor_for_close = sql_editor.clone();
        close_tab_btn.set_callback(move |_| {
            if sql_editor_for_close.is_query_running() {
                fltk::dialog::alert_default("A query is running. Stop it before closing tabs.");
                return;
            }
            let mut tabs = result_tabs_for_close.clone();
            tabs.close_current_tab();
            app::redraw();
        });

        let result_tabs_for_clear = result_tabs.clone();
        let sql_editor_for_clear = sql_editor.clone();
        clear_tabs_btn.set_callback(move |_| {
            if sql_editor_for_clear.is_query_running() {
                fltk::dialog::alert_default("A query is running. Stop it before clearing tabs.");
                return;
            }
            let mut tabs = result_tabs_for_clear.clone();
            tabs.clear();
            app::redraw();
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
            popups: Rc::new(RefCell::new(Vec::new())),
            window,
            right_flex: right_flex.clone(),
            query_split_adjusted: query_split_adjusted.clone(),
            connection_info: Rc::new(RefCell::new(None)),
            config: Rc::new(RefCell::new(config)),
            last_fetch_status_update: Instant::now(),
        }));

        {
            let mut state_borrow = state.borrow_mut();
            Self::adjust_query_layout(&mut state_borrow);
            Self::apply_font_settings(&mut state_borrow);
        }

        let weak_state_for_history_btn = Rc::downgrade(&state);
        query_history_btn.set_callback(move |_| {
            if let Some(state_for_history) = weak_state_for_history_btn.upgrade() {
                MainWindow::open_query_history_dialog(&state_for_history);
            }
        });

        Self { state }
    }

    fn open_query_history_dialog(state: &Rc<RefCell<AppState>>) {
        let (mut buffer, mut editor, popups) = {
            let s = state.borrow_mut();
            (s.sql_buffer.clone(), s.sql_editor.get_editor(), s.popups.clone())
        };
        if let Some(sql) = QueryHistoryDialog::show_with_registry(popups) {
            let buffer_length = buffer.length();
            let text_to_insert = if buffer_length > 0 {
                format!("\n{}", sql)
            } else {
                sql
            };
            buffer.insert(buffer_length, &text_to_insert);

            let new_length = buffer.length();
            editor.set_insert_position(new_length);
            editor.show_insert_position();

            state.borrow().sql_editor.refresh_highlighting();
        }
    }

    fn adjust_query_layout(state: &mut AppState) {
        let mut right_flex = state.right_flex.clone();
        let sql_group = state.sql_editor.get_group();
        if state.query_split_adjusted.get() {
            right_flex.layout();
        } else {
            Self::adjust_query_layout_with(&mut right_flex, &sql_group);
        }
    }

    fn apply_font_settings(state: &mut AppState) {
        let (unified_profile, ui_size, editor_size, result_size, result_cell_max_chars) = {
            let config = state.config.borrow();
            (
                font_settings::profile_by_name(&config.editor_font),
                config.ui_font_size.clamp(8, 24) as i32,
                config.editor_font_size,
                config.result_font_size,
                config.result_cell_max_chars.clamp(
                    RESULT_CELL_MAX_DISPLAY_CHARS_MIN,
                    RESULT_CELL_MAX_DISPLAY_CHARS_MAX,
                ),
            )
        };
        app::set_font(unified_profile.normal);
        app::set_font_size(ui_size);
        fltk::misc::Tooltip::set_font(unified_profile.normal);
        fltk::misc::Tooltip::set_font_size(ui_size);
        fltk::dialog::message_set_font(unified_profile.normal, ui_size);
        state
            .sql_editor
            .apply_font_settings(unified_profile, editor_size, ui_size);
        state
            .result_tabs
            .apply_font_settings(unified_profile, result_size);
        state
            .result_tabs
            .set_max_cell_display_chars(result_cell_max_chars as usize);
        state
            .object_browser
            .apply_font_settings(unified_profile, ui_size);
        Self::apply_runtime_ui_font(state, unified_profile.normal, ui_size);
        app::redraw();
        // Force FLTK to process the pending redraw immediately, so font
        // changes are visible right after the settings dialog closes
        // instead of requiring multiple save cycles.
        app::flush();
        app::awake();
    }

    fn apply_runtime_ui_font(
        state: &mut AppState,
        font: fltk::enums::Font,
        ui_size: i32,
    ) {
        fn apply_widget_font_recursive(
            widget: &mut Widget,
            font: fltk::enums::Font,
            size: i32,
        ) {
            widget.set_label_font(font);
            widget.set_label_size(size);
            if let Some(group) = widget.as_group() {
                for mut child in group.into_iter() {
                    apply_widget_font_recursive(&mut child, font, size);
                }
            }
        }

        let mut window = state.window.clone();
        window.set_label_font(font);
        window.set_label_size(ui_size);
        for mut child in window.clone().into_iter() {
            apply_widget_font_recursive(&mut child, font, ui_size);
        }

        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            menu.set_text_font(font);
            menu.set_text_size(ui_size);
        }

        for popup in state.popups.borrow_mut().iter_mut() {
            popup.set_label_font(font);
            popup.set_label_size(ui_size);
            for mut child in popup.clone().into_iter() {
                apply_widget_font_recursive(&mut child, font, ui_size);
            }
        }
    }

    fn adjust_query_layout_with(right_flex: &mut fltk::group::Flex, sql_group: &fltk::group::Flex) {
        let right_height = right_flex.h();
        if right_height <= 0 {
            return;
        }
        let max_height =
            (right_height - QUERY_SPLITTER_HEIGHT - MIN_RESULTS_HEIGHT).max(MIN_QUERY_HEIGHT);
        let mut desired_height = ((right_height as f32) * 0.4).round() as i32;
        if desired_height < MIN_QUERY_HEIGHT {
            desired_height = MIN_QUERY_HEIGHT;
        } else if desired_height > max_height {
            desired_height = max_height;
        }
        right_flex.fixed(sql_group, desired_height);
        right_flex.layout();
    }

    fn adjust_query_layout_on_resize(state: &AppState) {
        let mut right_flex = state.right_flex.clone();
        let sql_group = state.sql_editor.get_group();
        if state.query_split_adjusted.get() {
            right_flex.layout();
        } else {
            Self::adjust_query_layout_with(&mut right_flex, sql_group);
        }
    }

    pub fn setup_callbacks(&mut self) {
        let state = self.state.clone();
        let mut state_borrow = state.borrow_mut();
        let (schema_sender, schema_receiver) = std::sync::mpsc::channel::<SchemaUpdate>();

        // Setup SQL editor execute callback
        let weak_state_for_execute = Rc::downgrade(&state);
        state_borrow
            .sql_editor
            .set_execute_callback(move |query_result| {
                let Some(state_for_execute) = weak_state_for_execute.upgrade() else {
                    return;
                };
                let mut s = state_for_execute.borrow_mut();
                let conn_info = s.connection_info.borrow().clone();
                let base_msg = if query_result.success {
                    format!(
                        "{} | Time: {:.3}s",
                        query_result.message,
                        query_result.execution_time.as_secs_f64()
                    )
                } else {
                    format!(
                        "Error | Time: {:.3}s",
                        query_result.execution_time.as_secs_f64()
                    )
                };
                s.status_bar
                    .set_label(&format_status(&base_msg, &conn_info));
            });

        let weak_state_for_status = Rc::downgrade(&state);
        state_borrow.sql_editor.set_status_callback(move |message| {
            let Some(state_for_status) = weak_state_for_status.upgrade() else {
                return;
            };
            let mut s = state_for_status.borrow_mut();
            let conn_info = s.connection_info.borrow().clone();
            s.status_bar.set_label(&format_status(message, &conn_info));
        });

        let weak_state_for_find = Rc::downgrade(&state);
        state_borrow.sql_editor.set_find_callback(move || {
            let Some(state_for_find) = weak_state_for_find.upgrade() else {
                return;
            };
            let (mut editor, mut buffer, popups) = {
                let s = state_for_find.borrow();
                (
                    s.sql_editor.get_editor(),
                    s.sql_buffer.clone(),
                    s.popups.clone(),
                )
            };
            FindReplaceDialog::show_find_with_registry(&mut editor, &mut buffer, popups);
        });

        let weak_state_for_replace = Rc::downgrade(&state);
        state_borrow.sql_editor.set_replace_callback(move || {
            let Some(state_for_replace) = weak_state_for_replace.upgrade() else {
                return;
            };
            let (mut editor, mut buffer, popups) = {
                let s = state_for_replace.borrow();
                (
                    s.sql_editor.get_editor(),
                    s.sql_buffer.clone(),
                    s.popups.clone(),
                )
            };
            FindReplaceDialog::show_replace_with_registry(&mut editor, &mut buffer, popups);
        });

        // Setup object browser callback
        let weak_state_for_browser = Rc::downgrade(&state);
        state_borrow.object_browser.set_sql_callback(move |action| {
            let Some(state_for_browser) = weak_state_for_browser.upgrade() else {
                return;
            };
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
                SqlAction::Append(text) => {
                    let mut editor = s.sql_editor.get_editor();
                    let buffer_length = s.sql_buffer.length();

                    // Add newline prefix if buffer is not empty
                    let text_to_insert = if buffer_length > 0 {
                        format!("\n{}", text)
                    } else {
                        text
                    };

                    // Insert at the end of the buffer
                    s.sql_buffer.insert(buffer_length, &text_to_insert);

                    // Move cursor to the end
                    let new_length = s.sql_buffer.length();
                    editor.set_insert_position(new_length);

                    // Scroll to the bottom to show the inserted text
                    editor.show_insert_position();

                    s.sql_editor.refresh_highlighting();
                }
                SqlAction::Execute(sql) => {
                    s.sql_editor.execute_sql_text(&sql);
                }
            }
        });

        let weak_state_for_window = Rc::downgrade(&state);
        state_borrow.window.handle(move |_w, ev| {
            let Some(state_for_window) = weak_state_for_window.upgrade() else {
                return false;
            };
            match ev {
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
                    sql_editor
                        .hide_intellisense_if_outside(app::event_x_root(), app::event_y_root());
                    false
                }
                fltk::enums::Event::Resize => {
                    if let Ok(s) = state_for_window.try_borrow() {
                        MainWindow::adjust_query_layout_on_resize(&s);
                    }
                    false
                }
                _ => false,
            }
        });

        let weak_state_for_progress = Rc::downgrade(&state);
        let schema_sender_for_progress = schema_sender.clone();
        state_borrow
            .sql_editor
            .set_progress_callback(move |progress| {
                let Some(state_for_progress) = weak_state_for_progress.upgrade() else {
                    return;
                };
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
                        let conn_info = s.connection_info.borrow().clone();
                        s.status_bar
                            .set_label(&format_status("Executing query...", &conn_info));
                    }
                    QueryProgress::SelectStart { index, columns } => {
                        let tab_index = s.result_tab_offset + index;
                        s.result_tabs.start_streaming(tab_index, &columns);
                        s.fetch_row_counts.insert(index, 0);
                        s.last_fetch_status_update = Instant::now();
                        let conn_info = s.connection_info.borrow().clone();
                        s.status_bar
                            .set_label(&format_status("Fetching rows: 0", &conn_info));
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
                        if s.last_fetch_status_update.elapsed() >= FETCH_STATUS_UPDATE_INTERVAL {
                            let conn_info = s.connection_info.borrow().clone();
                            s.status_bar.set_label(&format_status(
                                &format!("Fetching rows: {}", new_count),
                                &conn_info,
                            ));
                            s.last_fetch_status_update = Instant::now();
                        }
                    }
                    QueryProgress::ScriptOutput { lines } => {
                        s.result_tabs.append_script_output_lines(&lines);
                    }
                    QueryProgress::PromptInput { .. } => {}
                    QueryProgress::AutoCommitChanged { enabled } => {
                        if let Some(menu) = app::widget_from_id::<MenuBar>("main_menu") {
                            if let Some(mut item) = menu.find_item("&Tools/&Auto-Commit\t") {
                                if enabled {
                                    item.set();
                                } else {
                                    item.clear();
                                }
                            }
                        }
                        let status = if enabled {
                            "Auto-commit enabled"
                        } else {
                            "Auto-commit disabled"
                        };
                        let conn_info = s.connection_info.borrow().clone();
                        s.status_bar.set_label(&format_status(status, &conn_info));
                    }
                    QueryProgress::ConnectionChanged { info } => {
                        if let Some(info) = info {
                            *s.connection_info.borrow_mut() = Some(info.clone());
                            s.status_bar
                                .set_label(&format!("Connected | {}", info.display_string()));
                            s.object_browser.refresh();
                            s.sql_editor.focus();

                            let schema_sender = schema_sender_for_progress.clone();
                            let connection = s.connection.clone();
                            thread::spawn(move || {
                                let conn = {
                                    let conn_guard = lock_connection(&connection);
                                    conn_guard.get_connection()
                                };
                                if let Some(conn) = conn {
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
                                    let _ = schema_sender.send(SchemaUpdate {
                                        data,
                                        highlight_data,
                                    });
                                    app::awake();
                                }
                            });
                        } else {
                            *s.connection_info.borrow_mut() = None;
                            s.status_bar.set_label("Disconnected");
                            *s.sql_editor.get_intellisense_data().borrow_mut() =
                                IntellisenseData::new();
                            s.sql_editor
                                .get_highlighter()
                                .borrow_mut()
                                .set_highlight_data(HighlightData::new());
                        }
                    }
                    QueryProgress::StatementFinished { index, result, .. } => {
                        let tab_index = s.result_tab_offset + index;
                        if !result.success && !result.message.trim().is_empty() {
                            let lines: Vec<String> =
                                result.message.lines().map(|l| l.to_string()).collect();
                            s.result_tabs.append_script_output_lines(&lines);
                            s.result_tabs.select_script_output();
                        }
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

        drop(state_borrow);
        self.setup_menu_callbacks(schema_sender, schema_receiver);
    }

    fn setup_menu_callbacks(
        &mut self,
        schema_sender: std::sync::mpsc::Sender<SchemaUpdate>,
        schema_receiver: std::sync::mpsc::Receiver<SchemaUpdate>,
    ) {
        let state = self.state.clone();
        let (conn_sender, conn_receiver) = std::sync::mpsc::channel::<ConnectionResult>();
        let (file_sender, file_receiver) = std::sync::mpsc::channel::<FileActionResult>();

        // Wrap receivers in Rc<RefCell> to share across timeout callbacks
        let schema_receiver: Rc<RefCell<std::sync::mpsc::Receiver<SchemaUpdate>>> =
            Rc::new(RefCell::new(schema_receiver));
        let conn_receiver: Rc<RefCell<std::sync::mpsc::Receiver<ConnectionResult>>> =
            Rc::new(RefCell::new(conn_receiver));
        let file_receiver: Rc<RefCell<std::sync::mpsc::Receiver<FileActionResult>>> =
            Rc::new(RefCell::new(file_receiver));

        fn schedule_poll(
            schema_receiver: Rc<RefCell<std::sync::mpsc::Receiver<SchemaUpdate>>>,
            conn_receiver: Rc<RefCell<std::sync::mpsc::Receiver<ConnectionResult>>>,
            file_receiver: Rc<RefCell<std::sync::mpsc::Receiver<FileActionResult>>>,
            state_weak: Weak<RefCell<AppState>>,
            schema_sender: std::sync::mpsc::Sender<SchemaUpdate>,
        ) {
            let Some(state) = state_weak.upgrade() else {
                return;
            };
            let mut schema_disconnected = false;
            let mut conn_disconnected = false;
            let mut file_disconnected = false;

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
                            s.sql_editor
                                .get_highlighter()
                                .borrow_mut()
                                .set_highlight_data(update.highlight_data);
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
                                    *s.connection_info.borrow_mut() = Some(info.clone());
                                    s.status_bar.set_label(&format!(
                                        "Connected | {}",
                                        info.display_string()
                                    ));
                                    s.object_browser.refresh();
                                    s.sql_editor.focus();

                                    // Start schema update after successful connection
                                    let schema_sender = schema_sender.clone();
                                    let connection = s.connection.clone();
                                    thread::spawn(move || {
                                        let conn = {
                                            let conn_guard = lock_connection(&connection);
                                            conn_guard.get_connection()
                                        };
                                        if let Some(conn) = conn {
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
                                            data.rebuild_indices();
                                            let _ = schema_sender.send(SchemaUpdate {
                                                data,
                                                highlight_data,
                                            });
                                            app::awake();
                                        }
                                    });
                                }
                                ConnectionResult::Failure(err) => {
                                    s.status_bar.set_label("Connection failed");
                                    s.result_tabs.append_script_output_lines(&[format!(
                                        "Connection failed: {}",
                                        err
                                    )]);
                                    s.result_tabs.select_script_output();
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

            // Check for file operations
            {
                let r = file_receiver.borrow();
                loop {
                    match r.try_recv() {
                        Ok(result) => {
                            let mut s = state.borrow_mut();
                            match result {
                                FileActionResult::Open { path, result } => match result {
                                    Ok(content) => {
                                        s.sql_buffer.set_text(&content);
                                        *s.current_file.borrow_mut() = Some(path.clone());
                                        s.window.set_label(&format!(
                                            "Oracle Query Tool - {}",
                                            path.file_name().unwrap_or_default().to_string_lossy()
                                        ));
                                        s.sql_editor.refresh_highlighting();
                                        s.sql_editor.focus();
                                    }
                                    Err(err) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to open SQL file: {}",
                                            err
                                        ));
                                    }
                                },
                                FileActionResult::Save { path, result } => match result {
                                    Ok(()) => {
                                        *s.current_file.borrow_mut() = Some(path.clone());
                                        let file_label =
                                            path.file_name().unwrap_or_default().to_string_lossy();
                                        s.window.set_label(&format!(
                                            "Oracle Query Tool - {}",
                                            file_label
                                        ));
                                        let conn_info = s.connection_info.borrow().clone();
                                        s.status_bar.set_label(&format_status(
                                            &format!("Saved {}", file_label),
                                            &conn_info,
                                        ));
                                    }
                                    Err(err) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to save SQL file: {}",
                                            err
                                        ));
                                    }
                                },
                                FileActionResult::Export {
                                    path,
                                    row_count,
                                    result,
                                } => match result {
                                    Ok(()) => {
                                        let file_label =
                                            path.file_name().unwrap_or_default().to_string_lossy();
                                        let conn_info = s.connection_info.borrow().clone();
                                        s.status_bar.set_label(&format_status(
                                            &format!(
                                                "Exported {} rows to {}",
                                                row_count, file_label
                                            ),
                                            &conn_info,
                                        ));
                                    }
                                    Err(err) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Failed to export CSV: {}",
                                            err
                                        ));
                                    }
                                },
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            file_disconnected = true;
                            break;
                        }
                    }
                }
            }

            // Stop polling if all channels are disconnected
            if schema_disconnected && conn_disconnected && file_disconnected {
                return;
            }

            // Reschedule for next poll
            app::add_timeout3(0.05, move |_| {
                schedule_poll(
                    Rc::clone(&schema_receiver),
                    Rc::clone(&conn_receiver),
                    Rc::clone(&file_receiver),
                    state_weak.clone(),
                    schema_sender.clone(),
                );
            });
        }

        // Start polling
        let weak_state_for_poll = Rc::downgrade(&state);
        let schema_sender_for_poll = schema_sender.clone();
        schedule_poll(
            schema_receiver,
            conn_receiver,
            file_receiver,
            weak_state_for_poll,
            schema_sender_for_poll,
        );

        let weak_state_for_file_drop = Rc::downgrade(&state);
        let file_sender_for_drop = file_sender.clone();
        state
            .borrow_mut()
            .sql_editor
            .set_file_drop_callback(move |path| {
                if let Some(state_for_drop) = weak_state_for_file_drop.upgrade() {
                    let mut s = state_for_drop.borrow_mut();
                    let conn_info = s.connection_info.borrow().clone();
                    let file_label = path.file_name().unwrap_or_default().to_string_lossy();
                    s.status_bar.set_label(&format_status(
                        &format!("Opening {}", file_label),
                        &conn_info,
                    ));
                }

                let sender = file_sender_for_drop.clone();
                thread::spawn(move || {
                    let result = fs::read_to_string(&path).map_err(|err| err.to_string());
                    let _ = sender.send(FileActionResult::Open { path, result });
                    app::awake();
                });
            });

        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            let weak_state_for_menu = Rc::downgrade(&state);
            let file_sender = file_sender.clone();
            menu.set_callback(move |m| {
                let Some(state_for_menu) = weak_state_for_menu.upgrade() else {
                    return;
                };
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
                                    s.status_bar.set_label(&format!("Connecting to {}...", info.display_string()));
                                }
                                thread::spawn(move || {
                                    let mut db_conn = lock_connection(&connection);
                                    match db_conn.connect(info.clone()) {
                                        Ok(_) => {
                                            let session = db_conn.session_state();
                                            drop(db_conn);
                                            match session.lock() {
                                                Ok(mut guard) => guard.reset(),
                                                Err(poisoned) => {
                                                    eprintln!(
                                                        "Warning: session state lock was poisoned; recovering."
                                                    );
                                                    poisoned.into_inner().reset();
                                                }
                                            }
                                            // Clear password before sending info across channel
                                            let mut info = info;
                                            info.clear_password();
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
                            let session = db_conn.session_state();
                            drop(db_conn);
                            match session.lock() {
                                Ok(mut guard) => guard.reset(),
                                Err(poisoned) => {
                                    eprintln!(
                                        "Warning: session state lock was poisoned; recovering."
                                    );
                                    poisoned.into_inner().reset();
                                }
                            }

                            let mut s = state_for_menu.borrow_mut();
                            *s.connection_info.borrow_mut() = None;
                            s.status_bar.set_label("Disconnected");
                            *s.sql_editor.get_intellisense_data().borrow_mut() = IntellisenseData::new();
                            s.sql_editor.get_highlighter().borrow_mut().set_highlight_data(HighlightData::new());
                        }
                        "File/Open SQL File..." => {
                            let mut dialog = FileDialog::new(FileDialogType::BrowseFile);
                            dialog.set_filter("SQL Files\t*.sql\nAll Files\t*.*");
                            dialog.show();
                            let filename = dialog.filename();
                            if !filename.as_os_str().is_empty() {
                                let sender = file_sender.clone();
                                thread::spawn(move || {
                                    let result = fs::read_to_string(&filename)
                                        .map_err(|err| err.to_string());
                                    let _ = sender.send(FileActionResult::Open {
                                        path: filename,
                                        result,
                                    });
                                    app::awake();
                                });
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
                                let sender = file_sender.clone();
                                thread::spawn(move || {
                                    let result =
                                        fs::write(&path, sql_text).map_err(|err| err.to_string());
                                    let _ = sender.send(FileActionResult::Save { path, result });
                                    app::awake();
                                });
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
                                let conn_info = s.connection_info.borrow().clone();
                                if cell_count > 0 {
                                    s.status_bar.set_label(&format_status(
                                        &format!("Copied {} cells to clipboard", cell_count),
                                        &conn_info,
                                    ));
                                } else {
                                    s.status_bar.set_label(&format_status(
                                        "No cells selected to copy",
                                        &conn_info,
                                    ));
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
                                let conn_info = s.connection_info.borrow().clone();
                                s.status_bar.set_label(&format_status(
                                    "Copied selection with headers",
                                    &conn_info,
                                ));
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
                        "Query/Execute Statement (F9)" => state_for_menu
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
                            let row_count = state_for_menu.borrow().result_tabs.row_count();
                            let sender = file_sender.clone();
                            thread::spawn(move || {
                                let result =
                                    fs::write(&filename, csv).map_err(|err| err.to_string());
                                let _ = sender.send(FileActionResult::Export {
                                    path: filename,
                                    row_count,
                                    result,
                                });
                                app::awake();
                            });
                        }
                        "Edit/Find..." => {
                            let (mut editor, mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
                            };
                            FindReplaceDialog::show_find_with_registry(&mut editor, &mut buffer, popups);
                        }
                        "Edit/Find Next" => {
                            let (mut editor, mut buffer, popups) = {
                                let s = state_for_menu.borrow_mut();
                                (s.sql_editor.get_editor(), s.sql_buffer.clone(), s.popups.clone())
                            };
                            if !FindReplaceDialog::find_next_from_session(&mut editor, &mut buffer)
                                && !FindReplaceDialog::has_search_text()
                            {
                                FindReplaceDialog::show_find_with_registry(
                                    &mut editor,
                                    &mut buffer,
                                    popups,
                                );
                            }
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
                            MainWindow::open_query_history_dialog(&state_for_menu);
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
                            let connection = {
                                let s = state_for_menu.borrow();
                                s.connection.clone()
                            };
                            {
                                let mut connection = lock_connection(&connection);
                                connection.set_auto_commit(enabled);
                            }
                            let mut s = state_for_menu.borrow_mut();
                            let conn_info = s.connection_info.borrow().clone();
                            s.status_bar.set_label(&format_status(status, &conn_info));
                        }
                        "Settings/Preferences..." => {
                            let config_snapshot = {
                                let s = state_for_menu.borrow();
                                let config_snapshot = s.config.borrow().clone();
                                config_snapshot
                            };
                            if let Some(settings) = show_settings_dialog(&config_snapshot) {
                                let mut s = state_for_menu.borrow_mut();
                                let save_result = {
                                    let mut config = s.config.borrow_mut();
                                    config.editor_font = settings.font.clone();
                                    config.ui_font_size = settings.ui_size;
                                    config.editor_font_size = settings.editor_size;
                                    config.result_font = settings.font;
                                    config.result_font_size = settings.result_size;
                                    config.result_cell_max_chars = settings.result_cell_max_chars;
                                    config.save()
                                };
                                if let Err(err) = save_result {
                                    fltk::dialog::alert_default(&format!(
                                        "Failed to save settings: {}",
                                        err
                                    ));
                                }
                                MainWindow::apply_font_settings(&mut s);
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    pub fn show(&mut self) {
        let state = self.state.clone();
        let mut window = {
            let s = state.borrow();
            s.window.clone()
        };
        let weak_state_for_close = Rc::downgrade(&state);
        window.set_callback(move |w| {
            if let Some(state) = weak_state_for_close.upgrade() {
                let (popups, sql_editor, mut result_tabs) = {
                    let s = state.borrow();
                    (
                        s.popups.clone(),
                        s.sql_editor.clone(),
                        s.result_tabs.clone(),
                    )
                };
                let mut popups = popups.borrow_mut();
                for mut popup in popups.drain(..) {
                    popup.hide();
                }
                sql_editor.hide_intellisense();
                // Clean up result tabs to release FLTK widget callbacks and data buffers
                result_tabs.clear();
            }
            w.hide();
            app::quit();
        });
        window.show();
        app::flush();
        let _ = app::wait();
        {
            let mut s = state.borrow_mut();
            MainWindow::adjust_query_layout(&mut s);
            s.window.redraw();
            s.sql_editor.focus();
        }
    }

    pub fn run() {
        let app = app::App::default()
            .with_scheme(app::Scheme::Gtk)
            .load_system_fonts();
        let config = AppConfig::load();
        let ui_size = config.ui_font_size.clamp(8, 24) as i32;
        app::set_font_size(ui_size);
        fltk::misc::Tooltip::set_font_size(ui_size);

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
