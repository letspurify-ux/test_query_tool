use fltk::{
    app,
    button::Button,
    draw::set_cursor,
    enums::{Cursor, Event, Font, FrameType, Key},
    frame::Frame,
    group::{Flex, FlexType, Pack, PackType},
    input::IntInput,
    prelude::*,
    text::{TextBuffer, TextEditor, WrapMode},
};
use oracle::Error as OracleError;
use std::collections::HashSet;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::db::{lock_connection, QueryExecutor, QueryResult, SharedConnection};
use crate::ui::intellisense::{
    detect_sql_context, get_word_at_cursor, IntellisenseData, IntellisensePopup, SqlContext,
    SQL_KEYWORDS,
};
use crate::ui::query_history::QueryHistoryDialog;
use crate::ui::syntax_highlight::{create_style_table, HighlightData, SqlHighlighter};
use crate::ui::theme;

#[derive(Clone)]
enum SqlToken {
    Word(String),
    String(String),
    Comment(String),
    Symbol(String),
}

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
    StatementFinished {
        index: usize,
        result: QueryResult,
        connection_name: String,
    },
    BatchFinished,
}

#[derive(Clone)]
struct ColumnLoadUpdate {
    table: String,
    columns: Vec<String>,
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
    query_running: Rc<RefCell<bool>>,
    intellisense_data: Rc<RefCell<IntellisenseData>>,
    intellisense_popup: Rc<RefCell<IntellisensePopup>>,
    highlighter: Rc<RefCell<SqlHighlighter>>,
    timeout_input: IntInput,
    status_callback: Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>,
    find_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>>,
    replace_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>>,
    completion_range: Rc<RefCell<Option<(usize, usize)>>>,
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
        let query_running = Rc::new(RefCell::new(false));

