use fltk::{
    app,
    enums::{Align, Color, Event, Font, FrameType},
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
}

struct TableData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    col_widths: Vec<i32>,
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
        let data = Rc::new(RefCell::new(TableData::new()));

        let mut table = Table::default_fill();
        table.set_rows(0);
        table.set_cols(0);
        table.set_row_header(true);
        table.set_row_header_width(60);
        table.set_col_header(true);
        table.set_col_header_height(25);
        table.set_row_height_all(25);
        table.set_color(Color::from_rgb(30, 30, 30));
        table.set_selection_color(Color::from_rgb(38, 79, 120));

        let data_clone = data.clone();
        table.draw_cell(move |t, ctx, row, col, x, y, w, h| {
            let data = data_clone.borrow();

            match ctx {
                TableContext::StartPage => {
                    draw::set_font(Font::Helvetica, 14);
                }
                TableContext::ColHeader => {
                    draw::push_clip(x, y, w, h);
                    draw::draw_box(
                        FrameType::FlatBox,
                        x,
                        y,
                        w,
                        h,
                        Color::from_rgb(60, 60, 63),
                    );
                    draw::set_draw_color(Color::White);
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
                        Color::from_rgb(60, 60, 63),
                    );
                    draw::set_draw_color(Color::White);
                    draw::set_font(Font::Helvetica, 12);
                    draw::draw_text2(&format!("{}", row + 1), x, y, w, h, Align::Center);
                    draw::pop_clip();
                }
                TableContext::Cell => {
                    draw::push_clip(x, y, w, h);

                    // Alternate row colors
                    let bg_color = if row % 2 == 0 {
                        Color::from_rgb(30, 30, 30)
                    } else {
                        Color::from_rgb(40, 40, 43)
                    };

                    // Check if cell is selected
                    let (row_top, row_bot, col_left, col_right) = t.get_selection();
                    let is_selected = row >= row_top
                        && row <= row_bot
                        && col >= col_left
                        && col <= col_right;

                    let bg = if is_selected {
                        Color::from_rgb(38, 79, 120)
                    } else {
                        bg_color
                    };

                    draw::draw_box(FrameType::FlatBox, x, y, w, h, bg);

                    // Draw cell border
                    draw::set_draw_color(Color::from_rgb(60, 60, 63));
                    draw::draw_rect(x, y, w, h);

                    // Draw cell text
                    draw::set_draw_color(Color::from_rgb(220, 220, 220));
                    draw::set_font(Font::Courier, 12);

                    if let Some(row_data) = data.rows.get(row as usize) {
                        if let Some(cell) = row_data.get(col as usize) {
                            let display_text = if cell.len() > 50 {
                                format!("{}...", &cell[..47])
                            } else {
                                cell.clone()
                            };
                            draw::draw_text2(&display_text, x + 5, y, w - 10, h, Align::Left);
                        }
                    }
                    draw::pop_clip();
                }
                _ => {}
            }
        });

        // Setup right-click context menu for copy
        let data_for_handle = data.clone();
        table.handle(move |t, ev| {
            match ev {
                Event::Push => {
                    if app::event_mouse_button() == app::MouseButton::Right {
                        Self::show_context_menu(t, &data_for_handle);
                        return true;
                    }
                    false
                }
                Event::KeyDown => {
                    // Ctrl+C to copy
                    if app::event_state().contains(fltk::enums::Shortcut::Ctrl) {
                        let key = app::event_key();
                        if key == fltk::enums::Key::from_char('c') {
                            Self::copy_selected_to_clipboard(t, &data_for_handle);
                            return true;
                        }
                    }
                    false
                }
                _ => false,
            }
        });

        Self { table, data }
    }

    fn show_context_menu(table: &Table, data: &Rc<RefCell<TableData>>) {
        let mut menu = MenuButton::default();
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
