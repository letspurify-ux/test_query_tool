use fltk::{
    app,
    button::Button,
    draw::set_cursor,
    enums::{Color, Cursor, Event, Font, FrameType, Key},
    frame::Frame,
    group::{Flex, FlexType, Pack, PackType},
    input::IntInput,
    prelude::*,
    text::{TextBuffer, TextEditor, WrapMode},
};
use oracle::Error as OracleError;
use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use crate::db::{QueryExecutor, QueryResult, SharedConnection};
use crate::ui::intellisense::{get_word_at_cursor, IntellisenseData, IntellisensePopup};
use crate::ui::query_history::QueryHistoryDialog;
use crate::ui::syntax_highlight::{create_style_table, HighlightData, SqlHighlighter};

#[derive(Clone)]
pub enum QueryProgress {
    BatchStart,
    StatementStart { index: usize },
    SelectStart { index: usize, columns: Vec<String> },
    Rows { index: usize, rows: Vec<Vec<String>> },
    StatementFinished { index: usize, result: QueryResult },
    BatchFinished,
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
    progress_sender: app::Sender<QueryProgress>,
    query_running: Rc<RefCell<bool>>,
    intellisense_data: Rc<RefCell<IntellisenseData>>,
    intellisense_popup: Rc<RefCell<IntellisensePopup>>,
    highlighter: Rc<RefCell<SqlHighlighter>>,
    timeout_input: IntInput,
}

