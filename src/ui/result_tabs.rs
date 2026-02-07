use fltk::{
    app,
    enums::{Event, Key},
    group::{Group, Tabs, TabsOverflow},
    prelude::*,
    text::{TextBuffer, TextDisplay},
    widget::Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::ui::font_settings::{configured_editor_profile, FontProfile};
use crate::ui::constants;
use crate::ui::theme;
use crate::ui::ResultTableWidget;

#[derive(Clone)]
pub struct ResultTabsWidget {
    tabs: Tabs,
    data: Rc<RefCell<Vec<ResultTab>>>,
    active_index: Rc<RefCell<Option<usize>>>,
    script_output: Rc<RefCell<ScriptOutputTab>>,
    font_profile: Rc<Cell<FontProfile>>,
    font_size: Rc<Cell<u32>>,
}

#[derive(Clone)]
struct ResultTab {
    group: Group,
    table: ResultTableWidget,
}

#[derive(Clone)]
struct ScriptOutputTab {
    group: Group,
    display: TextDisplay,
    buffer: TextBuffer,
}

impl ResultTabsWidget {
    fn buffer_ends_with_newline(buffer: &TextBuffer) -> bool {
        let len = buffer.length();
        if len <= 0 {
            return false;
        }
        buffer
            .text_range(len - 1, len)
            .map(|s| s == "\n")
            .unwrap_or(false)
    }

    fn trim_script_output_buffer(buffer: &mut TextBuffer) {
        let max_chars = constants::SCRIPT_OUTPUT_MAX_CHARS;
        let target_chars = constants::SCRIPT_OUTPUT_TRIM_TARGET_CHARS.min(max_chars);
        let len = buffer.length().max(0) as usize;
        if len <= max_chars {
            return;
        }

        let remove_upto = len.saturating_sub(target_chars);
        if remove_upto == 0 {
            return;
        }

        let prefix = buffer.text_range(0, remove_upto as i32).unwrap_or_default();
        let cut = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(remove_upto);
        if cut > 0 {
            buffer.remove(0, cut as i32);
        }
    }

    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        // Use explicit dimensions to avoid "center of requires the size of the
        // widget to be known" panic that occurs with default_fill()
        let mut tabs = Tabs::new(x, y, w, h, None);
        tabs.set_color(theme::panel_bg());
        tabs.set_selection_color(theme::selection_strong());
        tabs.set_label_color(theme::text_secondary());
        tabs.handle_overflow(TabsOverflow::Compress);

        let data = Rc::new(RefCell::new(Vec::<ResultTab>::new()));
        let active_index = Rc::new(RefCell::new(None));
        let font_profile = Rc::new(Cell::new(configured_editor_profile()));
        let font_size = Rc::new(Cell::new(constants::DEFAULT_FONT_SIZE as u32));

        tabs.begin();
        let x = tabs.x();
        let y = tabs.y() + constants::TAB_HEADER_HEIGHT;
        let w = tabs.w().max(100);
        let h = (tabs.h() - constants::TAB_HEADER_HEIGHT).max(100);
        let mut script_group = Group::new(x, y, w, h, None).with_label("Script Output");
        script_group.set_color(theme::panel_bg());
        script_group.set_label_color(theme::text_secondary());
        script_group.begin();
        let padding = constants::SCRIPT_OUTPUT_PADDING;
        let display_x = x + padding;
        let display_y = y + padding;
        let display_w = (w - padding * 2).max(10);
        let display_h = (h - padding * 2).max(10);
        let mut script_display = TextDisplay::new(display_x, display_y, display_w, display_h, None);
        script_display.set_color(theme::panel_bg());
        script_display.set_text_color(theme::text_primary());
        let script_profile = font_profile.get();
        script_display.set_text_font(script_profile.normal);
        script_display.set_text_size(font_size.get() as i32);
        let mut script_buffer = TextBuffer::default();
        script_buffer.set_text("");
        script_display.set_buffer(script_buffer.clone());
        script_group.resizable(&script_display);
        script_group.end();
        tabs.end();

        let script_output = Rc::new(RefCell::new(ScriptOutputTab {
            group: script_group,
            display: script_display,
            buffer: script_buffer,
        }));

        let data_for_cb = data.clone();
        let active_for_cb = active_index.clone();
        let script_for_cb = script_output.clone();
        tabs.set_callback(move |t| {
            if let Some(widget) = t.value() {
                let ptr = widget.as_widget_ptr();
                let script_ptr = script_for_cb.borrow().group.as_widget_ptr();
                if ptr == script_ptr {
                    *active_for_cb.borrow_mut() = None;
                    return;
                }
                let data = data_for_cb.borrow();
                let index = data.iter().position(|tab| tab.group.as_widget_ptr() == ptr);
                *active_for_cb.borrow_mut() = index;
            }
        });

        let tabs_for_key = tabs.clone();
        tabs.handle(move |_, ev| {
            if !matches!(ev, Event::KeyDown) {
                return false;
            }

            let key = app::event_key();
            if !matches!(key, Key::Left | Key::Right | Key::Up | Key::Down) {
                return false;
            }

            let children: Vec<Group> = tabs_for_key
                .clone()
                .into_iter()
                .filter_map(|w| w.as_group())
                .collect();
            if children.is_empty() {
                return true;
            }

            let current_ptr = tabs_for_key.value().map(|w| w.as_widget_ptr());
            let index = current_ptr
                .and_then(|ptr| children.iter().position(|g| g.as_widget_ptr() == ptr))
                .unwrap_or(0);

            match key {
                Key::Left | Key::Up => index == 0,
                Key::Right | Key::Down => index + 1 >= children.len(),
                _ => false,
            }
        });

        let tab_header_height = constants::TAB_HEADER_HEIGHT;
        tabs.resize_callback(move |t, x, y, w, h| {
            let content_y = y + tab_header_height;
            let content_h = (h - tab_header_height).max(100);
            for child in t.clone().into_iter() {
                if let Some(mut group) = child.as_group() {
                    group.resize(x, content_y, w, content_h);
                }
            }
        });

        Self {
            tabs,
            data,
            active_index,
            script_output,
            font_profile,
            font_size,
        }
    }

    pub fn get_widget(&self) -> Tabs {
        self.tabs.clone()
    }

    pub fn apply_font_settings(&mut self, profile: FontProfile, size: u32) {
        self.font_profile.set(profile);
        self.font_size.set(size);
        {
            let mut script_output = self.script_output.borrow_mut();
            script_output.display.set_text_font(profile.normal);
            script_output.display.set_text_size(size as i32);
            script_output.display.redraw();
        }
        let mut data = self.data.borrow_mut();
        for tab in data.iter_mut() {
            tab.table.apply_font_settings(profile, size);
        }
    }

    pub fn clear(&mut self) {
        let tabs_to_delete: Vec<_> = self.data.borrow_mut().drain(..).collect();
        for tab in tabs_to_delete {
            self.delete_tab(tab);
        }
        self.clear_script_output();
        *self.active_index.borrow_mut() = None;
        let script_group = {
            let script_output = self.script_output.borrow();
            script_output.group.clone()
        };
        let _ = self.tabs.set_value(&script_group);
        self.tabs.redraw();
        let script_output = self.script_output.borrow();
        let mut script_group = script_output.group.clone();
        let mut script_display = script_output.display.clone();
        script_group.redraw();
        script_display.redraw();
    }

    pub fn tab_count(&self) -> usize {
        self.data.borrow().len()
    }

    pub fn append_script_output_lines(&mut self, lines: &[String]) {
        let mut script_output = self.script_output.borrow_mut();
        let mut buffer = script_output.buffer.clone();
        if lines.is_empty() {
            return;
        }
        if buffer.length() > 0 && !Self::buffer_ends_with_newline(&buffer) {
            buffer.append("\n");
        }
        for (idx, line) in lines.iter().enumerate() {
            buffer.append(line);
            if idx + 1 < lines.len() {
                buffer.append("\n");
            }
        }
        buffer.append("\n");
        Self::trim_script_output_buffer(&mut buffer);
        let line_count = script_output.display.count_lines(0, buffer.length(), true);
        script_output.display.scroll(line_count, 0);
    }

    pub fn start_statement(&mut self, index: usize, label: &str) {
        let current_len = self.data.borrow().len();
        if index < current_len {
            // Extract the group before calling set_value to avoid re-entrant borrow
            // when the tabs callback fires
            let group = self.data.borrow()[index].group.clone();
            let _ = self.tabs.set_value(&group);
            *self.active_index.borrow_mut() = Some(index);
            return;
        }

        self.tabs.begin();
        // Use explicit size from tabs instead of default_fill() to avoid
        // "center of requires the size of the widget to be known" panic
        // Use minimum dimensions (100x100) if tabs size is not yet known
        let x = self.tabs.x();
        let y = self.tabs.y() + constants::TAB_HEADER_HEIGHT;
        let w = self.tabs.w().max(100);
        let h = (self.tabs.h() - constants::TAB_HEADER_HEIGHT).max(100);
        let mut group = Group::new(x, y, w, h, None).with_label(label);
        group.set_color(theme::panel_bg());
        group.set_label_color(theme::text_secondary());

        group.begin();
        let mut table = ResultTableWidget::with_size(x, y, w, h);
        table.apply_font_settings(self.font_profile.get(), self.font_size.get());
        let widget = table.get_widget();
        group.resizable(&widget);
        group.end();
        self.tabs.end();

        self.data.borrow_mut().push(ResultTab { group, table });
        let new_index = self.data.borrow().len().saturating_sub(1);
        // Extract the group before calling set_value to avoid re-entrant borrow
        // when the tabs callback fires
        let group = self.data.borrow()[new_index].group.clone();
        let _ = self.tabs.set_value(&group);
        *self.active_index.borrow_mut() = Some(new_index);
    }

    pub fn start_streaming(&mut self, index: usize, columns: &[String]) {
        if let Some(tab) = self.data.borrow().get(index) {
            let mut table = tab.table.clone();
            table.start_streaming(columns);
        }
    }

    pub fn append_rows(&mut self, index: usize, rows: Vec<Vec<String>>) {
        if let Some(tab) = self.data.borrow().get(index) {
            let mut table = tab.table.clone();
            table.append_rows(rows);
        }
    }

    pub fn finish_streaming(&mut self, index: usize) {
        if let Some(tab) = self.data.borrow().get(index) {
            let mut table = tab.table.clone();
            table.finish_streaming();
        }
    }

    pub fn finish_all_streaming(&mut self) {
        let tables = self.data.borrow();
        for tab in tables.iter() {
            let mut table = tab.table.clone();
            table.finish_streaming();
        }
    }

    pub fn display_result(&mut self, index: usize, result: &crate::db::QueryResult) {
        if let Some(tab) = self.data.borrow().get(index) {
            let mut table = tab.table.clone();
            table.display_result(result);
        }
    }

    pub fn export_to_csv(&self) -> String {
        self.current_table()
            .map(|table| table.export_to_csv())
            .unwrap_or_default()
    }

    pub fn row_count(&self) -> usize {
        self.current_table()
            .map(|table| table.row_count())
            .unwrap_or(0)
    }

    pub fn has_data(&self) -> bool {
        self.current_table()
            .map(|table| table.has_data())
            .unwrap_or(false)
    }

    fn current_table(&self) -> Option<ResultTableWidget> {
        let index = *self.active_index.borrow();
        index
            .and_then(|idx| self.data.borrow().get(idx).cloned())
            .map(|tab| tab.table)
    }

    pub fn copy(&self) -> usize {
        if let Some(table) = self.current_table() {
            table.copy()
        } else {
            0
        }
    }

    pub fn copy_with_headers(&self) {
        if let Some(table) = self.current_table() {
            table.copy_with_headers();
        }
    }

    pub fn select_all(&self) {
        if let Some(mut table) = self.current_table() {
            table.select_all();
        }
    }

    fn delete_tab(&mut self, mut tab: ResultTab) {
        // FLTK memory management: proper cleanup order is critical
        // 1. Clear callbacks on child widgets to release captured Rc<RefCell<T>> references
        // 2. Remove child widgets from parent before deletion
        // 3. Delete child widgets
        // 4. Delete parent container

        // Step 1: Cleanup the table widget (clears callbacks and data buffers)
        tab.table.cleanup();

        // Step 2 & 3: Get the Table widget and remove from group, then delete
        let table_widget = tab.table.get_widget();
        let mut group = tab.group;
        group.remove(&table_widget);

        // Delete the Table widget
        unsafe {
            let widget = Widget::from_widget_ptr(table_widget.as_widget_ptr());
            Widget::delete(widget);
        }

        // Step 4: Remove group from tabs and delete
        if self.tabs.find(&group) >= 0 {
            self.tabs.remove(&group);
        }
        fltk::group::Group::delete(group);
    }

    pub fn select_script_output(&mut self) {
        let script_group = self.script_output.borrow().group.clone();
        let _ = self.tabs.set_value(&script_group);
        *self.active_index.borrow_mut() = None;
    }

    fn clear_script_output(&self) {
        let mut script_output = self.script_output.borrow_mut();
        let mut buffer = TextBuffer::default();
        buffer.set_text("");
        script_output.display.set_buffer(buffer.clone());
        script_output.buffer = buffer;
    }
}

impl Default for ResultTabsWidget {
    fn default() -> Self {
        Self::new(0, 0, 100, 100)
    }
}
