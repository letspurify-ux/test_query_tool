use fltk::{
    browser::HoldBrowser,
    button::Button,
    enums::{Color, FrameType},
    group::Flex,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use crate::utils::config::{QueryHistory, QueryHistoryEntry};

/// Query history dialog for viewing and re-executing past queries
pub struct QueryHistoryDialog;

impl QueryHistoryDialog {
    /// Show the query history dialog and return selected SQL if any
    pub fn show() -> Option<String> {
        enum DialogMessage {
            UpdatePreview(usize),
            UseSelected,
            ClearHistory,
            Close,
        }

        let history = QueryHistory::load();

        let mut dialog = Window::default()
            .with_size(800, 500)
            .with_label("Query History");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut main_flex = Flex::default().with_pos(10, 10).with_size(780, 480);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_spacing(10);

        // Top section with list and preview
        let mut content_flex = Flex::default();
        content_flex.set_type(fltk::group::FlexType::Row);
        content_flex.set_spacing(10);

        // Left - History list
        let mut list_flex = Flex::default();
        list_flex.set_type(fltk::group::FlexType::Column);
        list_flex.set_spacing(5);

        let mut list_label =
            fltk::frame::Frame::default().with_label("Query History (Most Recent First):");
        list_label.set_label_color(Color::White);
        list_flex.fixed(&list_label, 20);

        let mut browser = HoldBrowser::default();
        browser.set_color(Color::from_rgb(30, 30, 30));
        browser.set_selection_color(Color::from_rgb(0, 122, 204));

        // Populate browser with history entries
        for entry in history.queries.iter() {
            let display = format!(
                "{} | {} | {}ms | {} rows",
                entry.timestamp,
                truncate_sql(&entry.sql, 50),
                entry.execution_time_ms,
                entry.row_count
            );
            browser.add(&display);
        }

        list_flex.end();
        content_flex.fixed(&list_flex, 350);

        // Right - SQL preview
        let mut preview_flex = Flex::default();
        preview_flex.set_type(fltk::group::FlexType::Column);
        preview_flex.set_spacing(5);

        let mut preview_label = fltk::frame::Frame::default().with_label("SQL Preview:");
        preview_label.set_label_color(Color::White);
        preview_flex.fixed(&preview_label, 20);

        let preview_buffer = TextBuffer::default();
        let mut preview_display = TextDisplay::default();
        preview_display.set_buffer(preview_buffer.clone());
        preview_display.set_color(Color::from_rgb(30, 30, 30));
        preview_display.set_text_color(Color::from_rgb(220, 220, 220));
        preview_display.set_text_font(fltk::enums::Font::Courier);
        preview_display.set_text_size(12);

        preview_flex.end();

        content_flex.end();

        // Bottom buttons
        let mut button_flex = Flex::default();
        button_flex.set_type(fltk::group::FlexType::Row);
        button_flex.set_spacing(10);

        let _spacer = fltk::frame::Frame::default();

        let mut use_btn = Button::default().with_size(120, 30).with_label("Use Query");
        use_btn.set_color(Color::from_rgb(0, 122, 204));
        use_btn.set_label_color(Color::White);
        use_btn.set_frame(FrameType::FlatBox);

        let mut clear_btn = Button::default()
            .with_size(120, 30)
            .with_label("Clear History");
        clear_btn.set_color(Color::from_rgb(200, 50, 50));
        clear_btn.set_label_color(Color::White);
        clear_btn.set_frame(FrameType::FlatBox);

        let mut close_btn = Button::default().with_size(80, 30).with_label("Close");
        close_btn.set_color(Color::from_rgb(100, 100, 100));
        close_btn.set_label_color(Color::White);
        close_btn.set_frame(FrameType::FlatBox);

        button_flex.fixed(&use_btn, 120);
        button_flex.fixed(&clear_btn, 120);
        button_flex.fixed(&close_btn, 80);
        button_flex.end();
        main_flex.fixed(&button_flex, 35);

        main_flex.end();
        dialog.end();

        // State for selected query
        let selected_sql: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let queries: Rc<RefCell<Vec<QueryHistoryEntry>>> = Rc::new(RefCell::new(history.queries));

        let (sender, receiver) = mpsc::channel::<DialogMessage>();

        // Browser selection callback - update preview
        let sender_for_preview = sender.clone();
        browser.set_callback(move |b| {
            let selected = b.value();
            if selected > 0 {
                let _ =
                    sender_for_preview.send(DialogMessage::UpdatePreview((selected - 1) as usize));
            }
        });

        // Use Query button
        let sender_for_use = sender.clone();
        use_btn.set_callback(move |_| {
            let _ = sender_for_use.send(DialogMessage::UseSelected);
        });

        // Clear History button
        let sender_for_clear = sender.clone();
        clear_btn.set_callback(move |_| {
            let _ = sender_for_clear.send(DialogMessage::ClearHistory);
        });

        // Close button
        let sender_for_close = sender.clone();
        close_btn.set_callback(move |_| {
            let _ = sender_for_close.send(DialogMessage::Close);
        });

        dialog.show();

        let mut preview_buffer = preview_buffer.clone();
        let mut browser = browser.clone();
        while dialog.shown() && fltk::app::wait() {
            while let Ok(message) = receiver.try_recv() {
                match message {
                    DialogMessage::UpdatePreview(index) => {
                        let queries = queries.borrow();
                        if let Some(entry) = queries.get(index) {
                            preview_buffer.set_text(&entry.sql);
                        }
                    }
                    DialogMessage::UseSelected => {
                        let selected = browser.value();
                        if selected > 0 {
                            let idx = (selected - 1) as usize;
                            let queries = queries.borrow();
                            if let Some(entry) = queries.get(idx) {
                                *selected_sql.borrow_mut() = Some(entry.sql.clone());
                                dialog.hide();
                            }
                        } else {
                            fltk::dialog::alert_default("Please select a query from the list");
                        }
                    }
                    DialogMessage::ClearHistory => {
                        let choice = fltk::dialog::choice2_default(
                            "Are you sure you want to clear all query history?",
                            "Cancel",
                            "Clear All",
                            "",
                        );
                        if choice == Some(1) {
                            let new_history = QueryHistory::new();
                            let _ = new_history.save();
                            queries.borrow_mut().clear();
                            browser.clear();
                            preview_buffer.set_text("");
                        }
                    }
                    DialogMessage::Close => {
                        dialog.hide();
                    }
                }
            }
        }

        let result = selected_sql.borrow().clone();
        result
    }

    /// Add a query to history
    pub fn add_to_history(
        sql: &str,
        execution_time_ms: u64,
        row_count: usize,
        connection_name: &str,
    ) {
        let mut history = QueryHistory::load();

        let entry = QueryHistoryEntry {
            sql: sql.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            execution_time_ms,
            row_count,
            connection_name: connection_name.to_string(),
        };

        history.add_entry(entry);
        let _ = history.save();
    }
}

/// Truncate SQL for display in list
fn truncate_sql(sql: &str, max_len: usize) -> String {
    let normalized: String = sql
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let trimmed = normalized.trim();

    if trimmed.len() > max_len {
        format!("{}...", &trimmed[..max_len])
    } else {
        trimmed.to_string()
    }
}
