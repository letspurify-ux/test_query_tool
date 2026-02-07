use fltk::{
    browser::HoldBrowser,
    button::Button,
    enums::FrameType,
    group::Flex,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc, OnceLock};
use std::thread;

use crate::ui::center_on_main;
use crate::ui::constants::*;
use crate::ui::theme;
use crate::ui::{configured_editor_profile, configured_ui_font_size};
use crate::utils::config::{QueryHistory, QueryHistoryEntry};

/// Find the largest valid UTF-8 boundary at or before `index`.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn history_writer_sender() -> &'static mpsc::Sender<QueryHistoryEntry> {
    static HISTORY_WRITER: OnceLock<mpsc::Sender<QueryHistoryEntry>> = OnceLock::new();
    HISTORY_WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<QueryHistoryEntry>();
        thread::spawn(move || {
            let mut history = QueryHistory::load();
            while let Ok(entry) = receiver.recv() {
                history.add_entry(entry);
                while let Ok(next) = receiver.try_recv() {
                    history.add_entry(next);
                }
                if let Err(err) = history.save() {
                    eprintln!("Query history save error: {err}");
                }
            }
        });
        sender
    })
}

/// Query history dialog for viewing and re-executing past queries
pub struct QueryHistoryDialog;

impl QueryHistoryDialog {
    pub fn show_with_registry(popups: Rc<RefCell<Vec<Window>>>) -> Option<String> {
        enum DialogMessage {
            UpdatePreview(usize),
            UseSelected,
            ClearHistory,
            Close,
        }

        let history = QueryHistory::load();

        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut dialog = Window::default()
            .with_size(800, 500)
            .with_label("Query History");
        center_on_main(&mut dialog);
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);

        let mut main_flex = Flex::default().with_pos(10, 10).with_size(780, 480);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_spacing(DIALOG_SPACING);

        // Top section with list and preview
        let mut content_flex = Flex::default();
        content_flex.set_type(fltk::group::FlexType::Row);
        content_flex.set_spacing(DIALOG_SPACING);

        // Left - History list
        let mut list_flex = Flex::default();
        list_flex.set_type(fltk::group::FlexType::Column);
        list_flex.set_spacing(DIALOG_SPACING);

        let mut list_label =
            fltk::frame::Frame::default().with_label("Query History (Most Recent First):");
        list_label.set_label_color(theme::text_primary());
        list_flex.fixed(&list_label, LABEL_ROW_HEIGHT);

        let mut browser = HoldBrowser::default();
        browser.set_color(theme::input_bg());
        browser.set_selection_color(theme::selection_strong());

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
        preview_flex.set_spacing(DIALOG_SPACING);

        let mut preview_label = fltk::frame::Frame::default().with_label("SQL Preview:");
        preview_label.set_label_color(theme::text_primary());
        preview_flex.fixed(&preview_label, LABEL_ROW_HEIGHT);

        let preview_buffer = TextBuffer::default();
        let mut preview_display = TextDisplay::default();
        preview_display.set_buffer(preview_buffer.clone());
        preview_display.set_color(theme::editor_bg());
        preview_display.set_text_color(theme::text_primary());
        preview_display.set_text_font(configured_editor_profile().normal);
        preview_display.set_text_size(configured_ui_font_size());

        preview_flex.end();

        content_flex.end();

        // Bottom buttons
        let mut button_flex = Flex::default();
        button_flex.set_type(fltk::group::FlexType::Row);
        button_flex.set_spacing(DIALOG_SPACING);

        let _spacer = fltk::frame::Frame::default();

        let mut use_btn = Button::default()
            .with_size(BUTTON_WIDTH_LARGE, BUTTON_HEIGHT)
            .with_label("Use Query");
        use_btn.set_color(theme::button_primary());
        use_btn.set_label_color(theme::text_primary());
        use_btn.set_frame(FrameType::RFlatBox);

        let mut clear_btn = Button::default()
            .with_size(BUTTON_WIDTH_LARGE, BUTTON_HEIGHT)
            .with_label("Clear History");
        clear_btn.set_color(theme::button_danger());
        clear_btn.set_label_color(theme::text_primary());
        clear_btn.set_frame(FrameType::RFlatBox);

        let mut close_btn = Button::default()
            .with_size(BUTTON_WIDTH, BUTTON_HEIGHT)
            .with_label("Close");
        close_btn.set_color(theme::button_subtle());
        close_btn.set_label_color(theme::text_primary());
        close_btn.set_frame(FrameType::RFlatBox);

        button_flex.fixed(&use_btn, BUTTON_WIDTH_LARGE);
        button_flex.fixed(&clear_btn, BUTTON_WIDTH_LARGE);
        button_flex.fixed(&close_btn, BUTTON_WIDTH);
        button_flex.end();
        main_flex.fixed(&button_flex, BUTTON_ROW_HEIGHT);

        main_flex.end();
        dialog.end();

        popups.borrow_mut().push(dialog.clone());
        // State for selected query
        let selected_sql: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let queries: Rc<RefCell<Vec<QueryHistoryEntry>>> = Rc::new(RefCell::new(history.queries));

        let (sender, receiver) = mpsc::channel::<DialogMessage>();

        // Browser selection callback - update preview
        let sender_for_preview = sender.clone();
        browser.set_callback(move |b| {
            let selected = b.value();
            if selected > 0 {
                if let Some(idx) = (selected - 1).try_into().ok() {
                    let _ = sender_for_preview.send(DialogMessage::UpdatePreview(idx));
                }
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
        while dialog.shown() {
            fltk::app::wait();
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
                            if let Ok(idx) = usize::try_from(selected - 1) {
                                let queries = queries.borrow();
                                if let Some(entry) = queries.get(idx) {
                                    *selected_sql.borrow_mut() = Some(entry.sql.clone());
                                    dialog.hide();
                                }
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

        // Remove dialog from popups to prevent memory leak
        popups
            .borrow_mut()
            .retain(|w| w.as_widget_ptr() != dialog.as_widget_ptr());

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
        let entry = QueryHistoryEntry {
            sql: sql.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            execution_time_ms,
            row_count,
            connection_name: connection_name.to_string(),
        };

        if let Err(err) = history_writer_sender().send(entry) {
            let mut history = QueryHistory::load();
            history.add_entry(err.0);
            let _ = history.save();
        }
    }
}

/// Truncate SQL for display in list
fn truncate_sql(sql: &str, max_len: usize) -> String {
    let mut normalized = String::with_capacity(sql.len());
    for byte in sql.as_bytes() {
        if byte.is_ascii_whitespace() {
            normalized.push(' ');
        } else {
            normalized.push(*byte as char);
        }
    }
    let trimmed = normalized.trim_matches(|c: char| c.is_ascii_whitespace());

    if trimmed.len() > max_len {
        let end = floor_char_boundary(trimmed, max_len);
        format!("{}...", &trimmed[..end])
    } else {
        trimmed.to_string()
    }
}
