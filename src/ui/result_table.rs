use fltk::{
    app,
    enums::{Align, Color, Event, Font, FrameType, Key, Shortcut},
    menu::MenuButton,
    prelude::*,
    table::{Table, TableContext},
    draw,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::QueryResult;

#[derive(Clone)]
pub struct ResultTableWidget {
    table: Table,
    data: Rc<RefCell<TableData>>,
    drag_state: Rc<RefCell<DragState>>,
}

struct TableData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    col_widths: Vec<i32>,
}

#[derive(Default)]
struct DragState {
    is_dragging: bool,
    start_row: i32,
    start_col: i32,
}

impl TableData {
    fn new() -> Self {
        Self {
            headers: vec![],
            rows: vec![],
            col_widths: vec![],
        }
    }

    fn clear(&mut self) {
        self.headers.clear();
        self.rows.clear();
        self.col_widths.clear();
    }

    fn set_data(&mut self, result: &QueryResult) {
        self.clear();

        if !result.is_select {
            return;
        }

        self.headers = result.columns.iter().map(|c| c.name.clone()).collect();
        self.rows = result.rows.clone();

        // Calculate column widths based on content
        let mut widths: Vec<i32> = self.headers.iter().map(|h| (h.len() * 10).max(80) as i32).collect();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    let cell_width = (cell.len() * 8).max(80).min(300) as i32;
                    widths[i] = widths[i].max(cell_width);
                }
            }
        }

        self.col_widths = widths;
    }

    fn start_streaming(&mut self, headers: &[String]) {
        self.clear();
        self.headers = headers.to_vec();
        self.col_widths = self
            .headers
            .iter()
            .map(|h| (h.len() * 10).max(80) as i32)
            .collect();
    }

    fn append_rows(&mut self, rows: Vec<Vec<String>>) -> Vec<(usize, i32)> {
        if self.headers.is_empty() {
            return Vec::new();
        }

        let mut changed_widths: Vec<(usize, i32)> = Vec::new();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < self.col_widths.len() {
                    let cell_width = (cell.len() * 8).max(80).min(300) as i32;
                    if cell_width > self.col_widths[i] {
                        self.col_widths[i] = cell_width;
                        changed_widths.push((i, cell_width));
                    }
                }
            }
            self.rows.push(row);
        }

        changed_widths
    }
}

impl ResultTableWidget {
    pub fn new() -> Self {
        Self::with_size(0, 0, 100, 100)
    }

