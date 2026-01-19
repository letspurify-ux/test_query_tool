use fltk::{
    button::Button,
    enums::{Color, Font, FrameType},
    group::{Flex, FlexType, Pack, PackType},
    prelude::*,
    text::{TextBuffer, TextEditor, WrapMode},
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::{QueryExecutor, QueryResult, SharedConnection};

#[derive(Clone)]
pub struct SqlEditorWidget {
    group: Flex,
    editor: TextEditor,
    buffer: TextBuffer,
    connection: SharedConnection,
    execute_callback: Rc<RefCell<Option<Box<dyn FnMut(QueryResult)>>>>,
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

        let mut widget = Self {
            group,
            editor,
            buffer,
            connection,
            execute_callback,
        };

        widget.setup_button_callbacks(execute_btn, explain_btn, clear_btn, commit_btn, rollback_btn);

        widget
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
