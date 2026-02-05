use fltk::{
    app,
    button::Button,
    draw::set_cursor,
    enums::{Cursor, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType, Pack, PackType},
    input::IntInput,
    prelude::*,
    text::{TextBuffer, TextEditor, WrapMode},
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use crate::db::{
    lock_connection, ConnectionInfo, QueryExecutor, QueryResult, SharedConnection,
    TableColumnDetail,
};
use crate::ui::intellisense::{IntellisenseData, IntellisensePopup};
use crate::ui::query_history::QueryHistoryDialog;
use crate::ui::syntax_highlight::{
    create_style_table, HighlightData, SqlHighlighter, STYLE_DEFAULT,
};
use crate::ui::theme;

mod execution;
mod intellisense;

#[derive(Clone, Debug)]
pub(crate) enum SqlToken {
    Word(String),
    String(String),
    Comment(String),
    Symbol(String),
}

const INTELLISENSE_WORD_WINDOW: i32 = 256;
const INTELLISENSE_CONTEXT_WINDOW: i32 = 120_000;
const INTELLISENSE_QUALIFIER_WINDOW: i32 = 256;
const INTELLISENSE_STATEMENT_WINDOW: i32 = 120_000;

#[derive(Clone)]
struct TableReference {
    table: String,
    alias: Option<String>,
}

#[derive(Clone)]
pub enum QueryProgress {
    BatchStart,
    StatementStart {
        index: usize,
    },
    SelectStart {
        index: usize,
        columns: Vec<String>,
    },
    Rows {
        index: usize,
        rows: Vec<Vec<String>>,
    },
    ScriptOutput {
        lines: Vec<String>,
    },
    PromptInput {
        prompt: String,
        response: mpsc::Sender<Option<String>>,
    },
    AutoCommitChanged {
        enabled: bool,
    },
    ConnectionChanged {
        info: Option<ConnectionInfo>,
    },
    StatementFinished {
        index: usize,
        result: QueryResult,
        connection_name: String,
        timed_out: bool,
    },
    BatchFinished,
}

#[derive(Clone)]
pub(crate) struct ColumnLoadUpdate {
    table: String,
    columns: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct PendingIntellisense {
    cursor_pos: i32,
}

#[derive(Clone)]
enum UiActionResult {
    ExplainPlan(Result<Vec<String>, String>),
    QuickDescribe {
        object_name: String,
        result: Result<Vec<TableColumnDetail>, String>,
    },
    Commit(Result<(), String>),
    Rollback(Result<(), String>),
    Cancel(Result<(), String>),
}

#[derive(Clone)]
pub struct SqlEditorWidget {
    group: Flex,
    editor: TextEditor,
    buffer: TextBuffer,
    style_buffer: TextBuffer,
    connection: SharedConnection,
    execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>>,
    progress_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryProgress)>>>>,
    progress_sender: mpsc::Sender<QueryProgress>,
    column_sender: mpsc::Sender<ColumnLoadUpdate>,
    ui_action_sender: mpsc::Sender<UiActionResult>,
    query_running: Rc<RefCell<bool>>,
    intellisense_data: Rc<RefCell<IntellisenseData>>,
    intellisense_popup: Rc<RefCell<IntellisensePopup>>,
    highlighter: Rc<RefCell<SqlHighlighter>>,
    timeout_input: IntInput,
    status_callback: Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>,
    find_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>>,
    replace_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>>,
    completion_range: Rc<RefCell<Option<(usize, usize)>>>,
    pending_intellisense: Rc<RefCell<Option<PendingIntellisense>>>,
}

impl SqlEditorWidget {
    pub fn new(connection: SharedConnection) -> Self {
        let mut group = Flex::default();
        group.set_type(FlexType::Column);
        group.set_margin(0);
        group.set_spacing(5);
        group.set_frame(FrameType::FlatBox);
        group.set_color(theme::panel_bg()); // Windows 11-inspired panel background

        // Button toolbar with modern styling
        let mut button_pack = Pack::default();
        button_pack.set_type(PackType::Horizontal);
        button_pack.set_spacing(6);

        let mut execute_btn = Button::default().with_size(90, 20).with_label("@> Execute");
        execute_btn.set_color(theme::button_primary());
        execute_btn.set_label_color(theme::text_primary());
        execute_btn.set_frame(FrameType::RFlatBox);

        let mut cancel_btn = Button::default().with_size(80, 20).with_label("Cancel");
        cancel_btn.set_color(theme::button_warning());
        cancel_btn.set_label_color(theme::text_primary());
        cancel_btn.set_frame(FrameType::RFlatBox);

        let mut explain_btn = Button::default().with_size(80, 20).with_label("Explain");
        explain_btn.set_color(theme::button_secondary());
        explain_btn.set_label_color(theme::text_primary());
        explain_btn.set_frame(FrameType::RFlatBox);

        let mut clear_btn = Button::default().with_size(70, 20).with_label("Clear");
        clear_btn.set_color(theme::button_subtle());
        clear_btn.set_label_color(theme::text_secondary());
        clear_btn.set_frame(FrameType::RFlatBox);

        let mut commit_btn = Button::default().with_size(80, 20).with_label("Commit");
        commit_btn.set_color(theme::button_success());
        commit_btn.set_label_color(theme::text_primary());
        commit_btn.set_frame(FrameType::RFlatBox);

        let mut rollback_btn = Button::default().with_size(80, 20).with_label("Rollback");
        rollback_btn.set_color(theme::button_danger());
        rollback_btn.set_label_color(theme::text_primary());
        rollback_btn.set_frame(FrameType::RFlatBox);

        let mut timeout_label = Frame::default().with_size(85, 28);
        timeout_label.set_label("Timeout(s)");
        timeout_label.set_label_color(theme::text_muted());

        let mut timeout_input = IntInput::default().with_size(55, 28);
        timeout_input.set_color(theme::input_bg());
        timeout_input.set_text_color(theme::text_primary());
        timeout_input.set_tooltip("Call timeout in seconds (empty = no timeout)");

        button_pack.end();
        group.fixed(&button_pack, 34);

        // SQL Editor with modern styling
        let buffer = TextBuffer::default();
        let style_buffer = TextBuffer::default();
        let mut editor = TextEditor::default();
        editor.set_buffer(buffer.clone());
        editor.set_color(theme::editor_bg());
        editor.set_text_color(theme::text_primary());
        editor.set_text_font(Font::Courier);
        editor.set_text_size(14);
        editor.set_cursor_color(theme::text_primary());
        editor.wrap_mode(WrapMode::AtBounds, 0);
        editor.super_handle_first(false);

        // Windows 11 selection color
        editor.set_selection_color(theme::selection_soft());

        // Setup syntax highlighting
        let style_table = create_style_table();
        editor.set_highlight_data(style_buffer.clone(), style_table);

        // Add editor to flex and make it resizable (takes remaining space)
        group.resizable(&editor);
        group.end();

        let execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>> =
            Rc::new(RefCell::new(None));
        let progress_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryProgress)>>>> =
            Rc::new(RefCell::new(None));
        let (progress_sender, progress_receiver) = mpsc::channel::<QueryProgress>();
        let (column_sender, column_receiver) = mpsc::channel::<ColumnLoadUpdate>();
        let (ui_action_sender, ui_action_receiver) = mpsc::channel::<UiActionResult>();
        let query_running = Rc::new(RefCell::new(false));

        let intellisense_data = Rc::new(RefCell::new(IntellisenseData::new()));
        let intellisense_popup = Rc::new(RefCell::new(IntellisensePopup::new()));
        let highlighter = Rc::new(RefCell::new(SqlHighlighter::new()));
        let status_callback: Rc<RefCell<Option<Box<dyn FnMut(&str)>>>> =
            Rc::new(RefCell::new(None));
        let find_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let replace_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let completion_range = Rc::new(RefCell::new(None::<(usize, usize)>));
        let pending_intellisense = Rc::new(RefCell::new(None::<PendingIntellisense>));

        let mut widget = Self {
            group,
            editor,
            buffer,
            style_buffer,
            connection,
            execute_callback,
            progress_callback: progress_callback.clone(),
            progress_sender,
            column_sender,
            ui_action_sender,
            query_running: query_running.clone(),
            intellisense_data,
            intellisense_popup,
            highlighter,
            timeout_input: timeout_input.clone(),
            status_callback,
            find_callback,
            replace_callback,
            completion_range,
            pending_intellisense,
        };

        widget.setup_button_callbacks(
            execute_btn,
            cancel_btn,
            explain_btn,
            clear_btn,
            commit_btn,
            rollback_btn,
        );
        widget.setup_intellisense();
        widget.setup_syntax_highlighting();
        widget.setup_progress_handler(progress_receiver, progress_callback, query_running);
        widget.setup_column_loader(column_receiver);
        widget.setup_ui_action_handler(ui_action_receiver);

        widget
    }

    fn setup_progress_handler(
        &self,
        progress_receiver: mpsc::Receiver<QueryProgress>,
        progress_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryProgress)>>>>,
        query_running: Rc<RefCell<bool>>,
    ) {
        let execute_callback = self.execute_callback.clone();

        // Wrap receiver in Rc<RefCell> to share across timeout callbacks
        let receiver: Rc<RefCell<mpsc::Receiver<QueryProgress>>> =
            Rc::new(RefCell::new(progress_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<mpsc::Receiver<QueryProgress>>>,
            progress_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryProgress)>>>>,
            query_running: Rc<RefCell<bool>>,
            execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>>,
        ) {
            let mut disconnected = false;
            // Process any pending messages
            loop {
                let message = {
                    let r = receiver.borrow();
                    r.try_recv()
                };

                match message {
                    Ok(message) => {
                        if let Some(ref mut cb) = *progress_callback.borrow_mut() {
                            cb(message.clone());
                        }

                        match message {
                            QueryProgress::PromptInput { prompt, response } => {
                                let value = SqlEditorWidget::prompt_input_dialog(&prompt);
                                let _ = response.send(value);
                            }
                            QueryProgress::StatementFinished {
                                result,
                                connection_name,
                                timed_out,
                                ..
                            } => {
                                if timed_out {
                                    fltk::dialog::alert_default(&format!(
                                        "Query timed out!\n\n{}",
                                        result.message
                                    ));
                                }
                                QueryHistoryDialog::add_to_history(
                                    &result.sql,
                                    result.execution_time.as_millis() as u64,
                                    result.row_count,
                                    &connection_name,
                                );
                                if let Some(ref mut cb) = *execute_callback.borrow_mut() {
                                    cb(result);
                                }
                            }
                            QueryProgress::BatchFinished => {
                                *query_running.borrow_mut() = false;
                                set_cursor(Cursor::Default);
                                app::flush();
                            }
                            _ => {}
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                return;
            }

            if *query_running.borrow() {
                set_cursor(Cursor::Wait);
                app::flush();
            }

            // Reschedule for next poll
            app::add_timeout3(0.05, move |_| {
                schedule_poll(
                    Rc::clone(&receiver),
                    Rc::clone(&progress_callback),
                    Rc::clone(&query_running),
                    Rc::clone(&execute_callback),
                );
            });
        }

        // Start polling
        schedule_poll(receiver, progress_callback, query_running, execute_callback);
    }

    fn setup_column_loader(&self, column_receiver: mpsc::Receiver<ColumnLoadUpdate>) {
        let intellisense_data = self.intellisense_data.clone();
        let editor = self.editor.clone();
        let buffer = self.buffer.clone();
        let intellisense_popup = self.intellisense_popup.clone();
        let completion_range = self.completion_range.clone();
        let column_sender = self.column_sender.clone();
        let connection = self.connection.clone();
        let pending_intellisense = self.pending_intellisense.clone();

        // Wrap receiver in Rc<RefCell> to share across timeout callbacks
        let receiver: Rc<RefCell<mpsc::Receiver<ColumnLoadUpdate>>> =
            Rc::new(RefCell::new(column_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<mpsc::Receiver<ColumnLoadUpdate>>>,
            intellisense_data: Rc<RefCell<IntellisenseData>>,
            editor: TextEditor,
            buffer: TextBuffer,
            intellisense_popup: Rc<RefCell<IntellisensePopup>>,
            completion_range: Rc<RefCell<Option<(usize, usize)>>>,
            column_sender: mpsc::Sender<ColumnLoadUpdate>,
            connection: SharedConnection,
            pending_intellisense: Rc<RefCell<Option<PendingIntellisense>>>,
        ) {
            let mut disconnected = false;
            // Process any pending messages
            {
                let r = receiver.borrow();
                loop {
                    match r.try_recv() {
                        Ok(update) => {
                            {
                                let mut data = intellisense_data.borrow_mut();
                                data.set_columns_for_table(&update.table, update.columns);
                            }

                            let pending = pending_intellisense.borrow().clone();
                            if let Some(pending) = pending {
                                let cursor_pos = editor.insert_position().max(0);
                                if cursor_pos == pending.cursor_pos {
                                    SqlEditorWidget::trigger_intellisense(
                                        &editor,
                                        &buffer,
                                        &intellisense_data,
                                        &intellisense_popup,
                                        &completion_range,
                                        &column_sender,
                                        &connection,
                                        &pending_intellisense,
                                    );
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
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
                    Rc::clone(&intellisense_data),
                    editor.clone(),
                    buffer.clone(),
                    Rc::clone(&intellisense_popup),
                    Rc::clone(&completion_range),
                    column_sender.clone(),
                    connection.clone(),
                    Rc::clone(&pending_intellisense),
                );
            });
        }

        // Start polling
        schedule_poll(
            receiver,
            intellisense_data,
            editor,
            buffer,
            intellisense_popup,
            completion_range,
            column_sender,
            connection,
            pending_intellisense,
        );
    }

    fn setup_ui_action_handler(&self, ui_action_receiver: mpsc::Receiver<UiActionResult>) {
        let widget = self.clone();

        let receiver: Rc<RefCell<mpsc::Receiver<UiActionResult>>> =
            Rc::new(RefCell::new(ui_action_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<mpsc::Receiver<UiActionResult>>>,
            widget: SqlEditorWidget,
        ) {
            let mut disconnected = false;
            loop {
                let message = {
                    let r = receiver.borrow();
                    r.try_recv()
                };

                match message {
                    Ok(action) => {
                        let should_reset_cursor = !matches!(&action, UiActionResult::Cancel(_));
                        match action {
                            UiActionResult::ExplainPlan(result) => match result {
                                Ok(plan_lines) => {
                                    let plan_text = if plan_lines.is_empty() {
                                        "No plan output.".to_string()
                                    } else {
                                        plan_lines.join("\n")
                                    };
                                    SqlEditorWidget::show_plan_dialog(&plan_text);
                                }
                                Err(err) => {
                                    let _ =
                                        widget.progress_sender.send(QueryProgress::ScriptOutput {
                                            lines: vec![format!("Explain plan failed: {}", err)],
                                        });
                                    widget.emit_status("Explain plan failed");
                                }
                            },
                            UiActionResult::QuickDescribe {
                                object_name,
                                result,
                            } => match result {
                                Ok(columns) => {
                                    if columns.is_empty() {
                                        fltk::dialog::message_default(&format!(
                                            "No table or view found with name: {}",
                                            object_name.to_uppercase()
                                        ));
                                    } else {
                                        SqlEditorWidget::show_quick_describe_dialog(
                                            &object_name,
                                            &columns,
                                        );
                                    }
                                }
                                Err(err) => {
                                    if err.contains("Not connected") {
                                        fltk::dialog::alert_default("Not connected to database");
                                    } else {
                                        fltk::dialog::message_default(&format!(
                                            "Object not found or not accessible: {} ({})",
                                            object_name.to_uppercase(),
                                            err
                                        ));
                                    }
                                }
                            },
                            UiActionResult::Commit(result) => match result {
                                Ok(()) => {
                                    widget.emit_status("Committed");
                                }
                                Err(err) => {
                                    let _ =
                                        widget.progress_sender.send(QueryProgress::ScriptOutput {
                                            lines: vec![format!("Commit failed: {}", err)],
                                        });
                                    widget.emit_status("Commit failed");
                                }
                            },
                            UiActionResult::Rollback(result) => match result {
                                Ok(()) => {
                                    widget.emit_status("Rolled back");
                                }
                                Err(err) => {
                                    let _ =
                                        widget.progress_sender.send(QueryProgress::ScriptOutput {
                                            lines: vec![format!("Rollback failed: {}", err)],
                                        });
                                    widget.emit_status("Rollback failed");
                                }
                            },
                            UiActionResult::Cancel(result) => {
                                if let Err(err) = result {
                                    let _ =
                                        widget.progress_sender.send(QueryProgress::ScriptOutput {
                                            lines: vec![format!("Cancel failed: {}", err)],
                                        });
                                    widget.emit_status("Cancel failed");
                                }
                            }
                        }
                        if should_reset_cursor {
                            set_cursor(Cursor::Default);
                            app::flush();
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                return;
            }

            app::add_timeout3(0.05, move |_| {
                schedule_poll(Rc::clone(&receiver), widget.clone());
            });
        }

        schedule_poll(receiver, widget);
    }

    fn setup_syntax_highlighting(&self) {
        let highlighter = self.highlighter.clone();
        let mut style_buffer = self.style_buffer.clone();
        let mut buffer = self.buffer.clone();
        let editor = self.editor.clone();
        buffer.add_modify_callback2(move |buf, pos, ins, del, _restyled, deleted_text| {
            // Synchronize style_buffer length with text buffer
            // highlight_buffer_window will reset if lengths differ, but we do incremental
            // updates here to maintain consistency for small edits
            let text_len = buf.length();
            let style_len = style_buffer.length();

            if del > 0 && ins == 0 {
                // Pure deletion
                if pos >= 0 && pos < style_len {
                    let del_end = (pos + del).min(style_len);
                    if pos < del_end {
                        style_buffer.remove(pos, del_end);
                    }
                }
            } else if ins > 0 && del == 0 {
                // Pure insertion
                if pos >= 0 {
                    let insert_pos = pos.min(style_buffer.length());
                    let insert_styles: String = std::iter::repeat(STYLE_DEFAULT)
                        .take(ins as usize)
                        .collect();
                    style_buffer.insert(insert_pos, &insert_styles);
                }
            } else if ins > 0 && del > 0 {
                // Replacement: remove then insert
                if pos >= 0 && pos < style_len {
                    let del_end = (pos + del).min(style_len);
                    if pos < del_end {
                        style_buffer.remove(pos, del_end);
                    }
                }
                if pos >= 0 {
                    let insert_pos = pos.min(style_buffer.length());
                    let insert_styles: String = std::iter::repeat(STYLE_DEFAULT)
                        .take(ins as usize)
                        .collect();
                    style_buffer.insert(insert_pos, &insert_styles);
                }
            }

            // Final length check - if still mismatched, let highlight_buffer_window handle it
            // This provides a safety net for edge cases
            let final_style_len = style_buffer.length();
            if final_style_len != text_len {
                // Length mismatch detected - highlight_buffer_window will reset
                // This can happen with complex multi-byte character operations
            }

            let cursor_pos = editor.insert_position().max(0) as usize;
            let text_len = buf.length().max(0) as usize;
            let mut edited_range = compute_edited_range(pos, ins, del, text_len);

            if needs_full_rehighlight(buf, pos, ins, deleted_text) {
                edited_range = Some((0, text_len));
            } else if let Some((start, end)) = edited_range {
                let inserted_text = inserted_text(buf, pos, ins);
                if !has_stateful_sql_delimiter(&inserted_text) {
                    edited_range = Some(expand_connected_word_range(buf, start, end));
                }
            }

            highlighter.borrow().highlight_buffer_window(
                buf,
                &mut style_buffer,
                cursor_pos,
                edited_range,
            );
        });
        self.refresh_highlighting();
    }

    fn setup_button_callbacks(
        &mut self,
        mut execute_btn: Button,
        mut cancel_btn: Button,
        mut explain_btn: Button,
        mut clear_btn: Button,
        mut commit_btn: Button,
        mut rollback_btn: Button,
    ) {
        let widget = self.clone();
        execute_btn.set_callback(move |_| {
            widget.execute_current();
        });

        let widget = self.clone();
        cancel_btn.set_callback(move |_| {
            widget.cancel_current();
        });

        let widget = self.clone();
        explain_btn.set_callback(move |_| {
            widget.explain_current();
        });

        let widget = self.clone();
        clear_btn.set_callback(move |_| {
            widget.clear();
        });

        let widget = self.clone();
        commit_btn.set_callback(move |_| {
            widget.commit();
        });

        let widget = self.clone();
        rollback_btn.set_callback(move |_| {
            widget.rollback();
        });
    }

    pub fn explain_current(&self) {
        let sql = self.buffer.text();
        let cursor_pos = self.editor.insert_position() as usize;
        let Some(sql) = QueryExecutor::statement_at_cursor(&sql, cursor_pos) else {
            fltk::dialog::alert_default("No SQL at cursor");
            return;
        };

        let connection = self.connection.clone();
        let sender = self.ui_action_sender.clone();
        set_cursor(Cursor::Wait);
        app::flush();
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
                QueryExecutor::get_explain_plan(db_conn.as_ref(), &sql)
                    .map_err(|err| err.to_string())
            } else {
                Err("Not connected to database".to_string())
            };

            let _ = sender.send(UiActionResult::ExplainPlan(result));
            app::awake();
        });
    }

    fn show_plan_dialog(plan_text: &str) {
        use fltk::{prelude::*, text::TextDisplay, window::Window};

        let current_group = fltk::group::Group::try_current();

        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut dialog = Window::default()
            .with_size(800, 500)
            .with_label("Explain Plan")
            .center_screen();
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);
        dialog.begin();

        let mut display = TextDisplay::default().with_pos(10, 10).with_size(780, 440);
        display.set_color(theme::editor_bg());
        display.set_text_color(theme::text_primary());
        display.set_text_font(fltk::enums::Font::Courier);
        display.set_text_size(14);

        let mut buffer = fltk::text::TextBuffer::default();
        buffer.set_text(plan_text);
        display.set_buffer(buffer);

        let mut close_btn = fltk::button::Button::default()
            .with_pos(690, 455)
            .with_size(100, 20)
            .with_label("Close");
        close_btn.set_color(theme::button_secondary());
        close_btn.set_label_color(theme::text_primary());

        let (sender, receiver) = mpsc::channel::<()>();
        close_btn.set_callback(move |_| {
            let _ = sender.send(());
            app::awake();
        });

        dialog.end();
        dialog.show();
        fltk::group::Group::set_current(current_group.as_ref());
        let _ = dialog.take_focus();
        let _ = close_btn.take_focus();

        while dialog.shown() {
            app::wait();
            if receiver.try_recv().is_ok() {
                dialog.hide();
            }
        }
    }

    fn emit_status(&self, message: &str) {
        if let Some(ref mut callback) = *self.status_callback.borrow_mut() {
            callback(message);
        }
    }

    pub fn clear(&self) {
        let mut buffer = self.buffer.clone();
        buffer.set_text("");
    }

    pub fn commit(&self) {
        let connection = self.connection.clone();
        let sender = self.ui_action_sender.clone();
        set_cursor(Cursor::Wait);
        app::flush();
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
                db_conn.commit().map_err(|err| err.to_string())
            } else {
                Err("Not connected to database".to_string())
            };

            let _ = sender.send(UiActionResult::Commit(result));
            app::awake();
        });
    }

    pub fn rollback(&self) {
        let connection = self.connection.clone();
        let sender = self.ui_action_sender.clone();
        set_cursor(Cursor::Wait);
        app::flush();
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
                db_conn.rollback().map_err(|err| err.to_string())
            } else {
                Err("Not connected to database".to_string())
            };

            let _ = sender.send(UiActionResult::Rollback(result));
            app::awake();
        });
    }

    pub fn cancel_current(&self) {
        if !*self.query_running.borrow() {
            fltk::dialog::alert_default("No query is running");
            return;
        }

        let connection = self.connection.clone();
        let sender = self.ui_action_sender.clone();
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
                db_conn.break_execution().map_err(|err| err.to_string())
            } else {
                Err("Not connected to database".to_string())
            };

            let _ = sender.send(UiActionResult::Cancel(result));
            app::awake();
        });
    }

    pub fn set_execute_callback<F>(&mut self, callback: F)
    where
        F: FnMut(QueryResult) + 'static,
    {
        *self.execute_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn set_status_callback<F>(&mut self, callback: F)
    where
        F: FnMut(&str) + 'static,
    {
        *self.status_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn set_find_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + 'static,
    {
        *self.find_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn set_replace_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + 'static,
    {
        *self.replace_callback.borrow_mut() = Some(Box::new(callback));
    }

    #[allow(dead_code)]
    pub fn update_highlight_data(&mut self, data: HighlightData) {
        self.highlighter.borrow_mut().set_highlight_data(data);
        // Re-highlight current text
        let mut style_buffer = self.style_buffer.clone();
        self.highlighter.borrow().highlight_buffer_window(
            &self.buffer,
            &mut style_buffer,
            self.editor.insert_position().max(0) as usize,
            None,
        );
    }

    pub fn get_highlighter(&self) -> Rc<RefCell<SqlHighlighter>> {
        self.highlighter.clone()
    }

    #[allow(dead_code)]
    pub fn get_text(&self) -> String {
        self.buffer.text()
    }

    #[allow(dead_code)]
    pub fn set_text(&mut self, text: &str) {
        self.buffer.set_text(text);
    }

    pub fn get_group(&self) -> &Flex {
        &self.group
    }

    pub fn get_buffer(&self) -> TextBuffer {
        self.buffer.clone()
    }

    #[allow(dead_code)]
    pub fn refresh_highlighting(&self) {
        self.highlighter.borrow().highlight_buffer_window(
            &self.buffer,
            &mut self.style_buffer.clone(),
            self.editor.insert_position().max(0) as usize,
            None,
        );
    }

    #[allow(dead_code)]
    pub fn append_text(&mut self, text: &str) {
        let current = self.buffer.text();
        if current.is_empty() {
            self.buffer.set_text(text);
        } else {
            self.buffer.set_text(&format!("{}\n{}", current, text));
        }
    }

    pub fn get_editor(&self) -> TextEditor {
        self.editor.clone()
    }

    pub fn is_query_running(&self) -> bool {
        *self.query_running.borrow()
    }
}

fn inserted_text(buf: &TextBuffer, pos: i32, ins: i32) -> String {
    if ins <= 0 || pos < 0 {
        return String::new();
    }

    let insert_end = pos.saturating_add(ins).min(buf.length());
    buf.text_range(pos, insert_end).unwrap_or_default()
}

fn is_identifier_continue_byte_for_expand(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn expand_connected_word_range(buf: &TextBuffer, start: usize, end: usize) -> (usize, usize) {
    let text = buf.text();
    let bytes = text.as_bytes();
    let mut expanded_start = start.min(bytes.len());
    let mut expanded_end = end.min(bytes.len());

    while expanded_start > 0 && is_identifier_continue_byte_for_expand(bytes[expanded_start - 1]) {
        expanded_start -= 1;
    }

    while expanded_end < bytes.len() && is_identifier_continue_byte_for_expand(bytes[expanded_end])
    {
        expanded_end += 1;
    }

    (expanded_start, expanded_end)
}

fn compute_edited_range(pos: i32, ins: i32, del: i32, text_len: usize) -> Option<(usize, usize)> {
    if pos < 0 {
        return None;
    }

    let start = (pos as usize).min(text_len);
    let inserted = ins.max(0) as usize;
    let deleted = del.max(0) as usize;
    let changed_len = inserted.max(deleted);
    let end = start.saturating_add(changed_len).min(text_len);

    Some((start, end))
}

fn needs_full_rehighlight(buf: &TextBuffer, pos: i32, ins: i32, deleted_text: &str) -> bool {
    let mut changed_text = String::new();

    if !deleted_text.is_empty() {
        changed_text.push_str(deleted_text);
    }

    if ins > 0 && pos >= 0 {
        let insert_end = pos.saturating_add(ins).min(buf.length());
        if let Some(inserted_text) = buf.text_range(pos, insert_end) {
            changed_text.push_str(&inserted_text);
        }
    }

    if changed_text.is_empty() {
        return false;
    }

    if has_stateful_sql_delimiter(&changed_text) {
        return true;
    }

    if pos < 0 {
        return false;
    }

    let sample_start = pos.saturating_sub(2);
    let sample_end = pos
        .saturating_add(ins.max(0))
        .saturating_add(2)
        .min(buf.length());
    let nearby = buf.text_range(sample_start, sample_end).unwrap_or_default();

    has_stateful_sql_delimiter(&nearby)
}

fn has_stateful_sql_delimiter(text: &str) -> bool {
    text.contains("/*")
        || text.contains("*/")
        || text.contains("--")
        || text.contains("'")
        || text.contains("q'")
        || text.contains("Q'")
        || text.contains("nq'")
        || text.contains("NQ'")
        || text.contains("Nq'")
        || text.contains("nQ'")
}

#[cfg(test)]
mod sql_editor_tests;
