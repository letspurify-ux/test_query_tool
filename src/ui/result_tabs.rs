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
    pub fn new() -> Self {
        let mut tabs = Tabs::default_fill();
        tabs.set_color(Color::from_rgb(30, 30, 30));
        tabs.set_label_color(Color::White);
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
        let mut group = Group::default_fill().with_label(label);
        group.set_color(Color::from_rgb(30, 30, 30));
        group.set_label_color(Color::White);

        group.begin();
        let table = ResultTableWidget::new();
        let widget = table.get_widget();
        group.resizable(&widget);
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
        Self::new()
    }
}
