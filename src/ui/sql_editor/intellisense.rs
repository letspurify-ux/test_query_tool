use fltk::{
    app,
    draw::set_cursor,
    enums::{Cursor, Event, Key},
    prelude::*,
    text::{TextBuffer, TextEditor},
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use crate::db::{lock_connection, ObjectBrowser, SharedConnection, TableColumnDetail};
use crate::ui::intellisense::{
    detect_sql_context, get_word_at_cursor, IntellisenseData, IntellisensePopup, SqlContext,
};

use super::*;

impl SqlEditorWidget {
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
        let pending_intellisense = self.pending_intellisense.clone();

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
                let cursor_pos = editor_for_insert.insert_position().max(0) as i32;
                let cursor_pos_usize = cursor_pos as usize;
                let context_text = Self::context_before_cursor(&buffer_for_insert, cursor_pos);
                let context = detect_sql_context(&context_text, context_text.len());
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
                    let (word, start, _end) = Self::word_at_cursor(&buffer_for_insert, cursor_pos);
                    if word.is_empty() {
                        (cursor_pos_usize, cursor_pos_usize)
                    } else {
                        (start, cursor_pos_usize)
                    }
                };

                if start != end {
                    buffer_for_insert.replace(start as i32, end as i32, &selected);
                    editor_for_insert.set_insert_position((start + selected.len()) as i32);
                } else {
                    buffer_for_insert.insert(cursor_pos as i32, &selected);
                    editor_for_insert
                        .set_insert_position((cursor_pos_usize + selected.len()) as i32);
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
        let pending_intellisense_for_handle = pending_intellisense.clone();

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
                                let selected =
                                    intellisense_popup_for_handle.borrow().get_selected();
                                if let Some(selected) = selected {
                                    let cursor_pos = ed.insert_position().max(0) as i32;
                                    let cursor_pos_usize = cursor_pos as usize;
                                    let range = *completion_range_for_handle.borrow();
                                    let (start, end) = if let Some((range_start, range_end)) = range
                                    {
                                        (range_start, range_end)
                                    } else {
                                        let (word, start, _end) =
                                            Self::word_at_cursor(&buffer_for_handle, cursor_pos);
                                        if word.is_empty() {
                                            (cursor_pos_usize, cursor_pos_usize)
                                        } else {
                                            (start, cursor_pos_usize)
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
                                        buffer_for_handle.insert(cursor_pos, &selected);
                                        ed.set_insert_position(
                                            (cursor_pos_usize + selected.len()) as i32,
                                        );
                                    }
                                    *completion_range_for_handle.borrow_mut() = None;

                                    // Update syntax highlighting after insertion
                                    let cursor_pos = ed.insert_position().max(0) as usize;
                                    highlighter_for_handle.borrow().highlight_buffer_window(
                                        &buffer_for_handle,
                                        &mut style_buffer_for_handle,
                                        cursor_pos,
                                    );
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
                                    &pending_intellisense_for_handle,
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
                                if let Some(ref mut cb) = *replace_callback_for_handle.borrow_mut()
                                {
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
                            &pending_intellisense_for_handle,
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
                    let key = fltk::app::event_key();
                    let event_text = fltk::app::event_text();
                    let state = fltk::app::event_state();
                    let ctrl_or_cmd = state.contains(fltk::enums::Shortcut::Ctrl)
                        || state.contains(fltk::enums::Shortcut::Command);
                    let alt = state.contains(fltk::enums::Shortcut::Alt);

                    if event_text.is_empty()
                        && !ctrl_or_cmd
                        && !alt
                        && !matches!(
                            key,
                            Key::BackSpace
                                | Key::Delete
                                | Key::Left
                                | Key::Right
                                | Key::Up
                                | Key::Down
                                | Key::Home
                                | Key::End
                                | Key::PageUp
                                | Key::PageDown
                                | Key::Enter
                                | Key::KPEnter
                                | Key::Tab
                                | Key::Escape
                        )
                    {
                        if popup_visible {
                            intellisense_popup_for_handle.borrow_mut().hide();
                            *completion_range_for_handle.borrow_mut() = None;
                        }
                        return false;
                    }

                    if matches!(key, Key::Up | Key::Down) && *suppress_nav_for_handle.borrow() {
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
                    let cursor_pos = ed.insert_position().max(0) as i32;
                    let (word, _, _) = Self::word_at_cursor(&buffer_for_handle, cursor_pos);
                    let context_text = Self::context_before_cursor(&buffer_for_handle, cursor_pos);
                    let context = detect_sql_context(&context_text, context_text.len());

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
                                &pending_intellisense_for_handle,
                            );
                        } else {
                            intellisense_popup_for_handle.borrow_mut().hide();
                            *completion_range_for_handle.borrow_mut() = None;
                        }
                    } else {
                        let typed_byte = event_text.as_bytes().first().copied();

                        if let Some(byte) = typed_byte {
                            if byte == b'.' {
                                Self::trigger_intellisense(
                                    ed,
                                    &buffer_for_handle,
                                    &intellisense_data_for_handle,
                                    &intellisense_popup_for_handle,
                                    &completion_range_for_handle,
                                    &column_sender_for_handle,
                                    &connection_for_handle,
                                    &pending_intellisense_for_handle,
                                );
                            } else if Self::is_identifier_byte(byte) {
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
                                        &pending_intellisense_for_handle,
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
                Event::Paste => false,
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
        pending_intellisense: &Rc<RefCell<Option<PendingIntellisense>>>,
    ) {
        let cursor_pos = editor.insert_position().max(0) as i32;
        let cursor_pos_usize = cursor_pos as usize;
        let (word, start, _) = Self::word_at_cursor(buffer, cursor_pos);
        let qualifier = Self::qualifier_before_word(buffer, start);
        let prefix = if word.is_empty() {
            if qualifier.is_none() {
                *pending_intellisense.borrow_mut() = None;
                *completion_range.borrow_mut() = None;
                return;
            }
            String::new()
        } else {
            word
        };

        let context_text = Self::context_before_cursor(buffer, cursor_pos);
        let context = detect_sql_context(&context_text, context_text.len());
        let statement_text = Self::statement_context(buffer, cursor_pos);
        let table_refs = if statement_text.is_empty() {
            Self::collect_table_references(&context_text)
        } else {
            Self::collect_table_references(&statement_text)
        };
        let column_tables = Self::resolve_column_tables(&table_refs, qualifier.as_deref());
        let include_columns = qualifier.is_some()
            || matches!(context, SqlContext::ColumnName | SqlContext::ColumnOrAll);

        if include_columns {
            for table in &column_tables {
                Self::request_table_columns(table, intellisense_data, column_sender, connection);
            }
        }

        let columns_loading = if qualifier.is_some() {
            let data = intellisense_data.borrow();
            column_tables.iter().any(|table| {
                let key = table.to_uppercase();
                data.columns_loading.contains(&key)
            })
        } else {
            false
        };

        let suggestions = {
            let mut data = intellisense_data.borrow_mut();
            let column_scope = if !column_tables.is_empty() {
                Some(column_tables.as_slice())
            } else {
                None
            };
            if qualifier.is_some() {
                data.get_column_suggestions(&prefix, column_scope)
            } else {
                data.get_suggestions(&prefix, include_columns, column_scope)
            }
        };

        if suggestions.is_empty() {
            if columns_loading {
                *pending_intellisense.borrow_mut() = Some(PendingIntellisense { cursor_pos });
            } else {
                *pending_intellisense.borrow_mut() = None;
            }
            intellisense_popup.borrow_mut().hide();
            *completion_range.borrow_mut() = None;
            return;
        }
        *pending_intellisense.borrow_mut() = None;

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
        let completion_start = if prefix.is_empty() {
            cursor_pos_usize
        } else {
            start
        };
        *completion_range.borrow_mut() = Some((completion_start, cursor_pos_usize));
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

    fn word_at_cursor(buffer: &TextBuffer, cursor_pos: i32) -> (String, usize, usize) {
        let buffer_len = buffer.length().max(0);
        if buffer_len == 0 {
            return (String::new(), 0, 0);
        }
        let cursor_pos = cursor_pos.clamp(0, buffer_len);
        let start = (cursor_pos - INTELLISENSE_WORD_WINDOW).max(0);
        let end = (cursor_pos + INTELLISENSE_WORD_WINDOW).min(buffer_len);
        let start = buffer.line_start(start).max(0);
        let end = buffer.line_end(end).max(start);
        let text = buffer.text_range(start, end).unwrap_or_default();
        let rel_cursor = (cursor_pos - start).max(0) as usize;
        let (word, rel_start, rel_end) = get_word_at_cursor(&text, rel_cursor);
        let abs_start = start as usize + rel_start;
        let abs_end = start as usize + rel_end;
        (word, abs_start, abs_end)
    }

    fn context_before_cursor(buffer: &TextBuffer, cursor_pos: i32) -> String {
        let buffer_len = buffer.length().max(0);
        let cursor_pos = cursor_pos.clamp(0, buffer_len);
        let start = (cursor_pos - INTELLISENSE_CONTEXT_WINDOW).max(0);
        let start = buffer.line_start(start).max(0);
        let text = buffer.text_range(start, cursor_pos).unwrap_or_default();
        if let Some(pos) = text.rfind(';') {
            return text[pos + 1..].to_string();
        }
        text
    }

    fn statement_context(buffer: &TextBuffer, cursor_pos: i32) -> String {
        let buffer_len = buffer.length().max(0);
        if buffer_len == 0 {
            return String::new();
        }
        let cursor_pos = cursor_pos.clamp(0, buffer_len);
        let start = (cursor_pos - INTELLISENSE_STATEMENT_WINDOW).max(0);
        let end = (cursor_pos + INTELLISENSE_STATEMENT_WINDOW).min(buffer_len);
        let start = buffer.line_start(start).max(0);
        let end = buffer.line_end(end).max(start);
        let Some(text) = buffer.text_range(start, end) else {
            return String::new();
        };
        let mut rel_cursor = (cursor_pos - start).max(0) as usize;
        if rel_cursor > text.len() {
            rel_cursor = text.len();
        }
        let bytes = text.as_bytes();
        let stmt_start = bytes[..rel_cursor]
            .iter()
            .rposition(|&b| b == b';')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let stmt_end = bytes[rel_cursor..]
            .iter()
            .position(|&b| b == b';')
            .map(|pos| rel_cursor + pos)
            .unwrap_or(text.len());
        text.get(stmt_start..stmt_end).unwrap_or("").to_string()
    }

    fn qualifier_before_word(buffer: &TextBuffer, word_start: usize) -> Option<String> {
        if word_start == 0 {
            return None;
        }
        let buffer_len = buffer.length().max(0) as usize;
        if word_start > buffer_len {
            return None;
        }
        let start = word_start
            .saturating_sub(INTELLISENSE_QUALIFIER_WINDOW as usize)
            .min(word_start);
        let start = buffer.line_start(start as i32).max(0) as usize;
        let text = buffer
            .text_range(start as i32, word_start as i32)
            .unwrap_or_default();
        let mut rel_word_start = word_start - start;
        if rel_word_start > text.len() {
            rel_word_start = text.len();
        }
        if rel_word_start == 0 {
            return None;
        }
        let bytes = text.as_bytes();
        if bytes.get(rel_word_start.saturating_sub(1)) != Some(&b'.') {
            return None;
        }
        let idx = rel_word_start - 1;
        let mut begin = idx;
        while begin > 0 {
            if let Some(&byte) = bytes.get(begin - 1) {
                if Self::is_identifier_byte(byte) {
                    begin -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if begin == idx {
            return None;
        }
        let Some(qualifier) = text.get(begin..idx) else {
            return None;
        };
        if qualifier.is_empty() {
            None
        } else {
            Some(qualifier.to_string())
        }
    }

    fn is_identifier_byte(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
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
            "ON" | "JOIN"
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
    fn show_quick_describe_dialog(object_name: &str, columns: &[TableColumnDetail]) {
        use fltk::{prelude::*, text::TextDisplay, window::Window};

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
        display.set_text_size(14);

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
        *self.pending_intellisense.borrow_mut() = None;
    }

    pub fn hide_intellisense(&self) {
        let mut popup = self.intellisense_popup.borrow_mut();
        if popup.is_visible() {
            popup.hide();
        }
        *self.completion_range.borrow_mut() = None;
        *self.pending_intellisense.borrow_mut() = None;
    }

    #[allow(dead_code)]
    pub fn update_intellisense_data(&mut self, data: IntellisenseData) {
        let mut data = data;
        data.rebuild_indices();
        *self.intellisense_data.borrow_mut() = data;
    }

    pub fn get_intellisense_data(&self) -> Rc<RefCell<IntellisenseData>> {
        self.intellisense_data.clone()
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
            &self.pending_intellisense,
        );
    }

    pub fn quick_describe_at_cursor(&self) {
        let cursor_pos = self.editor.insert_position().max(0) as i32;
        let (word, _, _) = Self::word_at_cursor(&self.buffer, cursor_pos);
        if word.is_empty() {
            return;
        }

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
                ObjectBrowser::get_table_structure(db_conn.as_ref(), &word)
                    .map_err(|err| err.to_string())
            } else {
                Err("Not connected to database".to_string())
            };

            let _ = sender.send(UiActionResult::QuickDescribe {
                object_name: word,
                result,
            });
            app::awake();
        });
    }
}