        let intellisense_data = Rc::new(RefCell::new(IntellisenseData::new()));
        let intellisense_popup = Rc::new(RefCell::new(IntellisensePopup::new()));
        let highlighter = Rc::new(RefCell::new(SqlHighlighter::new()));
        let status_callback: Rc<RefCell<Option<Box<dyn FnMut(&str)>>>> =
            Rc::new(RefCell::new(None));
        let find_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let replace_callback: Rc<RefCell<Option<Box<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let completion_range = Rc::new(RefCell::new(None::<(usize, usize)>));

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
            query_running: query_running.clone(),
            intellisense_data,
            intellisense_popup,
            highlighter,
            timeout_input: timeout_input.clone(),
            status_callback,
            find_callback,
            replace_callback,
            completion_range,
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
            // Process any pending messages
            {
                let r = receiver.borrow();
                while let Ok(message) = r.try_recv() {
                    if let Some(ref mut cb) = *progress_callback.borrow_mut() {
                        cb(message.clone());
                    }

                    match message {
                        QueryProgress::StatementFinished {
                            result,
                            connection_name,
                            ..
                        } => {
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
            }

            // Reschedule for next poll
            let receiver = receiver.clone();
            let progress_callback = progress_callback.clone();
            let query_running = query_running.clone();
            let execute_callback = execute_callback.clone();

            app::add_timeout(0.05, move || {
                schedule_poll(receiver, progress_callback, query_running, execute_callback);
            });
        }

        // Start polling
        schedule_poll(receiver, progress_callback, query_running, execute_callback);
    }

    fn setup_column_loader(&self, column_receiver: mpsc::Receiver<ColumnLoadUpdate>) {
        let intellisense_data = self.intellisense_data.clone();

        // Wrap receiver in Rc<RefCell> to share across timeout callbacks
        let receiver: Rc<RefCell<mpsc::Receiver<ColumnLoadUpdate>>> =
            Rc::new(RefCell::new(column_receiver));

        fn schedule_poll(
            receiver: Rc<RefCell<mpsc::Receiver<ColumnLoadUpdate>>>,
            intellisense_data: Rc<RefCell<IntellisenseData>>,
        ) {
            // Process any pending messages
            {
                let r = receiver.borrow();
                while let Ok(update) = r.try_recv() {
                    let mut data = intellisense_data.borrow_mut();
                    data.set_columns_for_table(&update.table, update.columns);
                }
            }

            // Reschedule for next poll
            let receiver = receiver.clone();
            let intellisense_data = intellisense_data.clone();

            app::add_timeout(0.05, move || {
                schedule_poll(receiver, intellisense_data);
            });
        }

        // Start polling
        schedule_poll(receiver, intellisense_data);
    }

    fn setup_syntax_highlighting(&self) {
        let highlighter = self.highlighter.clone();
        let mut style_buffer = self.style_buffer.clone();
        let mut buffer = self.buffer.clone();
        buffer.add_modify_callback2(move |buf, _pos, _ins, _del, _restyled, _deleted_text| {
            let text = buf.text();
            highlighter.borrow().highlight(&text, &mut style_buffer);
        });
        self.refresh_highlighting();
    }

    fn setup_intellisense(&mut self) {
        let buffer = self.buffer.clone();
        let mut editor = self.editor.clone();
        let intellisense_data = self.intellisense_data.clone();
        let intellisense_popup = self.intellisense_popup.clone();
        let connection = self.connection.clone();
        let column_sender = self.column_sender.clone();
        let highlighter = self.highlighter.clone();
        let style_buffer = self.style_buffer.clone();
        let suppress_enter = Rc::new(RefCell::new(false));
        let suppress_nav = Rc::new(RefCell::new(false));
        let nav_anchor = Rc::new(RefCell::new(None::<i32>));
        let completion_range = self.completion_range.clone();
        let ctrl_enter_handled = Rc::new(RefCell::new(false));

        // Setup callback for inserting selected text
        let mut buffer_for_insert = buffer.clone();
        let mut editor_for_insert = editor.clone();
        let completion_range_for_insert = completion_range.clone();
        let intellisense_data_for_insert = intellisense_data.clone();
        let column_sender_for_insert = column_sender.clone();
        let connection_for_insert = connection.clone();
        {
            let mut popup = intellisense_popup.borrow_mut();
            popup.set_selected_callback(move |selected| {
                let cursor_pos = editor_for_insert.insert_position() as usize;
                let text = buffer_for_insert.text();
                let context = detect_sql_context(&text, cursor_pos);
                if matches!(context, SqlContext::TableName) {
                    let should_prefetch = {
                        let data = intellisense_data_for_insert.borrow();
                        data.is_known_relation(&selected)
                    };
                    if should_prefetch {
                        Self::request_table_columns(
                            &selected,
                            &intellisense_data_for_insert,
                            &column_sender_for_insert,
                            &connection_for_insert,
                        );
                    }
                }
                let range = *completion_range_for_insert.borrow();
                let (start, end) = if let Some((range_start, range_end)) = range {
                    (range_start, range_end)
                } else {
                    let (word, start, _end) = get_word_at_cursor(&text, cursor_pos);
                    if word.is_empty() {
                        (cursor_pos, cursor_pos)
                    } else {
                        (start, cursor_pos)
                    }
                };

                if start != end {
                    buffer_for_insert.replace(start as i32, end as i32, &selected);
                    editor_for_insert.set_insert_position((start + selected.len()) as i32);
                } else {
                    buffer_for_insert.insert(cursor_pos as i32, &selected);
                    editor_for_insert.set_insert_position((cursor_pos + selected.len()) as i32);
                }
                *completion_range_for_insert.borrow_mut() = None;
            });
        }

        // Handle keyboard events for triggering intellisense and syntax highlighting
        let mut buffer_for_handle = buffer.clone();
        let intellisense_data_for_handle = intellisense_data.clone();
        let intellisense_popup_for_handle = intellisense_popup.clone();
        let column_sender_for_handle = column_sender.clone();
        let connection_for_handle = connection.clone();
        let highlighter_for_handle = highlighter.clone();
        let mut style_buffer_for_handle = style_buffer.clone();
        let suppress_enter_for_handle = suppress_enter.clone();
        let suppress_nav_for_handle = suppress_nav.clone();
        let nav_anchor_for_handle = nav_anchor.clone();
        let completion_range_for_handle = completion_range.clone();
        let widget_for_shortcuts = self.clone();
        let find_callback_for_handle = self.find_callback.clone();
        let replace_callback_for_handle = self.replace_callback.clone();
        let ctrl_enter_handled_for_handle = ctrl_enter_handled.clone();

        editor.handle(move |ed, ev| {
            match ev {
                Event::KeyDown => {
                    let key = fltk::app::event_key();
                    let popup_visible = intellisense_popup_for_handle.borrow().is_visible();

                    if popup_visible {
                        match key {
                            Key::Escape => {
                                // Close popup, consume event
                                intellisense_popup_for_handle.borrow_mut().hide();
                                *completion_range_for_handle.borrow_mut() = None;
                                return true;
                            }
                            Key::Up => {
                                // Navigate popup up, consume event
                                let pos = ed.insert_position();
                                *nav_anchor_for_handle.borrow_mut() = Some(pos);
                                intellisense_popup_for_handle.borrow_mut().select_prev();
                                ed.set_insert_position(pos);
                                ed.show_insert_position();
                                *suppress_nav_for_handle.borrow_mut() = true;
                                return true;
                            }
                            Key::Down => {
                                // Navigate popup down, consume event
                                let pos = ed.insert_position();
                                *nav_anchor_for_handle.borrow_mut() = Some(pos);
                                intellisense_popup_for_handle.borrow_mut().select_next();
                                ed.set_insert_position(pos);
                                ed.show_insert_position();
                                *suppress_nav_for_handle.borrow_mut() = true;
                                return true;
                            }
                            Key::Enter | Key::KPEnter | Key::Tab => {
                                // Insert selected suggestion, consume event
                                let selected = intellisense_popup_for_handle.borrow().get_selected();
                                if let Some(selected) = selected {
                                    let cursor_pos = ed.insert_position() as usize;
                                    let text = buffer_for_handle.text();
                                    let range = *completion_range_for_handle.borrow();
                                    let (start, end) = if let Some((range_start, range_end)) = range {
                                        (range_start, range_end)
                                    } else {
                                        let (word, start, _end) =
                                            get_word_at_cursor(&text, cursor_pos);
                                        if word.is_empty() {
                                            (cursor_pos, cursor_pos)
                                        } else {
                                            (start, cursor_pos)
                                        }
                                    };

                                    if start != end {
                                        buffer_for_handle.replace(
                                            start as i32,
                                            end as i32,
                                            &selected,
                                        );
                                        ed.set_insert_position((start + selected.len()) as i32);
                                    } else {
                                        buffer_for_handle.insert(cursor_pos as i32, &selected);
                                        ed.set_insert_position(
                                            (cursor_pos + selected.len()) as i32,
                                        );
                                    }
                                    *completion_range_for_handle.borrow_mut() = None;

                                    // Update syntax highlighting after insertion
                                    let new_text = buffer_for_handle.text();
                                    highlighter_for_handle
                                        .borrow()
                                        .highlight(&new_text, &mut style_buffer_for_handle);
                                }
                                if matches!(key, Key::Enter | Key::KPEnter) {
                                    *suppress_enter_for_handle.borrow_mut() = true;
                                }
                                intellisense_popup_for_handle.borrow_mut().hide();
                                return true;
                            }
                            _ => {
                                // Let other keys pass through to editor
                            }
                        }
                    }

                    if !ed.active() || (!ed.has_focus() && !popup_visible) {
                        return false;
                    }
                    // KeyDown fires BEFORE the character is inserted into the buffer.
                    // Handle navigation and selection keys here to consume them
                    // before they affect the editor.

                    // Handle basic editing shortcuts
                    let state = fltk::app::event_state();
                    let ctrl_or_cmd = state.contains(fltk::enums::Shortcut::Ctrl)
                        || state.contains(fltk::enums::Shortcut::Command);
                    let shift = state.contains(fltk::enums::Shortcut::Shift);
                    
                    if ctrl_or_cmd {
                        if shift && (key == Key::from_char('f') || key == Key::from_char('F')) {
                            widget_for_shortcuts.format_selected_sql();
                            return true;
                        }
                        match key {
                            k if k == Key::from_char(' ') => {
                                // Ctrl+Space - Trigger intellisense
                                Self::trigger_intellisense(
                                    ed,
                                    &buffer_for_handle,
                                    &intellisense_data_for_handle,
                                    &intellisense_popup_for_handle,
                                    &completion_range_for_handle,
                                    &column_sender_for_handle,
                                    &connection_for_handle,
                                );
                                return true;
                            }
                            Key::Enter | Key::KPEnter => {
                                if *ctrl_enter_handled_for_handle.borrow() {
                                    return true;
                                }
                                *ctrl_enter_handled_for_handle.borrow_mut() = true;
                                widget_for_shortcuts.execute_statement_at_cursor();
                                return true;
                            }
                            k if k == Key::from_char('f') || k == Key::from_char('F') => {
                                if let Some(ref mut cb) = *find_callback_for_handle.borrow_mut() {
                                    cb();
                                }
                                return true;
                            }
                            k if k == Key::from_char('/') || k == Key::from_char('?') => {
                                widget_for_shortcuts.toggle_comment();
                                return true;
                            }
                            k if k == Key::from_char('u') || k == Key::from_char('U') => {
                                widget_for_shortcuts.convert_selection_case(true);
                                return true;
                            }
                            k if k == Key::from_char('l') || k == Key::from_char('L') => {
                                widget_for_shortcuts.convert_selection_case(false);
                                return true;
                            }
                            k if k == Key::from_char('h') || k == Key::from_char('H') => {
                                if let Some(ref mut cb) = *replace_callback_for_handle.borrow_mut() {
                                    cb();
                                }
                                return true;
                            }
                            _ => {}
                        }
                    }

                    // F4 - Quick Describe (handle on KeyDown for immediate response)
                    if key == Key::F4 {
                        widget_for_shortcuts.quick_describe_at_cursor();
                        return true;
                    }

                    if key == Key::F5 {
                        widget_for_shortcuts.execute_current();
                        return true;
                    }

                    if key == Key::F9 {
                        widget_for_shortcuts.execute_selected();
                        return true;
                    }

                    if key == Key::F6 {
                        widget_for_shortcuts.explain_current();
                        return true;
                    }

                    if key == Key::F7 {
                        widget_for_shortcuts.commit();
                        return true;
                    }

                    if key == Key::F8 {
                        widget_for_shortcuts.rollback();
                        return true;
                    }

                    // Ctrl+Space - trigger intellisense manually
                    if fltk::app::event_state().contains(fltk::enums::Shortcut::Ctrl)
                        && key == Key::from_char(' ')
                    {
                        Self::trigger_intellisense(
                            ed,
                            &buffer_for_handle,
                            &intellisense_data_for_handle,
                            &intellisense_popup_for_handle,
                            &completion_range_for_handle,
                            &column_sender_for_handle,
                            &connection_for_handle,
                        );
                        return true;
                    }

                    false
                }
                Event::KeyUp => {
                    let popup_visible = intellisense_popup_for_handle.borrow().is_visible();
                    if !ed.active() || (!ed.has_focus() && !popup_visible) {
                        return false;
                    }
                    // KeyUp fires AFTER the character is inserted into the buffer.
                    // Filter/show intellisense here.
                    let text = buffer_for_handle.text();

                    let key = fltk::app::event_key();

                    if matches!(key, Key::Up | Key::Down)
                        && *suppress_nav_for_handle.borrow()
                    {
                        if let Some(pos) = *nav_anchor_for_handle.borrow() {
                            ed.set_insert_position(pos);
                            ed.show_insert_position();
                        }
                        *nav_anchor_for_handle.borrow_mut() = None;
                        *suppress_nav_for_handle.borrow_mut() = false;
                        return true;
                    }

                    if matches!(key, Key::Enter | Key::KPEnter)
                        && *suppress_enter_for_handle.borrow()
                    {
                        *suppress_enter_for_handle.borrow_mut() = false;
                        return true;
                    }
                    if matches!(key, Key::Enter | Key::KPEnter)
                        && *ctrl_enter_handled_for_handle.borrow()
                    {
                        *ctrl_enter_handled_for_handle.borrow_mut() = false;
                        return true;
                    }

                    // Navigation keys - hide popup and let editor handle cursor movement
                    if matches!(
                        key,
                        Key::Left | Key::Right | Key::Home | Key::End | Key::PageUp | Key::PageDown
                    ) {
                        if popup_visible {
                            intellisense_popup_for_handle.borrow_mut().hide();
                            *completion_range_for_handle.borrow_mut() = None;
                        }
                        return false;
                    }

                    // Skip if these keys (already handled in KeyDown)
                    if popup_visible
                        && matches!(
                            key,
                            Key::Up
                                | Key::Down
                                | Key::Escape
                                | Key::Enter
                                | Key::KPEnter
                                | Key::Tab
                        )
                    {
                        return true;
                    }

                    // Handle typing - update intellisense filter
                    let cursor_pos = ed.insert_position() as usize;
                    let (word, _, _) = get_word_at_cursor(&text, cursor_pos);
                    let context = detect_sql_context(&text, cursor_pos);

                    if key == Key::BackSpace || key == Key::Delete {
                        // After backspace/delete, re-evaluate
                        if word.len() >= 2 {
                            Self::trigger_intellisense(
                                ed,
                                &buffer_for_handle,
                                &intellisense_data_for_handle,
                                &intellisense_popup_for_handle,
                                &completion_range_for_handle,
                                &column_sender_for_handle,
                                &connection_for_handle,
                            );
                        } else {
                            intellisense_popup_for_handle.borrow_mut().hide();
                            *completion_range_for_handle.borrow_mut() = None;
                        }
                    } else {
                        let typed_char = fltk::app::event_text().chars().next().or_else(|| {
                            let bits = key.bits();
                            if bits >= 0x20 && bits <= 0x7e {
                                key.to_char()
                            } else {
                                None
                            }
                        });

                        if let Some(ch) = typed_char {
                            if ch.is_alphanumeric() || ch == '_' {
                                // Alphanumeric typed - show/update popup if word is long enough
                                if word.len() >= 2 {
                                    Self::trigger_intellisense(
                                        ed,
                                        &buffer_for_handle,
                                        &intellisense_data_for_handle,
                                        &intellisense_popup_for_handle,
                                        &completion_range_for_handle,
                                        &column_sender_for_handle,
                                        &connection_for_handle,
                                    );
                                } else {
                                    intellisense_popup_for_handle.borrow_mut().hide();
                                    *completion_range_for_handle.borrow_mut() = None;
                                }
                            } else {
                                // Non-identifier character (space, punctuation, etc.)
                                // Close popup - user is done with this word
                                intellisense_popup_for_handle.borrow_mut().hide();
                                *completion_range_for_handle.borrow_mut() = None;
                            }
                        }
                    }

                    Self::maybe_prefetch_columns_for_word(
                        context,
                        &word,
                        &intellisense_data_for_handle,
                        &column_sender_for_handle,
                        &connection_for_handle,
                    );
                    false
                }
                Event::Shortcut => {
                    let key = fltk::app::event_key();
                    let popup_visible = intellisense_popup_for_handle.borrow().is_visible();
                    let state = fltk::app::event_state();
                    let ctrl_or_cmd = state.contains(fltk::enums::Shortcut::Ctrl)
                        || state.contains(fltk::enums::Shortcut::Command);
                    
                    // If intellisense is visible, consume Enter/Tab to prevent them from reaching other handlers
                    if popup_visible
                        && matches!(
                            key,
                            Key::Up | Key::Down | Key::Enter | Key::KPEnter | Key::Tab
                        )
                    {
                        return true;
                    }

                    if ctrl_or_cmd && matches!(key, Key::Enter | Key::KPEnter) {
                        if *ctrl_enter_handled_for_handle.borrow() {
                            return true;
                        }
                        *ctrl_enter_handled_for_handle.borrow_mut() = true;
                        widget_for_shortcuts.execute_statement_at_cursor();
                        return true;
                    }

                    false
                }
                Event::Paste => {
                    false
                }
                _ => false,
            }
        });

    }

    fn trigger_intellisense(
        editor: &TextEditor,
        buffer: &TextBuffer,
        intellisense_data: &Rc<RefCell<IntellisenseData>>,
        intellisense_popup: &Rc<RefCell<IntellisensePopup>>,
        completion_range: &Rc<RefCell<Option<(usize, usize)>>>,
        column_sender: &mpsc::Sender<ColumnLoadUpdate>,
        connection: &SharedConnection,
    ) {
        let cursor_pos = editor.insert_position() as usize;
        let text = buffer.text();
        let (word, start, _) = get_word_at_cursor(&text, cursor_pos);
        let qualifier = Self::qualifier_before_word(&text, start);
        let prefix = if word.is_empty() {
            if qualifier.is_none() {
                *completion_range.borrow_mut() = None;
                return;
            }
            String::new()
        } else {
            word
        };

        let context = detect_sql_context(&text, cursor_pos);
        let text_before_cursor: String = text.chars().take(cursor_pos).collect();
        let table_refs = Self::collect_table_references(&text_before_cursor);
        let column_tables = Self::resolve_column_tables(&table_refs, qualifier.as_deref());
        let include_columns = qualifier.is_some()
            || matches!(context, SqlContext::ColumnName | SqlContext::ColumnOrAll);

        if include_columns {
            for table in &column_tables {
                Self::request_table_columns(table, intellisense_data, column_sender, connection);
            }
        }

        let suggestions = {
            let data = intellisense_data.borrow();
            let column_scope = if include_columns && !column_tables.is_empty() {
                Some(column_tables.as_slice())
            } else {
                None
            };
            data.get_suggestions(&prefix, include_columns, column_scope)
        };

        if suggestions.is_empty() {
            intellisense_popup.borrow_mut().hide();
            *completion_range.borrow_mut() = None;
            return;
        }

        // Get cursor position in editor's local coordinates (already window-relative in FLTK)
        let (cursor_x, cursor_y) = editor.position_to_xy(editor.insert_position());

        // Get window's screen coordinates
        let (win_x, win_y) = editor
            .window()
            .map(|win| (win.x_root(), win.y_root()))
            .unwrap_or((0, 0));

        let popup_width = 320;
        let popup_height = (suggestions.len().min(10) * 20 + 10) as i32;

        // Calculate absolute screen position
        let mut popup_x = win_x + cursor_x;
        let mut popup_y = win_y + cursor_y + 20;

        if let Some(win) = editor.window() {
            let win_w = win.w();
            let win_h = win.h();
            let max_x = (win_x + win_w - popup_width).max(win_x);
            let max_y = (win_y + win_h - popup_height).max(win_y);
            popup_x = popup_x.clamp(win_x, max_x);
            popup_y = popup_y.clamp(win_y, max_y);
        }

        intellisense_popup
            .borrow_mut()
            .show_suggestions(suggestions, popup_x, popup_y);
        let completion_start = if prefix.is_empty() { cursor_pos } else { start };
        *completion_range.borrow_mut() = Some((completion_start, cursor_pos));
        let mut editor = editor.clone();
        let _ = editor.take_focus();
    }

    fn maybe_prefetch_columns_for_word(
        context: SqlContext,
        word: &str,
        intellisense_data: &Rc<RefCell<IntellisenseData>>,
        column_sender: &mpsc::Sender<ColumnLoadUpdate>,
        connection: &SharedConnection,
    ) {
        if !matches!(context, SqlContext::TableName) || word.is_empty() {
            return;
        }

        let should_prefetch = {
            let data = intellisense_data.borrow();
            data.is_known_relation(word)
        };

        if should_prefetch {
            Self::request_table_columns(word, intellisense_data, column_sender, connection);
        }
    }

    fn request_table_columns(
        table_name: &str,
        intellisense_data: &Rc<RefCell<IntellisenseData>>,
        column_sender: &mpsc::Sender<ColumnLoadUpdate>,
        connection: &SharedConnection,
    ) {
        let table_key = table_name
            .split('.')
            .last()
            .unwrap_or(table_name)
            .to_string();
        let should_load = {
            let mut data = intellisense_data.borrow_mut();
            if !data.is_known_relation(&table_key) {
                return;
            }
            data.mark_columns_loading(&table_key)
        };

        if !should_load {
            return;
        }

        let connection = connection.clone();
        let sender = column_sender.clone();
        let table_key_for_thread = table_key.clone();
        thread::spawn(move || {
            let conn_guard = lock_connection(&connection);
            let conn = conn_guard.get_connection();
            drop(conn_guard);

            let columns = if let Some(conn) = conn {
                match crate::db::ObjectBrowser::get_table_columns(
                    conn.as_ref(),
                    &table_key_for_thread,
                ) {
                    Ok(cols) => cols.into_iter().map(|col| col.name).collect(),
                    Err(_) => Vec::new(),
                }
            } else {
                Vec::new()
            };

            let _ = sender.send(ColumnLoadUpdate {
                table: table_key_for_thread,
                columns,
            });
            app::awake();
        });
    }

    fn qualifier_before_word(text: &str, word_start: usize) -> Option<String> {
        if word_start == 0 {
            return None;
        }

        let chars: Vec<char> = text.chars().collect();
        if chars.get(word_start - 1)? != &'.' {
            return None;
        }

        let mut start = word_start - 1;
        while start > 0 {
            let ch = chars[start - 1];
            if !ch.is_alphanumeric() && ch != '_' {
                break;
            }
            start -= 1;
        }

        if start == word_start - 1 {
            return None;
        }

        let qualifier: String = chars[start..word_start - 1].iter().collect();
        if qualifier.is_empty() {
            None
        } else {
            Some(qualifier)
        }
    }

    fn collect_table_references(text: &str) -> Vec<TableReference> {
        let tokens = Self::tokenize_sql(text);
        let mut references = Vec::new();
        let mut expect_table = false;
        let mut idx = 0;

        while idx < tokens.len() {
            match &tokens[idx] {
                SqlToken::Symbol(sym) if sym == ";" => {
                    references.clear();
                    expect_table = false;
                    idx += 1;
                    continue;
                }
                SqlToken::Word(word) => {
                    let upper = word.to_uppercase();
                    if Self::is_table_intro_keyword(&upper) {
                        expect_table = true;
                        idx += 1;
                        continue;
                    }
                    if Self::is_table_stop_keyword(&upper) {
                        expect_table = false;
                        idx += 1;
                        continue;
                    }
                    if expect_table {
                        if let Some((table, next_idx)) = Self::parse_table_name(&tokens, idx) {
                            let (alias, after_alias) = Self::parse_alias(&tokens, next_idx);
                            references.push(TableReference { table, alias });
                            if let Some(SqlToken::Symbol(sym)) = tokens.get(after_alias) {
                                if sym == "," {
                                    expect_table = true;
                                    idx = after_alias + 1;
                                    continue;
                                }
                            }
                            expect_table = false;
                            idx = after_alias;
                            continue;
                        }
                        expect_table = false;
                    }
                }
                _ => {}
            }
            idx += 1;
        }

        references
    }

    fn resolve_column_tables(
        table_refs: &[TableReference],
        qualifier: Option<&str>,
    ) -> Vec<String> {
        let mut tables = Vec::new();
        let mut seen = HashSet::new();

        if let Some(qualifier) = qualifier {
            let qualifier_upper = qualifier.to_uppercase();
            for table_ref in table_refs {
                let table_upper = table_ref.table.to_uppercase();
                let alias_upper = table_ref.alias.as_ref().map(|a| a.to_uppercase());
                if table_upper == qualifier_upper
                    || alias_upper.as_deref() == Some(qualifier_upper.as_str())
                {
                    if seen.insert(table_upper) {
                        tables.push(table_ref.table.clone());
                    }
                    return tables;
                }
            }
            if seen.insert(qualifier_upper) {
                tables.push(qualifier.to_string());
            }
            return tables;
        }

        for table_ref in table_refs {
            let table_upper = table_ref.table.to_uppercase();
            if seen.insert(table_upper) {
                tables.push(table_ref.table.clone());
            }
        }

        tables
    }

    fn parse_table_name(tokens: &[SqlToken], start: usize) -> Option<(String, usize)> {
        match tokens.get(start) {
            Some(SqlToken::Symbol(sym)) if sym == "(" => None,
            Some(SqlToken::Word(word)) => {
                let mut table = word.clone();
                let mut idx = start + 1;
                if matches!(tokens.get(idx), Some(SqlToken::Symbol(sym)) if sym == ".") {
                    if let Some(SqlToken::Word(name)) = tokens.get(idx + 1) {
                        table = name.clone();
                        idx += 2;
                    }
                }
                Some((table, idx))
            }
            _ => None,
        }
    }

    fn parse_alias(tokens: &[SqlToken], start: usize) -> (Option<String>, usize) {
        match tokens.get(start) {
            Some(SqlToken::Word(word)) => {
                let upper = word.to_uppercase();
                if upper == "AS" {
                    if let Some(SqlToken::Word(alias)) = tokens.get(start + 1) {
                        return (Some(alias.clone()), start + 2);
                    }
                    return (None, start + 1);
                }
                if !Self::is_alias_breaker(&upper) {
                    return (Some(word.clone()), start + 1);
                }
            }
            _ => {}
        }

        (None, start)
    }

    fn is_table_intro_keyword(word: &str) -> bool {
        matches!(word, "FROM" | "JOIN" | "INTO" | "UPDATE")
    }

    fn is_table_stop_keyword(word: &str) -> bool {
        matches!(
            word,
            "WHERE"
                | "GROUP"
                | "ORDER"
                | "HAVING"
                | "CONNECT"
                | "START"
                | "UNION"
                | "INTERSECT"
                | "EXCEPT"
                | "MINUS"
                | "FETCH"
                | "FOR"
                | "WINDOW"
                | "QUALIFY"
                | "LIMIT"
                | "OFFSET"
                | "RETURNING"
                | "VALUES"
                | "SET"
        )
    }

    fn is_alias_breaker(word: &str) -> bool {
        matches!(
            word,
            "ON"
                | "JOIN"
                | "INNER"
                | "LEFT"
                | "RIGHT"
                | "FULL"
                | "CROSS"
                | "OUTER"
                | "WHERE"
                | "GROUP"
                | "ORDER"
                | "HAVING"
                | "CONNECT"
                | "START"
                | "UNION"
                | "INTERSECT"
                | "EXCEPT"
                | "MINUS"
                | "FETCH"
                | "FOR"
                | "WINDOW"
                | "QUALIFY"
                | "LIMIT"
                | "OFFSET"
                | "RETURNING"
                | "VALUES"
                | "SET"
                | "USING"
        )
    }

    /// Show quick describe dialog for a table (F4 functionality)
    fn show_quick_describe(conn: &oracle::Connection, object_name: &str) {
        use crate::db::ObjectBrowser;
        use fltk::{prelude::*, text::TextDisplay, window::Window};

        // Try to get table structure
        match ObjectBrowser::get_table_structure(conn, object_name) {
            Ok(columns) => {
                if columns.is_empty() {
                    fltk::dialog::message_default(&format!(
                        "No table or view found with name: {}",
                        object_name.to_uppercase()
                    ));
                    return;
                }

                // Build description text
                let mut info = format!("=== {} ===\n\n", object_name.to_uppercase());
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

                // Show in a dialog
                let current_group = fltk::group::Group::try_current();
                fltk::group::Group::set_current(None::<&fltk::group::Group>);
                
                let mut dialog = Window::default()
                    .with_size(600, 400)
                    .with_label(&format!("Describe: {}", object_name.to_uppercase()))
                    .center_screen();
                dialog.set_color(theme::panel_raised());
                dialog.make_modal(true);
                dialog.begin();

                let mut display = TextDisplay::default().with_pos(10, 10).with_size(580, 340);
                display.set_color(theme::editor_bg());
                display.set_text_color(theme::text_primary());
                display.set_text_font(fltk::enums::Font::Courier);
                display.set_text_size(12);

                let mut buffer = fltk::text::TextBuffer::default();
                buffer.set_text(&info);
                display.set_buffer(buffer);

                let mut close_btn = fltk::button::Button::default()
                    .with_pos(250, 360)
                    .with_size(100, 20)
                    .with_label("Close");
                close_btn.set_color(theme::button_secondary());
                close_btn.set_label_color(theme::text_primary());

                let (sender, receiver) = mpsc::channel::<()>();
                close_btn.set_callback(move |_| {
                    let _ = sender.send(());
                });

                dialog.end();
                dialog.show();
                fltk::group::Group::set_current(current_group.as_ref());

                while dialog.shown() {
                    fltk::app::wait();
                    if receiver.try_recv().is_ok() {
                        dialog.hide();
                    }
                }
            }
            Err(_) => {
                fltk::dialog::message_default(&format!(
                    "Object not found or not accessible: {}",
                    object_name.to_uppercase()
                ));
            }
        }
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
        let buffer = self.buffer.clone();
        let sql = if buffer.selected() {
            buffer.selection_text()
        } else {
            buffer.text()
        };
        if sql.trim().is_empty() {
            fltk::dialog::alert_default("No SQL to explain");
            return;
        }

        let conn_guard = lock_connection(&self.connection);
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            match QueryExecutor::get_explain_plan(db_conn.as_ref(), &sql) {
                Ok(plan_lines) => {
                    let plan_text = if plan_lines.is_empty() {
                        "No plan output.".to_string()
                    } else {
                        plan_lines.join("\n")
                    };
                    Self::show_plan_dialog(&plan_text);
                }
                Err(err) => {
                    fltk::dialog::alert_default(&format!("Failed to explain plan: {}", err));
                }
            }
        }
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
        display.set_text_size(12);

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
        let conn_guard = lock_connection(&self.connection);
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            self.emit_status("Commit failed: not connected");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            if let Err(err) = db_conn.commit() {
                fltk::dialog::alert_default(&format!("Commit failed: {}", err));
                self.emit_status("Commit failed");
            } else {
                self.emit_status("Committed");
            }
        } else {
            self.emit_status("Commit failed");
        }
    }

    pub fn rollback(&self) {
        let conn_guard = lock_connection(&self.connection);
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            self.emit_status("Rollback failed: not connected");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            if let Err(err) = db_conn.rollback() {
                fltk::dialog::alert_default(&format!("Rollback failed: {}", err));
                self.emit_status("Rollback failed");
            } else {
                self.emit_status("Rolled back");
            }
        } else {
            self.emit_status("Rollback failed");
        }
    }

    pub fn cancel_current(&self) {
        if !*self.query_running.borrow() {
            fltk::dialog::alert_default("No query is running");
            return;
        }

        let conn_guard = lock_connection(&self.connection);
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            if let Err(err) = db_conn.break_execution() {
                fltk::dialog::alert_default(&format!("Cancel failed: {}", err));
            }
        }
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

    pub fn hide_intellisense_if_outside(&self, x: i32, y: i32) {
        let mut popup = self.intellisense_popup.borrow_mut();
        if !popup.is_visible() {
            return;
        }
        if popup.contains_point(x, y) {
            return;
        }
        popup.hide();
        *self.completion_range.borrow_mut() = None;
    }

    pub fn hide_intellisense(&self) {
        let mut popup = self.intellisense_popup.borrow_mut();
        if popup.is_visible() {
            popup.hide();
        }
        *self.completion_range.borrow_mut() = None;
    }

    #[allow(dead_code)]
    pub fn update_intellisense_data(&mut self, data: IntellisenseData) {
        *self.intellisense_data.borrow_mut() = data;
    }

    pub fn get_intellisense_data(&self) -> Rc<RefCell<IntellisenseData>> {
        self.intellisense_data.clone()
    }

    #[allow(dead_code)]
    pub fn update_highlight_data(&mut self, data: HighlightData) {
        self.highlighter.borrow_mut().set_highlight_data(data);
        // Re-highlight current text
        let text = self.buffer.text();
        let mut style_buffer = self.style_buffer.clone();
        self.highlighter
            .borrow()
            .highlight(&text, &mut style_buffer);
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

    pub fn get_style_buffer(&self) -> TextBuffer {
        self.style_buffer.clone()
    }

    #[allow(dead_code)]
    pub fn refresh_highlighting(&self) {
        let text = self.buffer.text();
        self.highlighter
            .borrow()
            .highlight(&text, &mut self.style_buffer.clone());
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

    pub fn show_intellisense(&self) {
        Self::trigger_intellisense(
            &self.editor,
            &self.buffer,
            &self.intellisense_data,
            &self.intellisense_popup,
            &self.completion_range,
            &self.column_sender,
            &self.connection,
        );
    }

    pub fn quick_describe_at_cursor(&self) {
        let text = self.buffer.text();
        let cursor_pos = self.editor.insert_position() as usize;
        let (word, _, _) = get_word_at_cursor(&text, cursor_pos);
        if word.is_empty() {
            return;
        }

        let conn_guard = lock_connection(&self.connection);
        if conn_guard.is_connected() {
            if let Some(db_conn) = conn_guard.get_connection() {
                Self::show_quick_describe(db_conn.as_ref(), &word);
            }
        } else {
            fltk::dialog::alert_default("Not connected to database");
        }
    }

    pub fn execute_sql_text(&self, sql: &str) {
        self.execute_sql(sql);
    }

    pub fn focus(&mut self) {
        self.group.show();
        let _ = self.editor.take_focus();
    }

    pub fn execute_current(&self) {
        let sql = self.buffer.text();
        self.execute_sql(&sql);
    }

    pub fn execute_statement_at_cursor(&self) {
        let sql = self.buffer.text();
        let cursor_pos = self.editor.insert_position() as usize;
        if let Some(statement) = QueryExecutor::statement_at_cursor(&sql, cursor_pos) {
            self.execute_sql(&statement);
        } else {
            fltk::dialog::alert_default("No SQL at cursor");
        }
    }

    pub fn execute_selected(&self) {
        let mut buffer = self.buffer.clone();
        if !buffer.selected() {
            fltk::dialog::alert_default("No SQL selected");
            return;
        }

        let selection = buffer.selection_position();
        let insert_pos = self.editor.insert_position();
        let sql = buffer.selection_text();
        self.execute_sql(&sql);
        if let Some((start, end)) = selection {
            buffer.select(start, end);
            let mut editor = self.editor.clone();
            editor.set_insert_position(insert_pos);
            editor.show_insert_position();
        }
    }

    pub fn format_selected_sql(&self) {
        let mut buffer = self.buffer.clone();
        let selection = buffer.selection_position();
        let (start, end, source, select_formatted) = match selection {
            Some((start, end)) if start != end => {
                let (start, end) = if start <= end { (start, end) } else { (end, start) };
                (start, end, buffer.selection_text(), true)
            }
            _ => {
                let text = buffer.text();
                let end = buffer.length();
                (0, end, text, false)
            }
        };

        let formatted = Self::format_sql_basic(&source);
        if formatted == source {
            return;
        }

        let mut editor = self.editor.clone();
        let original_pos = editor.insert_position();
        buffer.replace(start, end, &formatted);

        if select_formatted {
            buffer.select(start, start + formatted.len() as i32);
            editor.set_insert_position(start + formatted.len() as i32);
        } else {
            let new_pos = (original_pos as usize).min(formatted.len()) as i32;
            editor.set_insert_position(new_pos);
        }
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    pub fn toggle_comment(&self) {
        let mut buffer = self.buffer.clone();
        let mut editor = self.editor.clone();
        let selection = buffer.selection_position();
        let had_selection = matches!(selection, Some((start, end)) if start != end);
        let original_pos = editor.insert_position();

        let (start, end) = if let Some((start, end)) = selection {
            if start <= end {
                (start, end)
            } else {
                (end, start)
            }
        } else {
            let line_start = buffer.line_start(original_pos);
            let line_end = buffer.line_end(original_pos);
            (line_start, line_end)
        };

        let line_start = buffer.line_start(start);
        let line_end = buffer.line_end(end);
        let text = buffer.text_range(line_start, line_end).unwrap_or_default();
        let ends_with_newline = text.ends_with('\n');
        let lines: Vec<&str> = text.lines().collect();

        let all_commented = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with("--"));

        let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
        for line in lines {
            if line.trim().is_empty() {
                new_lines.push(line.to_string());
                continue;
            }

            let prefix_len = line.len() - line.trim_start().len();
            let prefix = &line[..prefix_len];
            let trimmed = &line[prefix_len..];

            if all_commented {
                let uncommented = trimmed.strip_prefix("--").unwrap_or(trimmed);
                let uncommented = if uncommented.starts_with(' ') {
                    &uncommented[1..]
                } else {
                    uncommented
                };
                new_lines.push(format!("{}{}", prefix, uncommented));
            } else if trimmed.starts_with("--") {
                new_lines.push(line.to_string());
            } else {
                new_lines.push(format!("{}-- {}", prefix, trimmed));
            }
        }

        let mut new_text = new_lines.join("\n");
        if ends_with_newline {
            new_text.push('\n');
        }

        buffer.replace(line_start, line_end, &new_text);
        let new_end = line_start + new_text.len() as i32;
        if had_selection {
            buffer.select(line_start, new_end);
            editor.set_insert_position(new_end);
        } else {
            let delta = new_text.len() as i32 - (line_end - line_start);
            let new_pos = if original_pos >= line_start {
                original_pos + delta
            } else {
                original_pos
            };
            editor.set_insert_position(new_pos);
        }
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    pub fn convert_selection_case(&self, to_upper: bool) {
        let mut buffer = self.buffer.clone();
        let selection = buffer.selection_position();
        let (start, end) = match selection {
            Some((start, end)) if start != end => {
                if start <= end {
                    (start, end)
                } else {
                    (end, start)
                }
            }
            _ => {
                fltk::dialog::alert_default("No SQL selected");
                return;
            }
        };

        let selected = buffer.selection_text();
        let converted = if to_upper {
            selected.to_uppercase()
        } else {
            selected.to_lowercase()
        };

        if converted == selected {
            return;
        }

        buffer.replace(start, end, &converted);
        buffer.select(start, start + converted.len() as i32);

        let mut editor = self.editor.clone();
        editor.set_insert_position(start + converted.len() as i32);
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    fn format_sql_basic(sql: &str) -> String {
        let mut formatted = String::new();
        let statements = QueryExecutor::split_statements_with_blocks(sql);
        if statements.is_empty() {
            return String::new();
        }

        let trailing_semicolon = sql.trim_end().ends_with(';');
        let statement_count = statements.len();
        for (idx, statement) in statements.iter().enumerate() {
            let formatted_statement = Self::format_statement(statement);
            formatted.push_str(&formatted_statement);
            if idx + 1 < statement_count || trailing_semicolon {
                formatted.push(';');
                if idx + 1 < statement_count {
                    formatted.push('\n');
                }
            }
        }

        formatted
    }

    fn format_statement(statement: &str) -> String {
        let clause_keywords = [
            "SELECT", "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "UNION", "INTERSECT", "MINUS",
            "INSERT", "UPDATE", "DELETE", "MERGE", "VALUES", "SET", "INTO", "WITH",
        ];
        let join_modifiers = ["LEFT", "RIGHT", "FULL", "INNER", "CROSS"];
        let join_keyword = "JOIN";
        let outer_keyword = "OUTER";
        let condition_keywords = ["ON", "AND", "OR", "WHEN", "ELSE"];
        let block_start = ["BEGIN", "DECLARE", "LOOP", "CASE"];
        let block_end = ["END"];

        let tokens = Self::tokenize_sql(statement);
        let mut out = String::new();
        let mut indent_level = 0usize;
        let mut suppress_comma_break_depth = 0usize;
        let mut paren_stack: Vec<bool> = Vec::new();
        let mut at_line_start = true;
        let mut needs_space = false;
        let mut line_indent = 0usize;
        let mut join_modifier_active = false;

        let newline_with = |out: &mut String,
                            indent_level: usize,
                            extra: usize,
                            at_line_start: &mut bool,
                            needs_space: &mut bool,
                            line_indent: &mut usize| {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            *line_indent = indent_level + extra;
            *at_line_start = true;
            *needs_space = false;
        };

        let ensure_indent =
            |out: &mut String, at_line_start: &mut bool, line_indent: usize| {
                if *at_line_start {
                    out.push_str(&" ".repeat(line_indent * 4));
                    *at_line_start = false;
                }
            };

        let trim_trailing_space = |out: &mut String| {
            while out.ends_with(' ') {
                out.pop();
            }
        };

        let mut idx = 0;
        while idx < tokens.len() {
            let token = tokens[idx].clone();
            let next_word_upper = tokens[idx + 1..]
                .iter()
                .find_map(|t| match t {
                    SqlToken::Word(w) => Some(w.to_uppercase()),
                    _ => None,
                });

            match token {
                SqlToken::Word(word) => {
                    let upper = word.to_uppercase();
                    let is_keyword = SQL_KEYWORDS.iter().any(|&kw| kw == upper);
                    if block_end.contains(&upper.as_str()) {
                        if indent_level > 0 {
                            indent_level -= 1;
                        }
                        newline_with(
                            &mut out,
                            indent_level,
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if clause_keywords.contains(&upper.as_str()) {
                        newline_with(
                            &mut out,
                            indent_level,
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if condition_keywords.contains(&upper.as_str()) {
                        newline_with(
                            &mut out,
                            indent_level,
                            1,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if upper == join_keyword {
                        if !join_modifier_active {
                            newline_with(
                                &mut out,
                                indent_level,
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        join_modifier_active = false;
                    } else if join_modifiers.contains(&upper.as_str()) {
                        if matches!(next_word_upper.as_deref(), Some("JOIN" | "OUTER")) {
                            newline_with(
                                &mut out,
                                indent_level,
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            join_modifier_active = true;
                        }
                    } else if upper == outer_keyword {
                        if matches!(next_word_upper.as_deref(), Some("JOIN")) && !join_modifier_active {
                            newline_with(
                                &mut out,
                                indent_level,
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            join_modifier_active = true;
                        }
                    } else if block_start.contains(&upper.as_str()) {
                        newline_with(
                            &mut out,
                            indent_level,
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    }

                    ensure_indent(&mut out, &mut at_line_start, line_indent);
                    if needs_space {
                        out.push(' ');
                    }
                    if is_keyword {
                        out.push_str(&upper);
                    } else {
                        out.push_str(&word);
                    }
                    needs_space = true;

                    if block_start.contains(&upper.as_str()) {
                        indent_level += 1;
                    }
                }
                SqlToken::String(literal) => {
                    ensure_indent(&mut out, &mut at_line_start, line_indent);
                    if needs_space {
                        out.push(' ');
                    }
                    out.push_str(&literal);
                    needs_space = true;
                    if literal.contains('\n') {
                        at_line_start = true;
                    }
                }
                SqlToken::Comment(comment) => {
                    if !at_line_start {
                        out.push(' ');
                    }
                    ensure_indent(&mut out, &mut at_line_start, line_indent);
                    out.push_str(&comment);
                    needs_space = true;
                    if comment.ends_with('\n') || comment.contains('\n') {
                        at_line_start = true;
                        needs_space = false;
                    }
                }
                SqlToken::Symbol(sym) => {
                    match sym.as_str() {
                        "," => {
                            trim_trailing_space(&mut out);
                            out.push(',');
                            if suppress_comma_break_depth == 0 {
                                newline_with(
                                    &mut out,
                                    indent_level,
                                    1,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            } else {
                                out.push(' ');
                                needs_space = false;
                            }
                        }
                        ";" => {
                            trim_trailing_space(&mut out);
                            out.push(';');
                            newline_with(
                                &mut out,
                                indent_level,
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        "(" => {
                            let is_subquery = matches!(
                                next_word_upper.as_deref(),
                                Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "MERGE")
                            );
                            if needs_space {
                                out.push(' ');
                            }
                            out.push('(');
                            paren_stack.push(is_subquery);
                            if is_subquery {
                                indent_level += 1;
                                newline_with(
                                    &mut out,
                                    indent_level,
                                    0,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            } else {
                                suppress_comma_break_depth += 1;
                            }
                            needs_space = false;
                        }
                        ")" => {
                            trim_trailing_space(&mut out);
                            let was_subquery = paren_stack.pop().unwrap_or(false);
                            if was_subquery {
                                if indent_level > 0 {
                                    indent_level -= 1;
                                }
                                newline_with(
                                    &mut out,
                                    indent_level,
                                    0,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                                ensure_indent(&mut out, &mut at_line_start, line_indent);
                            } else if suppress_comma_break_depth > 0 {
                                suppress_comma_break_depth -= 1;
                            }
                            out.push(')');
                            needs_space = true;
                        }
                        "." => {
                            trim_trailing_space(&mut out);
                            out.push('.');
                            needs_space = false;
                        }
                        _ => {
                            ensure_indent(&mut out, &mut at_line_start, line_indent);
                            if needs_space {
                                out.push(' ');
                            }
                            out.push_str(&sym);
                            needs_space = true;
                        }
                    }
                }
            }

            idx += 1;
        }

        out.trim_end().to_string()
    }

    fn tokenize_sql(sql: &str) -> Vec<SqlToken> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = sql.chars().collect();
        let mut i = 0;
        let mut current = String::new();

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;

        let flush_word = |current: &mut String, tokens: &mut Vec<SqlToken>| {
            if !current.is_empty() {
                tokens.push(SqlToken::Word(std::mem::take(current)));
            }
        };

        while i < chars.len() {
            let c = chars[i];
            let next = if i + 1 < chars.len() {
                Some(chars[i + 1])
            } else {
                None
            };

            if in_line_comment {
                current.push(c);
                if c == '\n' {
                    tokens.push(SqlToken::Comment(std::mem::take(&mut current)));
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                current.push(c);
                if c == '*' && next == Some('/') {
                    current.push('/');
                    tokens.push(SqlToken::Comment(std::mem::take(&mut current)));
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                current.push(c);
                if c == '\'' {
                    if next == Some('\'') {
                        current.push('\'');
                        i += 2;
                        continue;
                    }
                    tokens.push(SqlToken::String(std::mem::take(&mut current)));
                    in_single_quote = false;
                    i += 1;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                current.push(c);
                if c == '"' {
                    if next == Some('"') {
                        current.push('"');
                        i += 2;
                        continue;
                    }
                    tokens.push(SqlToken::String(std::mem::take(&mut current)));
                    in_double_quote = false;
                    i += 1;
                    continue;
                }
                i += 1;
                continue;
            }

            if c.is_whitespace() {
                flush_word(&mut current, &mut tokens);
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                flush_word(&mut current, &mut tokens);
                in_line_comment = true;
                current.push('-');
                current.push('-');
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                flush_word(&mut current, &mut tokens);
                in_block_comment = true;
                current.push('/');
                current.push('*');
                i += 2;
                continue;
            }

            if c == '\'' {
                flush_word(&mut current, &mut tokens);
                in_single_quote = true;
                current.push('\'');
                i += 1;
                continue;
            }

            if c == '"' {
                flush_word(&mut current, &mut tokens);
                in_double_quote = true;
                current.push('"');
                i += 1;
                continue;
            }

            if c.is_alphanumeric() || c == '_' || c == '$' || c == '#' {
                current.push(c);
                i += 1;
                continue;
            }

            flush_word(&mut current, &mut tokens);

            let sym = match (c, next) {
                ('<', Some('=')) => Some("<=".to_string()),
                ('>', Some('=')) => Some(">=".to_string()),
                ('<', Some('>')) => Some("<>".to_string()),
                ('!', Some('=')) => Some("!=".to_string()),
                ('|', Some('|')) => Some("||".to_string()),
                (':', Some('=')) => Some(":=".to_string()),
                ('=', Some('>')) => Some("=>".to_string()),
                _ => None,
            };

            if let Some(sym) = sym {
                tokens.push(SqlToken::Symbol(sym));
                i += 2;
                continue;
            }

            tokens.push(SqlToken::Symbol(c.to_string()));
            i += 1;
        }

        flush_word(&mut current, &mut tokens);
        tokens
    }

    fn execute_sql(&self, sql: &str) {
        if sql.trim().is_empty() {
            return;
        }

        if *self.query_running.borrow() {
            fltk::dialog::alert_default("A query is already running");
            return;
        }

        let conn_guard = lock_connection(&self.connection);
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        let conn_name = conn_guard.get_info().name.clone();
        let auto_commit = conn_guard.auto_commit();
        let query_timeout = Self::parse_timeout(&self.timeout_input.value());

        if let Some(db_conn) = conn_guard.get_connection() {
            let sql_text = sql.to_string();
            let sender = self.progress_sender.clone();
            let conn = db_conn.clone();
            let query_running = self.query_running.clone();

            *query_running.borrow_mut() = true;

            // Change cursor to wait and flush UI before executing query
            set_cursor(Cursor::Wait);
            app::flush();

            thread::spawn(move || {
                let statements = QueryExecutor::split_statements_with_blocks(&sql_text);
                if statements.is_empty() {
                    let _ = sender.send(QueryProgress::BatchFinished);
                    app::awake();
                    return;
                }

                let _ = sender.send(QueryProgress::BatchStart);
                app::awake();

                let previous_timeout = conn.call_timeout().unwrap_or(None);
                if let Err(err) = conn.set_call_timeout(query_timeout) {
                    let _ = sender.send(QueryProgress::StatementFinished {
                        index: 0,
                        result: QueryResult::new_error(&sql_text, &err.to_string()),
                        connection_name: conn_name.clone(),
                    });
                    let _ = sender.send(QueryProgress::BatchFinished);
                    app::awake();
                    let _ = conn.set_call_timeout(previous_timeout);
                    return;
                }

                for (index, statement) in statements.iter().enumerate() {
                    let trimmed = statement.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let is_select = QueryExecutor::is_select_statement(trimmed);

                    let _ = sender.send(QueryProgress::StatementStart { index });
                    app::awake();

                    let mut buffered_rows: Vec<Vec<String>> = Vec::new();
                    let mut last_flush = std::time::Instant::now();
                    let mut timed_out = false;

                    let mut result = if is_select {
                        match QueryExecutor::execute_select_streaming(
                            conn.as_ref(),
                            trimmed,
                            &mut |columns| {
                                let names = columns
                                    .iter()
                                    .map(|col| col.name.clone())
                                    .collect::<Vec<String>>();
                                let _ = sender.send(QueryProgress::SelectStart {
                                    index,
                                    columns: names,
                                });
                                app::awake();
                            },
                            &mut |row| {
                                buffered_rows.push(row);
                                if last_flush.elapsed() >= Duration::from_secs(1) {
                                    let rows = std::mem::take(&mut buffered_rows);
                                    let _ = sender.send(QueryProgress::Rows { index, rows });
                                    app::awake();
                                    last_flush = std::time::Instant::now();
                                }
                            },
                        ) {
                            Ok(result) => result,
                            Err(err) => {
                                timed_out = Self::is_timeout_error(&err);
                                let message = if timed_out {
                                    Self::timeout_message(query_timeout)
                                } else {
                                    err.to_string()
                                };
                                QueryResult::new_error(trimmed, &message)
                            }
                        }
                    } else {
                        match QueryExecutor::execute(conn.as_ref(), trimmed) {
                            Ok(result) => result,
                            Err(err) => {
                                timed_out = Self::is_timeout_error(&err);
                                let message = if timed_out {
                                    Self::timeout_message(query_timeout)
                                } else {
                                    err.to_string()
                                };
                                QueryResult::new_error(trimmed, &message)
                            }
                        }
                    };

                    if !buffered_rows.is_empty() {
                        let rows = std::mem::take(&mut buffered_rows);
                        let _ = sender.send(QueryProgress::Rows { index, rows });
                        app::awake();
                    }

                    if auto_commit && !result.is_select {
                        if let Err(err) = conn.commit() {
                            result = QueryResult::new_error(
                                trimmed,
                                &format!("Auto-commit failed: {}", err),
                            );
                        } else {
                            result.message = format!("{} | Auto-commit applied", result.message);
                        }
                    }

                    let _ = sender.send(QueryProgress::StatementFinished {
                        index,
                        result,
                        connection_name: conn_name.clone(),
                    });
                    app::awake();

                    if timed_out {
                        let _ = conn.set_call_timeout(previous_timeout);
                        let _ = sender.send(QueryProgress::BatchFinished);
                        app::awake();
                        return;
                    }
                }

                let _ = conn.set_call_timeout(previous_timeout);
                let _ = sender.send(QueryProgress::BatchFinished);
                app::awake();
            });
        }
    }

    fn is_timeout_error(err: &OracleError) -> bool {
        let message = err.to_string();
        message.contains("DPI-1067") || message.contains("ORA-01013")
    }

    fn timeout_message(timeout: Option<Duration>) -> String {
        match timeout {
            Some(duration) => format!("Query timed out after {} seconds", duration.as_secs()),
            None => "Query timed out".to_string(),
        }
    }

    fn parse_timeout(value: &str) -> Option<Duration> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        let secs = match trimmed.parse::<u64>() {
            Ok(secs) => secs,
            Err(err) => {
                eprintln!("Invalid timeout value '{trimmed}': {err}");
                return None;
            }
        };
        if secs == 0 {
            None
        } else {
            Some(Duration::from_secs(secs))
        }
    }

    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: FnMut(QueryProgress) + 'static,
    {
        *self.progress_callback.borrow_mut() = Some(Box::new(callback));
    }
}
