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

use crate::utils::config::{QueryHistory, QueryHistoryEntry};

/// Query history dialog for viewing and re-executing past queries
pub struct QueryHistoryDialog;

impl QueryHistoryDialog {
    /// Show the query history dialog and return selected SQL if any
    pub fn show() -> Option<String> {
        let history = QueryHistory::load();

        let mut dialog = Window::default()
            .with_size(800, 500)
            .with_label("Query History");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut main_flex = Flex::default()
            .with_pos(10, 10)
            .with_size(780, 480);
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

        let mut list_label = fltk::frame::Frame::default()
            .with_label("Query History (Most Recent First):");
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

        let mut preview_label = fltk::frame::Frame::default()
            .with_label("SQL Preview:");
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

        let mut use_btn = Button::default()
            .with_size(120, 30)
            .with_label("Use Query");
        use_btn.set_color(Color::from_rgb(0, 122, 204));
        use_btn.set_label_color(Color::White);
        use_btn.set_frame(FrameType::FlatBox);

        let mut clear_btn = Button::default()
            .with_size(120, 30)
            .with_label("Clear History");
        clear_btn.set_color(Color::from_rgb(200, 50, 50));
        clear_btn.set_label_color(Color::White);
        clear_btn.set_frame(FrameType::FlatBox);

        let mut close_btn = Button::default()
            .with_size(80, 30)
            .with_label("Close");
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

        // Browser selection callback - update preview
        let queries_for_browser = queries.clone();
        let mut preview_buffer_clone = preview_buffer.clone();
        browser.set_callback(move |b| {
            let selected = b.value();
            if selected > 0 {
                let idx = (selected - 1) as usize;
                let queries = queries_for_browser.borrow();
                if let Some(entry) = queries.get(idx) {
                    preview_buffer_clone.set_text(&entry.sql);
                }
            }
        });

        // Use Query button
        let queries_for_use = queries.clone();
        let selected_sql_for_use = selected_sql.clone();
        let browser_for_use = browser.clone();
        let mut dialog_for_use = dialog.clone();
        use_btn.set_callback(move |_| {
            let selected = browser_for_use.value();
            if selected > 0 {
                let idx = (selected - 1) as usize;
                let queries = queries_for_use.borrow();
                if let Some(entry) = queries.get(idx) {
                    *selected_sql_for_use.borrow_mut() = Some(entry.sql.clone());
                    dialog_for_use.hide();
                }
            } else {
                fltk::dialog::alert_default("Please select a query from the list");
            }
        });

        // Clear History button
        let mut browser_for_clear = browser.clone();
        let queries_for_clear = queries.clone();
        let mut preview_buffer_for_clear = preview_buffer.clone();
        clear_btn.set_callback(move |_| {
            let choice = fltk::dialog::choice2_default(
                "Are you sure you want to clear all query history?",
                "Cancel",
                "Clear All",
                ""
            );
            if choice == Some(1) {
                // Clear history
                let new_history = QueryHistory::new();
                let _ = new_history.save();
                queries_for_clear.borrow_mut().clear();
                browser_for_clear.clear();
                preview_buffer_for_clear.set_text("");
            }
        });

        // Close button
        let mut dialog_for_close = dialog.clone();
        close_btn.set_callback(move |_| {
            dialog_for_close.hide();
        });

        dialog.show();

        while dialog.shown() {
            fltk::app::wait();
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
