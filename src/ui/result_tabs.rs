use fltk::{
    enums::Color,
    group::{Group, Tabs, TabsOverflow},
    prelude::*,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::ui::ResultTableWidget;

#[derive(Clone)]
pub struct ResultTabsWidget {
    tabs: Tabs,
    data: Rc<RefCell<Vec<ResultTab>>>,
    active_index: Rc<RefCell<Option<usize>>>,
}

#[derive(Clone)]
struct ResultTab {
    group: Group,
    table: ResultTableWidget,
}

impl ResultTabsWidget {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        // Use explicit dimensions to avoid "center of requires the size of the
        // widget to be known" panic that occurs with default_fill()
        let mut tabs = Tabs::new(x, y, w, h, None);
        tabs.set_color(Color::from_rgb(37, 37, 38)); // Modern panel background
        tabs.set_selection_color(Color::from_rgb(0, 120, 212)); // Active tab accent
        tabs.set_label_color(Color::from_rgb(200, 200, 200)); // Tab text color
        tabs.handle_overflow(TabsOverflow::Compress);

        let data = Rc::new(RefCell::new(Vec::<ResultTab>::new()));
        let active_index = Rc::new(RefCell::new(None));

        let data_for_cb = data.clone();
        let active_for_cb = active_index.clone();
        tabs.set_callback(move |t| {
            if let Some(widget) = t.value() {
                let ptr = widget.as_widget_ptr();
                let data = data_for_cb.borrow();
                let index = data
                    .iter()
                    .position(|tab| tab.group.as_widget_ptr() == ptr);
                *active_for_cb.borrow_mut() = index;
            }
        });

        tabs.end();

        Self {
            tabs,
            data,
            active_index,
        }
    }

    pub fn get_widget(&self) -> Tabs {
        self.tabs.clone()
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
        self.data.borrow_mut().clear();
        *self.active_index.borrow_mut() = None;
    }

    pub fn start_statement(&mut self, index: usize, label: &str) {
        let current_len = self.data.borrow().len();
        if index < current_len {
            let _ = self
                .tabs
                .set_value(&self.data.borrow()[index].group);
            *self.active_index.borrow_mut() = Some(index);
            return;
        }

        self.tabs.begin();
        // Use explicit size from tabs instead of default_fill() to avoid
        // "center of requires the size of the widget to be known" panic
        // Use minimum dimensions (100x100) if tabs size is not yet known
        let x = self.tabs.x();
        let y = self.tabs.y() + 25;
        let w = self.tabs.w().max(100);
        let h = (self.tabs.h() - 25).max(100);
        let mut group = Group::new(x, y, w, h, None).with_label(label);
        group.set_color(Color::from_rgb(30, 30, 30)); // Tab content background
        group.set_label_color(Color::from_rgb(200, 200, 200)); // Tab label

        group.begin();
        let table = ResultTableWidget::with_size(x, y, w, h);
        let widget = table.get_widget();
        group.resizable(&*widget);
        group.end();
        self.tabs.end();

        self.data.borrow_mut().push(ResultTab { group, table });
        let _ = self
            .tabs
            .set_value(&self.data.borrow()[index].group);
        *self.active_index.borrow_mut() = Some(index);
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
}

impl Default for ResultTabsWidget {
    fn default() -> Self {
        Self::new(0, 0, 100, 100)
    }
}