    pub fn with_size(x: i32, y: i32, w: i32, h: i32) -> Self {
        let data = Rc::new(RefCell::new(TableData::new()));
        let drag_state = Rc::new(RefCell::new(DragState::default()));

        let mut table = Table::new(x, y, w, h, None);
        table.set_rows(0);
        table.set_cols(0);
        table.set_row_header(true);
        table.set_row_header_width(55);
        table.set_col_header(true);
        table.set_col_header_height(28);
        table.set_row_height_all(26);
        table.set_color(Color::from_rgb(30, 30, 30)); // Modern dark background
        table.set_selection_color(Color::from_rgb(38, 79, 120));

        let data_clone = data.clone();
        table.draw_cell(move |t, ctx, row, col, x, y, w, h| {
            let data = data_clone.borrow();

            match ctx {
                TableContext::StartPage => {
                    draw::set_font(Font::Helvetica, 13);
                }
                TableContext::ColHeader => {
                    draw::push_clip(x, y, w, h);
                    // Modern header with subtle gradient effect
                    draw::draw_box(
                        FrameType::FlatBox,
                        x,
                        y,
                        w,
                        h,
                        Color::from_rgb(45, 45, 48), // Modern header background
                    );
                    // Bottom border for header
                    draw::set_draw_color(Color::from_rgb(0, 120, 212)); // Accent line
                    draw::draw_line(x, y + h - 1, x + w, y + h - 1);

                    draw::set_draw_color(Color::from_rgb(220, 220, 220));
                    draw::set_font(Font::HelveticaBold, 12);

                    if let Some(header) = data.headers.get(col as usize) {
                        draw::draw_text2(header, x, y, w, h, Align::Center);
                    }
                    draw::pop_clip();
                }
                TableContext::RowHeader => {
                    draw::push_clip(x, y, w, h);
                    draw::draw_box(
                        FrameType::FlatBox,
                        x,
                        y,
                        w,
                        h,
                        Color::from_rgb(45, 45, 48), // Modern row header
                    );
                    draw::set_draw_color(Color::from_rgb(140, 140, 140)); // Subtle text
                    draw::set_font(Font::Helvetica, 11);
                    draw::draw_text2(&format!("{}", row + 1), x, y, w, h, Align::Center);
                    draw::pop_clip();
                }
                TableContext::Cell => {
                    draw::push_clip(x, y, w, h);

                    // Modern alternating row colors
                    let bg_color = if row % 2 == 0 {
                        Color::from_rgb(30, 30, 30) // Even row
                    } else {
                        Color::from_rgb(37, 37, 38) // Odd row - subtle difference
                    };

                    // Check if cell is selected
                    let (row_top, row_bot, col_left, col_right) = t.get_selection();
                    let is_selected = row >= row_top
                        && row <= row_bot
                        && col >= col_left
                        && col <= col_right;

                    let bg = if is_selected {
                        Color::from_rgb(38, 79, 120) // Selection color
                    } else {
                        bg_color
                    };

                    draw::draw_box(FrameType::FlatBox, x, y, w, h, bg);

                    // Subtle cell border
                    draw::set_draw_color(Color::from_rgb(50, 50, 52));
                    draw::draw_line(x + w - 1, y, x + w - 1, y + h); // Vertical
                    draw::draw_line(x, y + h - 1, x + w, y + h - 1); // Horizontal

                    // Cell text
                    draw::set_draw_color(Color::from_rgb(212, 212, 212));
                    draw::set_font(Font::Courier, 12);

                    if let Some(row_data) = data.rows.get(row as usize) {
                        if let Some(cell) = row_data.get(col as usize) {
                            let display_text = if cell.len() > 50 {
                                format!("{}...", &cell[..47])
                            } else {
                                cell.clone()
                            };
                            draw::draw_text2(&display_text, x + 6, y, w - 12, h, Align::Left);
                        }
                    }
                    draw::pop_clip();
                }
                _ => {}
            }
        });

        // Setup event handler for mouse selection and keyboard shortcuts
        let data_for_handle = data.clone();
        let drag_state_for_handle = drag_state.clone();
        table.handle(move |t, ev| {
            match ev {
                Event::Push => {
                    if app::event_mouse_button() == app::MouseButton::Right {
                        Self::show_context_menu(t, &data_for_handle);
                        return true;
                    }
                    // Left click - start drag selection
                    if app::event_mouse_button() == app::MouseButton::Left {
                        let _ = t.take_focus();
                        let (row, col) = Self::get_cell_at_mouse(t);
                        if row >= 0 && col >= 0 {
                            let mut state = drag_state_for_handle.borrow_mut();
                            state.is_dragging = true;
                            state.start_row = row;
                            state.start_col = col;
                            t.set_selection(row, col, row, col);
                            t.redraw();
                            return true;
                        }
                    }
                    false
                }
                Event::Drag => {
                    // Mouse drag - extend selection
                    let state = drag_state_for_handle.borrow();
                    if state.is_dragging {
                        let (row, col) = Self::get_cell_at_mouse(t);
                        if row >= 0 && col >= 0 {
                            let (r1, r2) = if state.start_row <= row {
                                (state.start_row, row)
                            } else {
                                (row, state.start_row)
                            };
                            let (c1, c2) = if state.start_col <= col {
                                (state.start_col, col)
                            } else {
                                (col, state.start_col)
                            };
                            t.set_selection(r1, c1, r2, c2);
                            t.redraw();
                            return true;
                        }
                    }
                    false
                }
                Event::Released => {
                    // End drag selection
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
                            // Ctrl+A - Select all
                            k if k == Key::from_char('a') => {
                                let rows = t.rows();
                                let cols = t.cols();
                                if rows > 0 && cols > 0 {
                                    t.set_selection(0, 0, rows - 1, cols - 1);
                                    t.redraw();
                                }
                                return true;
                            }
                            // Ctrl+C - Copy selected cells
                            k if k == Key::from_char('c') => {
                                Self::copy_selected_to_clipboard(t, &data_for_handle);
                                return true;
                            }
                            // Ctrl+H - Copy with headers
                            k if k == Key::from_char('h') => {
                                Self::copy_selected_with_headers(t, &data_for_handle);
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

        Self { table, data, drag_state }
    }

    /// Get cell row/col at current mouse position
    fn get_cell_at_mouse(table: &Table) -> (i32, i32) {
        let mouse_x = app::event_x();
        let mouse_y = app::event_y();

        if table.rows() <= 0 || table.cols() <= 0 {
            return (-1, -1);
        }

        if let Some((row, col)) = Self::cell_at_point(table, mouse_x, mouse_y) {
            return (row, col);
        }

        let row = Self::row_at_y(table, mouse_y);
        let col = Self::col_at_x(table, mouse_x);

        match (row, col) {
            (Some(r), Some(c)) => (r, c),
            _ => (-1, -1),
        }
    }

    fn show_context_menu(table: &Table, data: &Rc<RefCell<TableData>>) {
        // Get mouse position for proper popup placement
        let mouse_x = app::event_x();
        let mouse_y = app::event_y();

        // Create menu at mouse position with explicit size
        let mut menu = MenuButton::new(mouse_x, mouse_y, 0, 0, None);
        menu.set_color(Color::from_rgb(45, 45, 48));
        menu.set_text_color(Color::White);
        menu.add_choice("Copy|Copy with Headers|Copy Cell|Copy All");

        if let Some(choice) = menu.popup() {
            let choice_label = choice.label().unwrap_or_default();
            match choice_label.as_str() {
                "Copy" => {
                    Self::copy_selected_to_clipboard(table, data);
                }
                "Copy with Headers" => {
                    Self::copy_selected_with_headers(table, data);
                }
                "Copy Cell" => {
                    Self::copy_current_cell(table, data);
                }
                "Copy All" => {
                    Self::copy_all_to_clipboard(data);
                }
                _ => {}
            }
        }
    }

    fn cell_at_point(table: &Table, x: i32, y: i32) -> Option<(i32, i32)> {
        for row in 0..table.rows() {
            for col in 0..table.cols() {
                if let Some((cell_x, cell_y, cell_w, cell_h)) =
                    table.find_cell(TableContext::Cell, row, col)
                {
                    if x >= cell_x
                        && x < cell_x + cell_w
                        && y >= cell_y
                        && y < cell_y + cell_h
                    {
                        return Some((row, col));
                    }
                }
            }
        }
        None
    }

    fn row_at_y(table: &Table, y: i32) -> Option<i32> {
        let mut last_row: Option<i32> = None;
        let mut last_y_end: Option<i32> = None;
        for row in 0..table.rows() {
            if let Some((_, cell_y, _, cell_h)) = table.find_cell(TableContext::Cell, row, 0) {
                last_row = Some(row);
                last_y_end = Some(cell_y + cell_h);
                if y < cell_y {
                    return Some(row);
                }
                if y >= cell_y && y < cell_y + cell_h {
                    return Some(row);
                }
            }
        }
        if let (Some(row), Some(end)) = (last_row, last_y_end) {
            if y >= end {
                return Some(row);
            }
        }
        None
    }

    fn col_at_x(table: &Table, x: i32) -> Option<i32> {
        let mut last_col: Option<i32> = None;
        let mut last_x_end: Option<i32> = None;
        for col in 0..table.cols() {
            if let Some((cell_x, _, cell_w, _)) = table.find_cell(TableContext::Cell, 0, col) {
                last_col = Some(col);
                last_x_end = Some(cell_x + cell_w);
                if x < cell_x {
                    return Some(col);
                }
                if x >= cell_x && x < cell_x + cell_w {
                    return Some(col);
                }
            }
        }
        if let (Some(col), Some(end)) = (last_col, last_x_end) {
            if x >= end {
                return Some(col);
            }
        }
        None
    }

    fn copy_selected_to_clipboard(table: &Table, data: &Rc<RefCell<TableData>>) {
        let (row_top, row_bot, col_left, col_right) = table.get_selection();
        if row_top < 0 || col_left < 0 {
            return;
        }

        let data = data.borrow();
        let mut result = String::new();

        for row in row_top..=row_bot {
            if let Some(row_data) = data.rows.get(row as usize) {
                let mut row_str = String::new();
                for col in col_left..=col_right {
                    if let Some(cell) = row_data.get(col as usize) {
                        if !row_str.is_empty() {
                            row_str.push('\t');
                        }
                        row_str.push_str(cell);
                    }
                }
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(&row_str);
            }
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    fn copy_selected_with_headers(table: &Table, data: &Rc<RefCell<TableData>>) {
        let (row_top, row_bot, col_left, col_right) = table.get_selection();
        if row_top < 0 || col_left < 0 {
            return;
        }

        let data = data.borrow();
        let mut result = String::new();

        // Add headers first
        let mut header_str = String::new();
        for col in col_left..=col_right {
            if let Some(header) = data.headers.get(col as usize) {
                if !header_str.is_empty() {
                    header_str.push('\t');
                }
                header_str.push_str(header);
            }
        }
        result.push_str(&header_str);
        result.push('\n');

        // Add data rows
        for row in row_top..=row_bot {
            if let Some(row_data) = data.rows.get(row as usize) {
                let mut row_str = String::new();
                for col in col_left..=col_right {
                    if let Some(cell) = row_data.get(col as usize) {
                        if !row_str.is_empty() {
                            row_str.push('\t');
                        }
                        row_str.push_str(cell);
                    }
                }
                result.push_str(&row_str);
                result.push('\n');
            }
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    fn copy_current_cell(table: &Table, data: &Rc<RefCell<TableData>>) {
        let (row_top, _, col_left, _) = table.get_selection();
        if row_top < 0 || col_left < 0 {
            return;
        }

        let data = data.borrow();
        if let Some(row_data) = data.rows.get(row_top as usize) {
            if let Some(cell) = row_data.get(col_left as usize) {
                app::copy(cell);
            }
        }
    }

    fn copy_all_to_clipboard(data: &Rc<RefCell<TableData>>) {
        let data = data.borrow();
        let mut result = String::new();

        // Add headers
        result.push_str(&data.headers.join("\t"));
        result.push('\n');

        // Add all rows
        for row in &data.rows {
            result.push_str(&row.join("\t"));
            result.push('\n');
        }

        if !result.is_empty() {
            app::copy(&result);
        }
    }

    pub fn display_result(&mut self, result: &QueryResult) {
        let (is_select, row_count, col_count, col_widths) = {
            let mut data = self.data.borrow_mut();
            data.set_data(result);

            if result.is_select {
                (
                    true,
                    data.rows.len(),
                    data.headers.len(),
                    data.col_widths.clone(),
                )
            } else {
                (false, 0, 0, Vec::new())
            }
        };

        if is_select {
            self.table.set_rows(row_count as i32);
            self.table.set_cols(col_count as i32);

            for (i, width) in col_widths.iter().enumerate() {
                self.table.set_col_width(i as i32, *width);
            }
        } else {
            self.table.set_rows(0);
            self.table.set_cols(0);
        }

        self.table.redraw();
    }

    pub fn start_streaming(&mut self, headers: &[String]) {
        let col_widths = {
            let mut data = self.data.borrow_mut();
            data.start_streaming(headers);
            data.col_widths.clone()
        };

        self.table.set_rows(0);
        self.table.set_cols(headers.len() as i32);
        for (i, width) in col_widths.iter().enumerate() {
            self.table.set_col_width(i as i32, *width);
        }
        self.table.redraw();
        app::flush();
    }

    pub fn append_rows(&mut self, rows: Vec<Vec<String>>) {
        let (row_count, width_updates) = {
            let mut data = self.data.borrow_mut();
            let updates = data.append_rows(rows);
            (data.rows.len(), updates)
        };

        self.table.set_rows(row_count as i32);
        for (index, width) in width_updates {
            self.table.set_col_width(index as i32, width);
        }
        self.table.redraw();
        app::flush();
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.data.borrow_mut().clear();
        self.table.set_rows(0);
        self.table.set_cols(0);
        self.table.redraw();
    }

    #[allow(dead_code)]
    pub fn get_selected_data(&self) -> Option<String> {
        let (row_top, row_bot, col_left, col_right) = self.table.get_selection();

        if row_top < 0 || col_left < 0 {
            return None;
        }

        let data = self.data.borrow();
        let mut result = String::new();

        for row in row_top..=row_bot {
            if let Some(row_data) = data.rows.get(row as usize) {
                let mut row_str = String::new();
                for col in col_left..=col_right {
                    if let Some(cell) = row_data.get(col as usize) {
                        if !row_str.is_empty() {
                            row_str.push('\t');
                        }
                        row_str.push_str(cell);
                    }
                }
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(&row_str);
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Export all data to CSV format
    pub fn export_to_csv(&self) -> String {
        let data = self.data.borrow();
        let mut csv = String::new();

        // Header row
        let header_line: Vec<String> = data
            .headers
            .iter()
            .map(|h| Self::escape_csv_field(h))
            .collect();
        csv.push_str(&header_line.join(","));
        csv.push('\n');

        // Data rows
        for row in &data.rows {
            let row_line: Vec<String> = row.iter().map(|c| Self::escape_csv_field(c)).collect();
            csv.push_str(&row_line.join(","));
            csv.push('\n');
        }

        csv
    }

    /// Escape a CSV field (add quotes if needed)
    fn escape_csv_field(field: &str) -> String {
        if field.contains(',') || field.contains('"') || field.contains('\n') {
            format!("\"{}\"", field.replace('"', "\"\""))
        } else {
            field.to_string()
        }
    }

    /// Get row count
    pub fn row_count(&self) -> usize {
        self.data.borrow().rows.len()
    }

    /// Check if there is data
    pub fn has_data(&self) -> bool {
        !self.data.borrow().rows.is_empty()
    }

    /// Get the table widget
    pub fn get_widget(&self) -> Table {
        self.table.clone()
    }
}

impl Default for ResultTableWidget {
    fn default() -> Self {
        Self::new()
    }
}
