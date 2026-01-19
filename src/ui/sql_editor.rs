use fltk::{
    button::Button,
    enums::{Color, Event, Font, FrameType, Key},
    group::{Flex, FlexType, Pack, PackType},
    prelude::*,
    text::{TextBuffer, TextEditor, WrapMode},
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::{QueryExecutor, QueryResult, SharedConnection};
use crate::ui::intellisense::{get_word_at_cursor, IntellisenseData, IntellisensePopup};

#[derive(Clone)]
pub struct SqlEditorWidget {
    group: Flex,
    editor: TextEditor,
    buffer: TextBuffer,
    connection: SharedConnection,
    execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>>,
    intellisense_data: Rc<RefCell<IntellisenseData>>,
    intellisense_popup: Rc<RefCell<IntellisensePopup>>,
}

impl SqlEditorWidget {
    pub fn new(connection: SharedConnection) -> Self {
        let mut group = Flex::default();
        group.set_type(FlexType::Column);
        group.set_margin(5);
        group.set_color(Color::from_rgb(30, 30, 30));

        // Button toolbar
        let mut button_pack = Pack::default();
        button_pack.set_type(PackType::Horizontal);
        button_pack.set_spacing(5);

        let mut execute_btn = Button::default()
            .with_size(80, 25)
            .with_label("Execute");
        execute_btn.set_color(Color::from_rgb(0, 122, 204));
        execute_btn.set_label_color(Color::White);
        execute_btn.set_frame(FrameType::FlatBox);

        let mut explain_btn = Button::default()
            .with_size(80, 25)
            .with_label("Explain");
        explain_btn.set_color(Color::from_rgb(104, 33, 122));
        explain_btn.set_label_color(Color::White);
        explain_btn.set_frame(FrameType::FlatBox);

        let mut clear_btn = Button::default()
            .with_size(80, 25)
            .with_label("Clear");
        clear_btn.set_color(Color::from_rgb(100, 100, 100));
        clear_btn.set_label_color(Color::White);
        clear_btn.set_frame(FrameType::FlatBox);

        let mut commit_btn = Button::default()
            .with_size(80, 25)
            .with_label("Commit");
        commit_btn.set_color(Color::from_rgb(0, 150, 0));
        commit_btn.set_label_color(Color::White);
        commit_btn.set_frame(FrameType::FlatBox);

        let mut rollback_btn = Button::default()
            .with_size(80, 25)
            .with_label("Rollback");
        rollback_btn.set_color(Color::from_rgb(200, 50, 50));
        rollback_btn.set_label_color(Color::White);
        rollback_btn.set_frame(FrameType::FlatBox);

        button_pack.end();
        group.fixed(&button_pack, 30);

        // SQL Editor
        let buffer = TextBuffer::default();
        let mut editor = TextEditor::default();
        editor.set_buffer(buffer.clone());
        editor.set_color(Color::from_rgb(30, 30, 30));
        editor.set_text_color(Color::from_rgb(220, 220, 220));
        editor.set_text_font(Font::Courier);
        editor.set_text_size(14);
        editor.set_cursor_color(Color::White);
        editor.wrap_mode(WrapMode::AtBounds, 0);

        // Set selection color
        editor.set_selection_color(Color::from_rgb(38, 79, 120));

        group.end();

        let execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>> =
            Rc::new(RefCell::new(None));

        let intellisense_data = Rc::new(RefCell::new(IntellisenseData::new()));
        let intellisense_popup = Rc::new(RefCell::new(IntellisensePopup::new()));

        let mut widget = Self {
            group,
            editor,
            buffer,
            connection,
            execute_callback,
            intellisense_data,
            intellisense_popup,
        };

        widget.setup_button_callbacks(execute_btn, explain_btn, clear_btn, commit_btn, rollback_btn);
        widget.setup_intellisense();

        widget
    }

    fn setup_intellisense(&mut self) {
        let buffer = self.buffer.clone();
        let mut editor = self.editor.clone();
        let intellisense_data = self.intellisense_data.clone();
        let intellisense_popup = self.intellisense_popup.clone();

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

        // Handle keyboard events for triggering intellisense
        let mut buffer_for_handle = buffer.clone();
        let intellisense_data_for_handle = intellisense_data.clone();
        let intellisense_popup_for_handle = intellisense_popup.clone();

        editor.handle(move |ed, ev| {
            match ev {
                Event::KeyUp => {
                    let key = fltk::app::event_key();

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

                    // Auto-trigger on typing alphanumeric characters
                    if let Some(ch) = fltk::app::event_text().chars().next() {
                        if ch.is_alphanumeric() || ch == '_' {
                            let cursor_pos = ed.insert_position() as usize;
                            let text = buffer_for_handle.text();
                            let (word, _, _) = get_word_at_cursor(&text, cursor_pos);

                            // Only show suggestions if word is at least 2 characters
                            if word.len() >= 2 {
                                Self::trigger_intellisense(
                                    ed,
                                    &buffer_for_handle,
                                    &intellisense_data_for_handle,
                                    &intellisense_popup_for_handle,
                                );
                            }
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

        // Calculate popup position based on cursor
        let (x, y) = editor.position_to_xy(editor.insert_position());

        // Get editor's absolute position
        let editor_x = editor.x();
        let editor_y = editor.y();

        let popup_x = editor_x + x;
        let popup_y = editor_y + y + 20; // 20 pixels below cursor

        intellisense_popup
            .borrow_mut()
            .show_suggestions(suggestions, popup_x, popup_y);
    }

    fn setup_button_callbacks(
        &mut self,
        mut execute_btn: Button,
        mut explain_btn: Button,
        mut clear_btn: Button,
        mut commit_btn: Button,
        mut rollback_btn: Button,
    ) {
        // Execute button callback
        let connection = self.connection.clone();
        let buffer = self.buffer.clone();
        let callback = self.execute_callback.clone();

        execute_btn.set_callback(move |_| {
            let sql = buffer.text();
            if sql.trim().is_empty() {
                return;
            }

            let conn_guard = connection.lock().unwrap();
            if !conn_guard.is_connected() {
                fltk::dialog::alert_default("Not connected to database");
                return;
            }

            if let Some(db_conn) = conn_guard.get_connection() {
                let result = match QueryExecutor::execute(db_conn, &sql) {
                    Ok(result) => result,
                    Err(e) => QueryResult::new_error(&e.to_string()),
                };

                if let Some(ref mut cb) = *callback.borrow_mut() {
                    cb(result);
                }
            }
        });

        // Explain button callback
        let connection = self.connection.clone();
        let buffer = self.buffer.clone();

        explain_btn.set_callback(move |_| {
            let sql = buffer.text();
            if sql.trim().is_empty() {
                return;
            }

            let conn_guard = connection.lock().unwrap();
            if !conn_guard.is_connected() {
                fltk::dialog::alert_default("Not connected to database");
                return;
            }

            if let Some(db_conn) = conn_guard.get_connection() {
                match QueryExecutor::get_explain_plan(db_conn, &sql) {
                    Ok(plan) => {
                        let plan_text = plan.join("\n");
                        fltk::dialog::message_default(&plan_text);
                    }
                    Err(e) => {
                        fltk::dialog::alert_default(&format!("Explain failed: {}", e));
                    }
                }
            }
        });

        // Clear button callback
        let mut buffer = self.buffer.clone();
        clear_btn.set_callback(move |_| {
            buffer.set_text("");
        });

        // Commit button callback
        let connection = self.connection.clone();
        commit_btn.set_callback(move |_| {
            let conn_guard = connection.lock().unwrap();
            if !conn_guard.is_connected() {
                fltk::dialog::alert_default("Not connected to database");
                return;
            }

            if let Some(db_conn) = conn_guard.get_connection() {
                match db_conn.commit() {
                    Ok(_) => {
                        fltk::dialog::message_default("Transaction committed successfully");
                    }
                    Err(e) => {
                        fltk::dialog::alert_default(&format!("Commit failed: {}", e));
                    }
                }
            }
        });

        // Rollback button callback
        let connection = self.connection.clone();
        rollback_btn.set_callback(move |_| {
            let conn_guard = connection.lock().unwrap();
            if !conn_guard.is_connected() {
                fltk::dialog::alert_default("Not connected to database");
                return;
            }

            if let Some(db_conn) = conn_guard.get_connection() {
                match db_conn.rollback() {
                    Ok(_) => {
                        fltk::dialog::message_default("Transaction rolled back successfully");
                    }
                    Err(e) => {
                        fltk::dialog::alert_default(&format!("Rollback failed: {}", e));
                    }
                }
            }
        });
    }

    pub fn set_execute_callback<F>(&mut self, callback: F)
    where
        F: FnMut(QueryResult) + 'static,
    {
        *self.execute_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn update_intellisense_data(&mut self, data: IntellisenseData) {
        *self.intellisense_data.borrow_mut() = data;
    }

    pub fn get_intellisense_data(&self) -> Rc<RefCell<IntellisenseData>> {
        self.intellisense_data.clone()
    }

    pub fn get_text(&self) -> String {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer.set_text(text);
    }

    pub fn get_group(&self) -> &Flex {
        &self.group
    }

    pub fn append_text(&mut self, text: &str) {
        let current = self.buffer.text();
        if current.is_empty() {
            self.buffer.set_text(text);
        } else {
            self.buffer.set_text(&format!("{}\n{}", current, text));
        }
    }
}