impl SqlEditorWidget {
    pub fn new(connection: SharedConnection) -> Self {
        let mut group = Flex::default();
        group.set_type(FlexType::Column);
        group.set_margin(0);
        group.set_spacing(0);
        group.set_frame(FrameType::FlatBox);
        group.set_color(Color::from_rgb(37, 37, 38)); // Modern panel background

        // Button toolbar with modern styling
        let mut button_pack = Pack::default();
        button_pack.set_type(PackType::Horizontal);
        button_pack.set_spacing(6);

        let mut execute_btn = Button::default().with_size(90, 28).with_label("@> Execute");
        execute_btn.set_color(Color::from_rgb(0, 120, 212)); // Modern blue
        execute_btn.set_label_color(Color::White);
        execute_btn.set_frame(FrameType::RFlatBox);

        let mut cancel_btn = Button::default().with_size(80, 28).with_label("Cancel");
        cancel_btn.set_color(Color::from_rgb(196, 107, 34)); // Modern orange
        cancel_btn.set_label_color(Color::White);
        cancel_btn.set_frame(FrameType::RFlatBox);

        let mut explain_btn = Button::default().with_size(80, 28).with_label("Explain");
        explain_btn.set_color(Color::from_rgb(130, 80, 150)); // Modern purple
        explain_btn.set_label_color(Color::White);
        explain_btn.set_frame(FrameType::RFlatBox);

        let mut clear_btn = Button::default().with_size(70, 28).with_label("Clear");
        clear_btn.set_color(Color::from_rgb(55, 55, 58)); // Subtle gray
        clear_btn.set_label_color(Color::from_rgb(180, 180, 180));
        clear_btn.set_frame(FrameType::RFlatBox);

        let mut commit_btn = Button::default().with_size(80, 28).with_label("Commit");
        commit_btn.set_color(Color::from_rgb(34, 139, 34)); // Modern green
        commit_btn.set_label_color(Color::White);
        commit_btn.set_frame(FrameType::RFlatBox);

        let mut rollback_btn = Button::default().with_size(80, 28).with_label("Rollback");
        rollback_btn.set_color(Color::from_rgb(200, 60, 60)); // Modern red
        rollback_btn.set_label_color(Color::White);
        rollback_btn.set_frame(FrameType::RFlatBox);

        let mut timeout_label = Frame::default().with_size(85, 28);
        timeout_label.set_label("Timeout(s)");
        timeout_label.set_label_color(Color::from_rgb(160, 160, 160));

        let mut timeout_input = IntInput::default().with_size(55, 28);
        timeout_input.set_color(Color::from_rgb(45, 45, 48)); // Input background
        timeout_input.set_text_color(Color::from_rgb(212, 212, 212));
        timeout_input.set_tooltip("Call timeout in seconds (empty = no timeout)");

        button_pack.end();
        group.fixed(&button_pack, 34);

        // SQL Editor with modern styling
        let buffer = TextBuffer::default();
        let style_buffer = TextBuffer::default();
        let mut editor = TextEditor::default();
        editor.set_buffer(buffer.clone());
        editor.set_color(Color::from_rgb(30, 30, 30)); // Editor background
        editor.set_text_color(Color::from_rgb(212, 212, 212)); // Modern text
        editor.set_text_font(Font::Courier);
        editor.set_text_size(14);
        editor.set_cursor_color(Color::from_rgb(220, 220, 220));
        editor.wrap_mode(WrapMode::AtBounds, 0);

        // Modern selection color
        editor.set_selection_color(Color::from_rgb(38, 79, 120));

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
        let (progress_sender, progress_receiver) = app::channel::<QueryProgress>();
        let query_running = Rc::new(RefCell::new(false));

        let intellisense_data = Rc::new(RefCell::new(IntellisenseData::new()));
        let intellisense_popup = Rc::new(RefCell::new(IntellisensePopup::new()));
        let highlighter = Rc::new(RefCell::new(SqlHighlighter::new()));

        let mut widget = Self {
            group,
            editor,
            buffer,
            style_buffer,
            connection,
            execute_callback,
            progress_callback: progress_callback.clone(),
            progress_sender,
            query_running: query_running.clone(),
            intellisense_data,
            intellisense_popup,
            highlighter,
            timeout_input: timeout_input.clone(),
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

        widget
    }

    fn setup_progress_handler(
        &self,
        progress_receiver: app::Receiver<QueryProgress>,
        progress_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryProgress)>>>>,
        query_running: Rc<RefCell<bool>>,
    ) {
        let execute_callback = self.execute_callback.clone();
        app::add_idle3(move |_| {
            while let Some(message) = progress_receiver.recv() {
                if let Some(ref mut cb) = *progress_callback.borrow_mut() {
                    cb(message.clone());
                }

                match message {
                    QueryProgress::StatementFinished { result, .. } => {
                        if let Some(ref mut cb) = *execute_callback.borrow_mut() {
                            cb(result);
                        }
                    }
                    QueryProgress::BatchFinished => {
                        *query_running.borrow_mut() = false;
                        set_cursor(Cursor::Default);
                    }
                    _ => {}
                }
            }
        });
    }

    fn setup_syntax_highlighting(&self) {
        // Initial highlighting (empty)
        self.highlighter
            .borrow()
            .highlight("", &mut self.style_buffer.clone());
    }

