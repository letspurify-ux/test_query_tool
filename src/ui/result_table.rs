use fltk::{
    app,
    enums::{Color, Event, FrameType, Key, Shortcut},
    menu::MenuButton,
    prelude::*,
};
use fltk_table::{SmartTable, TableOpts};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::db::QueryResult;

/// Minimum interval between UI updates during streaming
const UI_UPDATE_INTERVAL: Duration = Duration::from_millis(500);
/// Maximum rows to buffer before forcing a UI update
const MAX_BUFFERED_ROWS: usize = 1000;

#[derive(Clone)]
pub struct ResultTableWidget {
    table: SmartTable,
    headers: Rc<RefCell<Vec<String>>>,
    /// Buffer for pending rows during streaming
    pending_rows: Rc<RefCell<Vec<Vec<String>>>>,
    /// Pending column width updates
    pending_widths: Rc<RefCell<Vec<i32>>>,
    /// Last UI update time
    last_flush: Rc<RefCell<Instant>>,
}

#[derive(Default)]
struct DragState {
    is_dragging: bool,
    start_row: i32,
    start_col: i32,
}

impl ResultTableWidget {
    pub fn new() -> Self {
        Self::with_size(0, 0, 100, 100)
    }

    pub fn with_size(x: i32, y: i32, w: i32, h: i32) -> Self {
        let headers = Rc::new(RefCell::new(Vec::new()));
        // Create SmartTable with dark theme styling
        let mut table = SmartTable::new(x, y, w, h, None).with_opts(Self::table_opts(0, 0));

        // Apply dark theme colors
        table.set_color(Color::from_rgb(30, 30, 30));
        table.set_row_header(true);
        table.set_row_header_width(55);
        table.set_col_header(true);
        table.set_col_header_height(28);
        table.set_row_height_all(26);

        // Setup event handler for mouse selection and keyboard shortcuts
        let headers_for_handle = headers.clone();
        let drag_state_for_handle = Rc::new(RefCell::new(DragState::default()));

        let mut table_for_handle = table.clone();
        table.handle(move |_, ev| {
            match ev {
                Event::Push => {
                    if app::event_mouse_button() == app::MouseButton::Right {
                        Self::show_context_menu(&table_for_handle, &headers_for_handle);
                        return true;
                    }
                    // Left click - start drag selection
                    if app::event_mouse_button() == app::MouseButton::Left {
                        let _ = table_for_handle.take_focus();
                        if let Some((row, col)) = Self::get_cell_at_mouse(&table_for_handle) {
                            let mut state = drag_state_for_handle.borrow_mut();
                            state.is_dragging = true;
                            state.start_row = row;
                            state.start_col = col;
                            table_for_handle.set_selection(row, col, row, col);
                            table_for_handle.redraw();
                            return true;
                        }
                    }
                    false
                }
                Event::Drag => {
                    let is_dragging = drag_state_for_handle.borrow().is_dragging;
                    if is_dragging {
                        if let Some((row, col)) = Self::get_cell_at_mouse_for_drag(&table_for_handle)
                        {
                            let state = drag_state_for_handle.borrow();
                            let r1 = state.start_row.min(row);
                            let r2 = state.start_row.max(row);
                            let c1 = state.start_col.min(col);
                            let c2 = state.start_col.max(col);
                            drop(state);
                            table_for_handle.set_selection(r1, c1, r2, c2);
                            table_for_handle.redraw();
                        }
                        return true;
                    }
                    false
                }
                Event::Released => {
                    let mut state = drag_state_for_handle.borrow_mut();
                    if state.is_dragging {
                        state.is_dragging = false;
                        return true;
                    }
                    false
                }
                Event::KeyDown => {
                    let key = app::event_key();
                    let ctrl = app::event_state().contains(Shortcut::Ctrl)
                        || app::event_state().contains(Shortcut::Command);

                    if ctrl {
                        match key {
                            k if k == Key::from_char('a') => {
                                // Ctrl+A - Select all
                                let rows = table_for_handle.rows();
                                let cols = table_for_handle.cols();
                                if rows > 0 && cols > 0 {
                                    table_for_handle.set_selection(0, 0, rows - 1, cols - 1);
                                    table_for_handle.redraw();
                                }
                                return true;
                            }
                            k if k == Key::from_char('c') => {
                                // Ctrl+C - Copy selected cells
                                Self::copy_selected_to_clipboard(
                                    &table_for_handle,
                                    &headers_for_handle,
                                );
                                return true;
                            }
                            k if k == Key::from_char('h') => {
                                // Ctrl+H - Copy with headers
                                Self::copy_selected_with_headers(
                                    &table_for_handle,
                                    &headers_for_handle,
                                );
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

        Self {
            table,
            headers,
            pending_rows: Rc::new(RefCell::new(Vec::new())),
            pending_widths: Rc::new(RefCell::new(Vec::new())),
            last_flush: Rc::new(RefCell::new(Instant::now())),
        }
    }

    fn table_opts(rows: i32, cols: i32) -> TableOpts {
        TableOpts {
            rows,
            cols,
            editable: false,
            cell_color: Color::from_rgb(37, 37, 38),
            cell_font_color: Color::from_rgb(220, 220, 220),
            cell_selection_color: Color::from_rgb(38, 79, 120),
            header_frame: FrameType::FlatBox,
            header_color: Color::from_rgb(45, 45, 48),
            header_font_color: Color::from_rgb(240, 240, 240),
            cell_border_color: Color::from_rgb(50, 50, 52),
            ..Default::default()
        }
    }

    /// Get cell at mouse position (returns None if outside cells)
    fn get_cell_at_mouse(table: &SmartTable) -> Option<(i32, i32)> {
        let rows = table.rows();
        let cols = table.cols();
        if rows <= 0 || cols <= 0 {
            return None;
        }

        let mouse_x = app::event_x();
        let mouse_y = app::event_y();

        let table_x = table.x();
        let table_y = table.y();
        let table_w = table.w();
        let table_h = table.h();
        let data_left = table_x + table.row_header_width();
        let data_top = table_y + table.col_header_height();
        let data_right = table_x + table_w;
        let data_bottom = table_y + table_h;

        if mouse_x < data_left || mouse_y < data_top || mouse_x >= data_right || mouse_y >= data_bottom
        {
            return None;
        }

        let start_row = table.row_position().max(0).min(rows - 1);
        let start_col = table.col_position().max(0).min(cols - 1);

        let mut row_hit = None;
        let mut row = start_row;
        while row < rows {
            if let Some((_, cy, _, ch)) =
                table.find_cell(fltk::table::TableContext::Cell, row, start_col)
            {
                if mouse_y >= cy && mouse_y < cy + ch {
                    row_hit = Some(row);
                    break;
                }
                if cy > mouse_y || cy >= data_bottom {
                    break;
                }
            } else {
                break;
            }
            row += 1;
        }

        let row_hit = match row_hit {
            Some(row_hit) => row_hit,
            None => return None,
        };

        let mut col = start_col;
        while col < cols {
            if let Some((cx, _, cw, _)) =
                table.find_cell(fltk::table::TableContext::Cell, row_hit, col)
            {
                if mouse_x >= cx && mouse_x < cx + cw {
                    return Some((row_hit, col));
                }
                if cx > mouse_x || cx >= data_right {
                    break;
                }
            } else {
                break;
            }
            col += 1;
        }

        None
    }

    /// Get cell at mouse position for drag (clamps to boundaries)
    fn get_cell_at_mouse_for_drag(table: &SmartTable) -> Option<(i32, i32)> {
        let rows = table.rows();
        let cols = table.cols();

        if rows <= 0 || cols <= 0 {
            return None;
        }

        let mouse_x = app::event_x();
        let mouse_y = app::event_y();

        // Try direct lookup first
        if let Some((row, col)) = Self::get_cell_at_mouse(table) {
            return Some((row, col));
        }

        // Calculate boundaries for clamping
        let table_x = table.x();
        let table_y = table.y();
        let table_w = table.w();
        let table_h = table.h();
        let row_header_w = table.row_header_width();
        let col_header_h = table.col_header_height();

        let data_left = table_x + row_header_w;
        let data_top = table_y + col_header_h;
        let data_right = table_x + table_w;
        let data_bottom = table_y + table_h;

        // Clamp row
        let row = if mouse_y < data_top {
            0
        } else if mouse_y >= data_bottom {
            rows - 1
        } else {
            // Find row by iterating
            (0..rows)
                .find(|&r| {
                    if let Some((_, cy, _, ch)) =
                        table.find_cell(fltk::table::TableContext::Cell, r, 0)
                    {
                        mouse_y >= cy && mouse_y < cy + ch
                    } else {
                        false
                    }
                })
                .unwrap_or(rows - 1)
        };

        // Clamp col
        let col = if mouse_x < data_left {
            0
        } else if mouse_x >= data_right {
            cols - 1
        } else {
            (0..cols)
                .find(|&c| {
                    if let Some((cx, _, cw, _)) =
                        table.find_cell(fltk::table::TableContext::Cell, 0, c)
                    {
                        mouse_x >= cx && mouse_x < cx + cw
                    } else {
                        false
                    }
                })
                .unwrap_or(cols - 1)
        };

        Some((row, col))
    }

    fn show_context_menu(table: &SmartTable, headers: &Rc<RefCell<Vec<String>>>) {
        let mouse_x = app::event_x();
        let mouse_y = app::event_y();

        // Prevent menu from being added to parent container
        let current_group = fltk::group::Group::try_current();
        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut menu = MenuButton::new(mouse_x, mouse_y, 0, 0, None);
        menu.set_color(Color::from_rgb(45, 45, 48));
        menu.set_text_color(Color::White);
        menu.add_choice("Copy|Copy with Headers|Copy Cell|Copy All");

        if let Some(ref group) = current_group {
            fltk::group::Group::set_current(Some(group));
        }

        if let Some(choice) = menu.popup() {
            let choice_label = choice.label().unwrap_or_default();
            match choice_label.as_str() {
                "Copy" => Self::copy_selected_to_clipboard(table, headers),
                "Copy with Headers" => Self::copy_selected_with_headers(table, headers),
                "Copy Cell" => Self::copy_current_cell(table),
                "Copy All" => Self::copy_all_to_clipboard(table, headers),
                _ => {}
            }
        }

        menu.hide();
    }

    fn copy_selected_to_clipboard(table: &SmartTable, _headers: &Rc<RefCell<Vec<String>>>) {
        let (row_top, row_bot, col_left, col_right) = table.get_selection();
        if row_top < 0 || col_left < 0 {
            return;
        }

        let mut result = String::new();
        for row in row_top..=row_bot {
            let mut row_str = String::new();
            for col in col_left..=col_right {
                if !row_str.is_empty() {
                    row_str.push('\t');
                }
                row_str.push_str(&table.cell_value(row, col));
            }
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&row_str);
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    fn copy_selected_with_headers(table: &SmartTable, headers: &Rc<RefCell<Vec<String>>>) {
        let (row_top, row_bot, col_left, col_right) = table.get_selection();
        if row_top < 0 || col_left < 0 {
            return;
        }

        let headers = headers.borrow();
        let mut result = String::new();

        // Add headers
        let mut header_str = String::new();
        for col in col_left..=col_right {
            if !header_str.is_empty() {
                header_str.push('\t');
            }
            if let Some(h) = headers.get(col as usize) {
                header_str.push_str(h);
            }
        }
        result.push_str(&header_str);
        result.push('\n');

        // Add data
        for row in row_top..=row_bot {
            let mut row_str = String::new();
            for col in col_left..=col_right {
                if !row_str.is_empty() {
                    row_str.push('\t');
                }
                row_str.push_str(&table.cell_value(row, col));
            }
            result.push_str(&row_str);
            result.push('\n');
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    fn copy_current_cell(table: &SmartTable) {
        let (row_top, _, col_left, _) = table.get_selection();
        if row_top >= 0 && col_left >= 0 {
            app::copy(&table.cell_value(row_top, col_left));
        }
    }

    fn copy_all_to_clipboard(table: &SmartTable, headers: &Rc<RefCell<Vec<String>>>) {
        let headers = headers.borrow();
        let mut result = String::new();

        // Add headers
        result.push_str(&headers.join("\t"));
        result.push('\n');

        // Add all data
        for row in 0..table.rows() {
            let mut row_str = String::new();
            for col in 0..table.cols() {
                if !row_str.is_empty() {
                    row_str.push('\t');
                }
                row_str.push_str(&table.cell_value(row, col));
            }
            result.push_str(&row_str);
            result.push('\n');
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    pub fn display_result(&mut self, result: &QueryResult) {
        if !result.is_select {
            self.table.set_opts(Self::table_opts(1, 1));
            self.table.set_col_header_value(0, "Result");
            self.table.set_col_width(0, (result.message.len() * 8).max(200) as i32);
            self.table.set_cell_value(0, 0, &result.message);
            *self.headers.borrow_mut() = vec!["Result".to_string()];
            self.table.redraw();
            return;
        }

        if result.rows.is_empty() && result.row_count > 0 && self.table.rows() > 0 {
            let col_names: Vec<String> =
                result.columns.iter().map(|c| c.name.clone()).collect();
            for (i, name) in col_names.iter().enumerate() {
                self.table.set_col_header_value(i as i32, name);
            }
            *self.headers.borrow_mut() = col_names;
            self.table.redraw();
            return;
        }

        let col_names: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
        let row_count = result.rows.len() as i32;
        let col_count = col_names.len() as i32;

        // Update table dimensions
        self.table.set_opts(Self::table_opts(row_count, col_count));

        // Set column headers
        for (i, name) in col_names.iter().enumerate() {
            self.table.set_col_header_value(i as i32, name);
        }

        // Calculate and set column widths
        let mut widths: Vec<i32> = col_names
            .iter()
            .map(|h| (h.len() * 10).max(80) as i32)
            .collect();

        for row in &result.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    let cell_width = (cell.len() * 8).max(80).min(300) as i32;
                    widths[i] = widths[i].max(cell_width);
                }
            }
        }

        for (i, width) in widths.iter().enumerate() {
            self.table.set_col_width(i as i32, *width);
        }

        // Set cell values
        for (row_idx, row) in result.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let display_text = if cell.len() > 50 {
                    format!("{}...", &cell[..47])
                } else {
                    cell.clone()
                };
                self.table.set_cell_value(row_idx as i32, col_idx as i32, &display_text);
            }
        }

        *self.headers.borrow_mut() = col_names;
        self.table.redraw();
    }

    pub fn start_streaming(&mut self, headers: &[String]) {
        let col_count = headers.len() as i32;

        // Clear any pending data from previous queries
        self.pending_rows.borrow_mut().clear();
        self.pending_widths.borrow_mut().clear();
        *self.last_flush.borrow_mut() = Instant::now();

        // Initialize pending widths based on headers
        let initial_widths: Vec<i32> = headers
            .iter()
            .map(|h| (h.len() * 10).max(80) as i32)
            .collect();
        *self.pending_widths.borrow_mut() = initial_widths.clone();

        self.table.set_opts(Self::table_opts(0, col_count));

        for (i, name) in headers.iter().enumerate() {
            self.table.set_col_header_value(i as i32, name);
            self.table.set_col_width(i as i32, initial_widths[i]);
        }

        *self.headers.borrow_mut() = headers.to_vec();
        self.table.redraw();
        app::flush();
    }

    /// Append rows to the buffer. UI is updated periodically for performance.
    pub fn append_rows(&mut self, rows: Vec<Vec<String>>) {
        let max_cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        // Update pending column widths
        {
            let mut widths = self.pending_widths.borrow_mut();
            if widths.len() < max_cols {
                widths.resize(max_cols, 80);
            }
            for row in &rows {
                for (col_idx, cell) in row.iter().enumerate() {
                    if col_idx < widths.len() {
                        let cell_width = (cell.len() * 8).max(80).min(300) as i32;
                        if cell_width > widths[col_idx] {
                            widths[col_idx] = cell_width;
                        }
                    }
                }
            }
        }

        // Add rows to pending buffer
        self.pending_rows.borrow_mut().extend(rows);

        // Check if we should flush to UI
        let should_flush = {
            let elapsed = self.last_flush.borrow().elapsed();
            let buffered_count = self.pending_rows.borrow().len();
            elapsed >= UI_UPDATE_INTERVAL || buffered_count >= MAX_BUFFERED_ROWS
        };

        if should_flush {
            self.flush_pending();
        }
    }

    /// Flush all pending rows to the UI
    pub fn flush_pending(&mut self) {
        let rows_to_add: Vec<Vec<String>> = self.pending_rows.borrow_mut().drain(..).collect();
        if rows_to_add.is_empty() {
            return;
        }

        let current_rows = self.table.rows();
        let new_row_count = current_rows + rows_to_add.len() as i32;
        let max_cols_in_rows = rows_to_add
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(0) as i32;

        let pending_cols = self.pending_widths.borrow().len() as i32;
        let mut cols = self.table.cols().max(max_cols_in_rows).max(pending_cols);

        // Resize table to accommodate new rows and columns
        let mut headers = self.headers.borrow_mut();
        if headers.len() < cols as usize {
            headers.resize(cols as usize, String::new());
        }
        drop(headers);

        self.table
            .set_opts(Self::table_opts(new_row_count, cols));
        let headers = self.headers.borrow();
        for (i, header) in headers.iter().enumerate() {
            self.table.set_col_header_value(i as i32, header);
        }

        // Update column widths first (batch update)
        let widths = self.pending_widths.borrow();
        for (col_idx, &width) in widths.iter().enumerate() {
            if (col_idx as i32) < cols {
                let current_width = self.table.col_width(col_idx as i32);
                if width > current_width {
                    self.table.set_col_width(col_idx as i32, width);
                }
            }
        }
        drop(widths);

        // Add cell values
        for (row_offset, row) in rows_to_add.iter().enumerate() {
            let row_idx = current_rows + row_offset as i32;
            for (col_idx, cell) in row.iter().enumerate() {
                if (col_idx as i32) < cols {
                    let display_text = if cell.len() > 50 {
                        format!("{}...", &cell[..47])
                    } else {
                        cell.clone()
                    };
                    self.table.set_cell_value(row_idx, col_idx as i32, &display_text);
                }
            }
        }

        *self.last_flush.borrow_mut() = Instant::now();
        self.table.redraw();
        app::flush();
    }

    /// Call this when streaming is complete to flush any remaining buffered rows
    pub fn finish_streaming(&mut self) {
        self.flush_pending();
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.table.set_opts(Self::table_opts(0, 0));
        self.headers.borrow_mut().clear();
        self.table.redraw();
    }

    #[allow(dead_code)]
    pub fn get_selected_data(&self) -> Option<String> {
        let (row_top, row_bot, col_left, col_right) = self.table.get_selection();

        if row_top < 0 || col_left < 0 {
            return None;
        }

        let mut result = String::new();
        for row in row_top..=row_bot {
            let mut row_str = String::new();
            for col in col_left..=col_right {
                if !row_str.is_empty() {
                    row_str.push('\t');
                }
                row_str.push_str(&self.table.cell_value(row, col));
            }
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&row_str);
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Export all data to CSV format
    pub fn export_to_csv(&self) -> String {
        let headers = self.headers.borrow();
        let mut csv = String::new();

        // Header row
        let header_line: Vec<String> = headers.iter().map(|h| Self::escape_csv_field(h)).collect();
        csv.push_str(&header_line.join(","));
        csv.push('\n');

        // Data rows
        for row in 0..self.table.rows() {
            let mut row_fields = Vec::new();
            for col in 0..self.table.cols() {
                row_fields.push(Self::escape_csv_field(&self.table.cell_value(row, col)));
            }
            csv.push_str(&row_fields.join(","));
            csv.push('\n');
        }

        csv
    }

    fn escape_csv_field(field: &str) -> String {
        if field.contains(',') || field.contains('"') || field.contains('\n') {
            format!("\"{}\"", field.replace('"', "\"\""))
        } else {
            field.to_string()
        }
    }

    pub fn row_count(&self) -> usize {
        self.table.rows() as usize
    }

    pub fn has_data(&self) -> bool {
        self.table.rows() > 0
    }

    pub fn get_widget(&self) -> SmartTable {
        self.table.clone()
    }
}

impl Default for ResultTableWidget {
    fn default() -> Self {
        Self::new()
    }
}
