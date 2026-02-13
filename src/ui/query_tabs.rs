use fltk::{
    app, draw,
    enums::Event,
    group::{Group, Tabs, TabsOverflow},
    prelude::*,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::ui::constants;
use crate::ui::theme;

type TabSelectCallback = Box<dyn FnMut(usize)>;
type TabCloseCallback = Box<dyn FnMut(usize)>;

const TAB_LABEL_SUFFIX: &str = "  x";
const HEADER_LEFT_PADDING: i32 = 6;
const TAB_LABEL_HORIZONTAL_PADDING: i32 = 26;
const MIN_TAB_WIDTH: i32 = 48;
const CLOSE_HIT_WIDTH: i32 = 16;
const CLOSE_HIT_RIGHT_PADDING: i32 = 4;

#[derive(Clone)]
pub struct QueryTabsWidget {
    tabs: Tabs,
    groups: Rc<RefCell<Vec<Group>>>,
    on_select: Rc<RefCell<Option<TabSelectCallback>>>,
    on_close: Rc<RefCell<Option<TabCloseCallback>>>,
}

impl QueryTabsWidget {
    fn content_bounds(tabs: &Tabs) -> (i32, i32, i32, i32) {
        let x = tabs.x();
        let y = tabs.y() + constants::TAB_HEADER_HEIGHT;
        let w = tabs.w().max(1);
        let h = (tabs.h() - constants::TAB_HEADER_HEIGHT).max(1);
        (x, y, w, h)
    }

    fn layout_children(tabs: &Tabs) {
        let (x, y, w, h) = Self::content_bounds(tabs);
        for child in tabs.clone().into_iter() {
            if let Some(mut group) = child.as_group() {
                group.resize(x, y, w, h);
            }
        }
    }

    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        let mut tabs = Tabs::new(x, y, w, h, None);
        tabs.end();
        tabs.set_color(theme::panel_bg());
        tabs.set_selection_color(theme::selection_strong());
        tabs.set_label_color(theme::text_secondary());
        tabs.handle_overflow(TabsOverflow::Compress);

        let groups = Rc::new(RefCell::new(Vec::<Group>::new()));
        let on_select = Rc::new(RefCell::new(None::<TabSelectCallback>));
        let on_close = Rc::new(RefCell::new(None::<TabCloseCallback>));

        let groups_for_cb = groups.clone();
        let on_select_for_cb = on_select.clone();
        tabs.set_callback(move |tabs| {
            let Some(selected) = tabs.value() else {
                return;
            };
            let selected_ptr = selected.as_widget_ptr();
            let selected_idx = groups_for_cb
                .borrow()
                .iter()
                .position(|group| group.as_widget_ptr() == selected_ptr);
            if let Some(index) = selected_idx {
                if let Some(callback) = on_select_for_cb.borrow_mut().as_mut() {
                    callback(index);
                }
            }
        });

        let groups_for_handle = groups.clone();
        let on_close_for_handle = on_close.clone();
        let tabs_for_handle = tabs.clone();
        tabs.handle(move |_, ev| {
            if !matches!(ev, Event::Push) {
                return false;
            }
            let ex = app::event_x();
            let ey = app::event_y();
            let hit = {
                let groups = groups_for_handle.borrow();
                Self::tab_at_point(&tabs_for_handle, &groups, ex, ey)
                    .map(|(index, x, w)| (index, Self::is_close_hit(ex, x, w)))
            };
            let Some((index, close_hit)) = hit else {
                return false;
            };
            if close_hit {
                if let Some(cb) = on_close_for_handle.borrow_mut().as_mut() {
                    cb(index);
                }
                return true;
            }
            false
        });
        tabs.resize_callback(move |t, _, _, _, _| {
            Self::layout_children(t);
        });

        Self {
            tabs,
            groups,
            on_select,
            on_close,
        }
    }

    pub fn get_widget(&self) -> Tabs {
        self.tabs.clone()
    }

    pub fn set_on_select<F>(&mut self, callback: F)
    where
        F: FnMut(usize) + 'static,
    {
        *self.on_select.borrow_mut() = Some(Box::new(callback));
    }

    pub fn set_on_close<F>(&mut self, callback: F)
    where
        F: FnMut(usize) + 'static,
    {
        *self.on_close.borrow_mut() = Some(Box::new(callback));
    }

    pub fn add_tab(&mut self, label: &str) -> usize {
        self.tabs.begin();
        let (x, y, w, h) = Self::content_bounds(&self.tabs);
        let mut group = Group::new(x, y, w, h, None).with_label(&Self::display_label(label));
        group.set_color(theme::panel_bg());
        group.set_label_color(theme::text_secondary());
        group.end();
        self.tabs.end();

        let mut groups = self.groups.borrow_mut();
        groups.push(group.clone());
        let index = groups.len().saturating_sub(1);
        let _ = self.tabs.set_value(&group);
        Self::layout_children(&self.tabs);
        self.tabs.redraw();
        index
    }

    pub fn select(&mut self, index: usize) {
        if let Some(group) = self.groups.borrow().get(index) {
            let _ = self.tabs.set_value(group);
            self.tabs.redraw();
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.tabs.value()?;
        let selected_ptr = selected.as_widget_ptr();
        self.groups
            .borrow()
            .iter()
            .position(|group| group.as_widget_ptr() == selected_ptr)
    }

    pub fn set_tab_label(&mut self, index: usize, label: &str) {
        if let Some(group) = self.groups.borrow().get(index) {
            let mut group = group.clone();
            group.set_label(&Self::display_label(label));
            self.tabs.redraw();
        }
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        let group = {
            let mut groups = self.groups.borrow_mut();
            if index >= groups.len() {
                return false;
            }
            groups.remove(index)
        };

        if self.tabs.find(&group) >= 0 {
            self.tabs.remove(&group);
        }
        fltk::group::Group::delete(group);
        Self::layout_children(&self.tabs);
        self.tabs.redraw();
        true
    }

    fn display_label(label: &str) -> String {
        format!("{label}{TAB_LABEL_SUFFIX}")
    }

    fn tab_at_point(tabs: &Tabs, groups: &[Group], ex: i32, ey: i32) -> Option<(usize, i32, i32)> {
        if groups.is_empty() {
            return None;
        }
        let (client_x, client_y, _, _) = tabs.client_area();
        let header_y = tabs.y();
        let header_h = (client_y - header_y).max(constants::TAB_HEADER_HEIGHT);
        if ey < header_y || ey > header_y + header_h {
            return None;
        }

        let widths = Self::tab_header_widths(tabs, groups);
        let mut cursor_x = tabs.x() + HEADER_LEFT_PADDING;
        for (index, width) in widths.into_iter().enumerate() {
            let right = cursor_x + width;
            if ex >= cursor_x && ex <= right {
                return Some((index, cursor_x, width));
            }
            cursor_x = right;
        }
        let _ = client_x;
        None
    }

    fn tab_header_widths(tabs: &Tabs, groups: &[Group]) -> Vec<i32> {
        if groups.is_empty() {
            return Vec::new();
        }
        draw::set_font(tabs.label_font(), tabs.label_size());
        let mut widths: Vec<i32> = groups
            .iter()
            .map(|group| {
                let label = group.label();
                let measured = draw::measure(&label, false).0;
                (measured + TAB_LABEL_HORIZONTAL_PADDING).max(MIN_TAB_WIDTH)
            })
            .collect();

        let total: i32 = widths.iter().sum();
        let available = (tabs.w() - HEADER_LEFT_PADDING * 2).max(MIN_TAB_WIDTH);
        if total <= available {
            return widths;
        }

        let count = widths.len() as i32;
        let equal = (available / count.max(1)).max(MIN_TAB_WIDTH);
        widths.fill(equal);
        widths
    }

    fn is_close_hit(ex: i32, tab_x: i32, tab_w: i32) -> bool {
        let right = tab_x + tab_w - CLOSE_HIT_RIGHT_PADDING;
        let left = right - CLOSE_HIT_WIDTH;
        ex >= left && ex <= right
    }
}

impl Default for QueryTabsWidget {
    fn default() -> Self {
        Self::new(0, 0, 100, 100)
    }
}