    fn setup_intellisense(&mut self) {
        let buffer = self.buffer.clone();
        let mut editor = self.editor.clone();
        let intellisense_data = self.intellisense_data.clone();
        let intellisense_popup = self.intellisense_popup.clone();
        let highlighter = self.highlighter.clone();
        let style_buffer = self.style_buffer.clone();
        let connection_for_describe = self.connection.clone();

        // Setup callback for inserting selected text
        let mut buffer_for_insert = buffer.clone();
        let mut editor_for_insert = editor.clone();
        {
            let mut popup = intellisense_popup.borrow_mut();
            popup.set_selected_callback(move |selected| {
                // Get current word position
                let cursor_pos = editor_for_insert.insert_position() as usize;
                let text = buffer_for_insert.text();
                let (word, start, _end) = get_word_at_cursor(&text, cursor_pos);

                // Replace the word with selected suggestion
                if !word.is_empty() {
                    buffer_for_insert.replace(start as i32, cursor_pos as i32, &selected);
                    editor_for_insert.set_insert_position((start + selected.len()) as i32);
                } else {
                    buffer_for_insert.insert(cursor_pos as i32, &selected);
                    editor_for_insert.set_insert_position((cursor_pos + selected.len()) as i32);
                }
            });
        }

        // Handle keyboard events for triggering intellisense and syntax highlighting
        let mut buffer_for_handle = buffer.clone();
        let intellisense_data_for_handle = intellisense_data.clone();
        let intellisense_popup_for_handle = intellisense_popup.clone();
        let highlighter_for_handle = highlighter.clone();
        let mut style_buffer_for_handle = style_buffer.clone();
        let connection_for_f4 = connection_for_describe.clone();

        editor.handle(move |ed, ev| {
            match ev {
                Event::KeyUp => {
                    // Update syntax highlighting
                    let text = buffer_for_handle.text();
                    highlighter_for_handle
                        .borrow()
                        .highlight(&text, &mut style_buffer_for_handle);

                    let key = fltk::app::event_key();

                    // F4 - Quick Describe (show table structure)
                    if key == Key::F4 {
                        let cursor_pos = ed.insert_position() as usize;
                        let (word, _, _) = get_word_at_cursor(&text, cursor_pos);

                        if !word.is_empty() {
                            let conn_guard = connection_for_f4.lock().unwrap();
                            if conn_guard.is_connected() {
                                if let Some(db_conn) = conn_guard.get_connection() {
                                    Self::show_quick_describe(db_conn.as_ref(), &word);
                                }
                            } else {
                                fltk::dialog::alert_default("Not connected to database");
                            }
                        }
                        return true;
                    }

                    // Check for Ctrl+Space to trigger intellisense
                    if fltk::app::event_state().contains(fltk::enums::Shortcut::Ctrl)
                        && key == Key::from_char(' ')
                    {
                        Self::trigger_intellisense(
                            ed,
                            &buffer_for_handle,
                            &intellisense_data_for_handle,
                            &intellisense_popup_for_handle,
                        );
                        return true;
                    }

                    // Hide intellisense on navigation keys or when word becomes invalid
                    let hide_keys = matches!(
                        key,
                        Key::Left | Key::Right | Key::Home | Key::End | Key::PageUp | Key::PageDown
                    );
                    if hide_keys {
                        intellisense_popup_for_handle.borrow_mut().hide();
                        return false;
                    }

                    // Check current character to decide whether to show/hide intellisense
                    if let Some(ch) = fltk::app::event_text().chars().next() {
                        if ch.is_alphanumeric() || ch == '_' {
                            // Auto-trigger on typing alphanumeric characters
                            let cursor_pos = ed.insert_position() as usize;
                            let (word, _, _) = get_word_at_cursor(&text, cursor_pos);

                            // Only show suggestions if word is at least 2 characters
                            if word.len() >= 2 {
                                Self::trigger_intellisense(
                                    ed,
                                    &buffer_for_handle,
                                    &intellisense_data_for_handle,
                                    &intellisense_popup_for_handle,
                                );
                            } else {
                                // Word too short, hide popup
                                intellisense_popup_for_handle.borrow_mut().hide();
                            }
                        } else {
                            // Non-identifier character typed (space, punctuation, etc.)
                            // Hide the intellisense popup
                            intellisense_popup_for_handle.borrow_mut().hide();
                        }
                    } else if key == Key::BackSpace || key == Key::Delete {
                        // On backspace/delete, check if we still have a valid word
                        let cursor_pos = ed.insert_position() as usize;
                        let (word, _, _) = get_word_at_cursor(&text, cursor_pos);
                        if word.len() >= 2 {
                            Self::trigger_intellisense(
                                ed,
                                &buffer_for_handle,
                                &intellisense_data_for_handle,
                                &intellisense_popup_for_handle,
                            );
                        } else {
                            intellisense_popup_for_handle.borrow_mut().hide();
                        }
                    }

                    false
                }
                Event::KeyDown => {
                    let key = fltk::app::event_key();
                    let mut popup = intellisense_popup_for_handle.borrow_mut();

                    if popup.is_visible() {
                        match key {
                            Key::Escape => {
                                popup.hide();
                                return true;
                            }
                            Key::Up => {
                                popup.select_prev();
                                return true;
                            }
                            Key::Down => {
                                popup.select_next();
                                return true;
                            }
                            Key::Enter | Key::Tab => {
                                if let Some(selected) = popup.get_selected() {
                                    // Insert selected suggestion
                                    let cursor_pos = ed.insert_position() as usize;
                                    let text = buffer_for_handle.text();
                                    let (word, start, _) = get_word_at_cursor(&text, cursor_pos);

                                    if !word.is_empty() {
                                        buffer_for_handle.replace(
                                            start as i32,
                                            cursor_pos as i32,
                                            &selected,
                                        );
                                        ed.set_insert_position((start + selected.len()) as i32);
                                    } else {
                                        buffer_for_handle.insert(cursor_pos as i32, &selected);
                                        ed.set_insert_position(
                                            (cursor_pos + selected.len()) as i32,
                                        );
                                    }
                                }
                                popup.hide();
                                return true;
                            }
                            _ => {}
                        }
                    }

                    false
                }
                Event::Shortcut => {
                    if ed.has_focus() {
                        return true;
                    }
                    false
                }
                Event::Paste => {
                    // Update syntax highlighting after paste
                    let text = buffer_for_handle.text();
                    highlighter_for_handle
                        .borrow()
                        .highlight(&text, &mut style_buffer_for_handle);
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
    ) {
        let cursor_pos = editor.insert_position() as usize;
        let text = buffer.text();
        let (word, _, _) = get_word_at_cursor(&text, cursor_pos);

        if word.is_empty() {
            return;
        }

        let data = intellisense_data.borrow();
        let suggestions = data.get_all_suggestions(&word);

        if suggestions.is_empty() {
            intellisense_popup.borrow_mut().hide();
            return;
        }

        // Position popup relative to the editor cursor within the window
        let (cursor_x, cursor_y) = editor.position_to_xy(editor.insert_position());
        let (editor_x, editor_y) = Self::widget_origin_in_window(editor);
        let popup_x = editor_x + cursor_x;
        let popup_y = editor_y + cursor_y + 20;

        intellisense_popup
            .borrow_mut()
            .show_suggestions(suggestions, popup_x, popup_y);
    }

    fn widget_origin_in_window<W: WidgetExt>(widget: &W) -> (i32, i32) {
        let mut x = widget.x();
        let mut y = widget.y();
        let mut parent = widget.parent();
        while let Some(group) = parent {
            x += group.x();
            y += group.y();
            parent = group.parent();
        }
        (x, y)
    }

    /// Show quick describe dialog for a table (F4 functionality)
    fn show_quick_describe(conn: &oracle::Connection, object_name: &str) {
        use crate::db::ObjectBrowser;
        use fltk::{enums::Color, prelude::*, text::TextDisplay, window::Window};

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
                let mut dialog = Window::default()
                    .with_size(600, 400)
                    .with_label(&format!("Describe: {}", object_name.to_uppercase()));
                dialog.set_color(Color::from_rgb(45, 45, 48));
                dialog.make_modal(true);

                let mut display = TextDisplay::default().with_pos(10, 10).with_size(580, 340);
                display.set_color(Color::from_rgb(30, 30, 30));
                display.set_text_color(Color::from_rgb(220, 220, 220));
                display.set_text_font(fltk::enums::Font::Courier);
                display.set_text_size(12);

                let mut buffer = fltk::text::TextBuffer::default();
                buffer.set_text(&info);
                display.set_buffer(buffer);

                let mut close_btn = fltk::button::Button::default()
                    .with_pos(250, 360)
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

        let conn_guard = self.connection.lock().unwrap();
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
        use fltk::{enums::Color, prelude::*, text::TextDisplay, window::Window};

        let mut dialog = Window::default()
            .with_size(800, 500)
            .with_label("Explain Plan");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut display = TextDisplay::default().with_pos(10, 10).with_size(780, 440);
        display.set_color(Color::from_rgb(30, 30, 30));
        display.set_text_color(Color::from_rgb(220, 220, 220));
        display.set_text_font(fltk::enums::Font::Courier);
        display.set_text_size(12);

        let mut buffer = fltk::text::TextBuffer::default();
        buffer.set_text(plan_text);
        display.set_buffer(buffer);

        let mut close_btn = fltk::button::Button::default()
            .with_pos(690, 455)
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

    pub fn clear(&self) {
        let mut buffer = self.buffer.clone();
        buffer.set_text("");
    }

    pub fn commit(&self) {
        let conn_guard = self.connection.lock().unwrap();
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            if let Err(err) = db_conn.commit() {
                fltk::dialog::alert_default(&format!("Commit failed: {}", err));
            }
        }
    }

    pub fn rollback(&self) {
        let conn_guard = self.connection.lock().unwrap();
        if !conn_guard.is_connected() {
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        if let Some(db_conn) = conn_guard.get_connection() {
            if let Err(err) = db_conn.rollback() {
                fltk::dialog::alert_default(&format!("Rollback failed: {}", err));
            }
        }
    }

    pub fn cancel_current(&self) {
        if !*self.query_running.borrow() {
            fltk::dialog::alert_default("No query is running");
            return;
        }

        let conn_guard = self.connection.lock().unwrap();
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

    pub fn focus(&mut self) {
        self.group.show();
        let _ = self.editor.take_focus();
    }

    pub fn execute_current(&self) {
        let sql = self.buffer.text();
        self.execute_sql(&sql);
    }

    pub fn execute_selected(&self) {
        let buffer = self.buffer.clone();
        if !buffer.selected() {
            fltk::dialog::alert_default("No SQL selected");
            return;
        }

        let sql = buffer.selection_text();
        self.execute_sql(&sql);
    }

    fn execute_sql(&self, sql: &str) {
        if sql.trim().is_empty() {
            return;
        }

        if *self.query_running.borrow() {
            fltk::dialog::alert_default("A query is already running");
            return;
        }

        let conn_guard = self.connection.lock().unwrap();
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
                    return;
                }

                let _ = sender.send(QueryProgress::BatchStart);

                let previous_timeout = conn.call_timeout().unwrap_or(None);
                if let Err(err) = conn.set_call_timeout(query_timeout) {
                    let _ = sender.send(QueryProgress::StatementFinished {
                        index: 0,
                        result: QueryResult::new_error(&sql_text, &err.to_string()),
                    });
                    let _ = sender.send(QueryProgress::BatchFinished);
                    let _ = conn.set_call_timeout(previous_timeout);
                    return;
                }

                for (index, statement) in statements.iter().enumerate() {
                    let trimmed = statement.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let sql_upper = trimmed.to_uppercase();
                    let is_select = sql_upper.starts_with("SELECT") || sql_upper.starts_with("WITH");

                    let _ = sender.send(QueryProgress::StatementStart { index });

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
                            },
                            &mut |row| {
                                buffered_rows.push(row);
                                if last_flush.elapsed() >= Duration::from_secs(1) {
                                    let rows = std::mem::take(&mut buffered_rows);
                                    let _ = sender.send(QueryProgress::Rows { index, rows });
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

                    QueryHistoryDialog::add_to_history(
                        trimmed,
                        result.execution_time.as_millis() as u64,
                        result.row_count,
                        &conn_name,
                    );

                    let _ = sender.send(QueryProgress::StatementFinished { index, result });

                    if timed_out {
                        let _ = conn.set_call_timeout(previous_timeout);
                        let _ = sender.send(QueryProgress::BatchFinished);
                        return;
                    }
                }

                let _ = conn.set_call_timeout(previous_timeout);
                let _ = sender.send(QueryProgress::BatchFinished);
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

        let secs = trimmed.parse::<u64>().ok()?;
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
