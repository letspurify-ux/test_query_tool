use fltk::{
    button::{Button, CheckButton},
    enums::{CallbackTrigger, Color, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    input::Input,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

use crate::utils::feature_catalog::{build_catalog_text_filtered, load_feature_catalog};

pub struct FeatureCatalogDialog;

impl FeatureCatalogDialog {
    pub fn show() {
        let mut dialog = Window::default()
            .with_size(900, 700)
            .with_label("Feature Catalog");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut main_flex = Flex::default().with_pos(10, 10).with_size(880, 680);
        main_flex.set_type(FlexType::Column);
        main_flex.set_spacing(10);

        let mut controls_row = Flex::default();
        controls_row.set_type(FlexType::Row);
        controls_row.set_spacing(10);

        let mut filter_label = Frame::default().with_label("Filter:");
        filter_label.set_label_color(Color::White);

        let mut filter_input = Input::default();
        filter_input.set_color(Color::from_rgb(60, 60, 63));
        filter_input.set_text_color(Color::White);
        filter_input.set_trigger(CallbackTrigger::Changed);

        let mut show_implemented = CheckButton::default().with_label("Implemented");
        show_implemented.set_label_color(Color::White);
        show_implemented.set_value(true);

        let mut show_planned = CheckButton::default().with_label("Planned");
        show_planned.set_label_color(Color::White);
        show_planned.set_value(true);

        let mut reload_btn = Button::default().with_size(90, 30).with_label("Reload");
        reload_btn.set_color(Color::from_rgb(0, 122, 204));
        reload_btn.set_label_color(Color::White);
        reload_btn.set_frame(FrameType::FlatBox);

        let mut clear_btn = Button::default().with_size(80, 30).with_label("Clear");
        clear_btn.set_color(Color::from_rgb(100, 100, 100));
        clear_btn.set_label_color(Color::White);
        clear_btn.set_frame(FrameType::FlatBox);

        controls_row.fixed(&filter_label, 50);
        controls_row.fixed(&show_implemented, 120);
        controls_row.fixed(&show_planned, 90);
        controls_row.fixed(&reload_btn, 90);
        controls_row.fixed(&clear_btn, 80);
        controls_row.end();
        main_flex.fixed(&controls_row, 30);

        let catalog = std::rc::Rc::new(std::cell::RefCell::new(load_feature_catalog()));
        let mut display_buffer = TextBuffer::default();
        display_buffer.set_text(&build_catalog_text_filtered(
            &catalog.borrow(),
            "",
            true,
            true,
        ));

        let mut display = TextDisplay::default();
        display.set_buffer(display_buffer.clone());
        display.set_color(Color::from_rgb(30, 30, 30));
        display.set_text_color(Color::from_rgb(220, 220, 220));
        display.set_text_font(Font::Courier);
        display.set_text_size(12);

        let mut button_row = Flex::default();
        button_row.set_type(FlexType::Row);

        let _spacer = fltk::frame::Frame::default();
        let mut close_btn = Button::default().with_size(80, 30).with_label("Close");
        close_btn.set_color(Color::from_rgb(100, 100, 100));
        close_btn.set_label_color(Color::White);
        close_btn.set_frame(FrameType::FlatBox);

        button_row.fixed(&close_btn, 80);
        button_row.end();
        main_flex.fixed(&button_row, 30);

        main_flex.end();

        dialog.end();

        let update_display = {
            let filter_input = filter_input.clone();
            let show_implemented = show_implemented.clone();
            let show_planned = show_planned.clone();
            let mut display_buffer = display_buffer.clone();
            let catalog = catalog.clone();

            move || {
                let text = build_catalog_text_filtered(
                    &catalog.borrow(),
                    &filter_input.value(),
                    show_implemented.value(),
                    show_planned.value(),
                );
                display_buffer.set_text(&text);
            }
        };

        let update_display = std::rc::Rc::new(std::cell::RefCell::new(update_display));

        let update_for_filter = update_display.clone();
        filter_input.set_callback(move |_| {
            (update_for_filter.borrow_mut())();
        });

        let update_for_impl = update_display.clone();
        show_implemented.set_callback(move |_| {
            (update_for_impl.borrow_mut())();
        });

        let update_for_planned = update_display.clone();
        show_planned.set_callback(move |_| {
            (update_for_planned.borrow_mut())();
        });

        let update_for_clear = update_display.clone();
        let mut filter_for_clear = filter_input.clone();
        clear_btn.set_callback(move |_| {
            filter_for_clear.set_value("");
            (update_for_clear.borrow_mut())();
        });

        let update_for_reload = update_display.clone();
        let catalog_for_reload = catalog.clone();
        reload_btn.set_callback(move |_| {
            *catalog_for_reload.borrow_mut() = load_feature_catalog();
            (update_for_reload.borrow_mut())();
        });

        let mut dialog_clone = dialog.clone();
        close_btn.set_callback(move |_| {
            dialog_clone.hide();
        });

        dialog.show();

        while dialog.shown() {
            fltk::app::wait();
        }
    }
}
