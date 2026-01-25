use fltk::{
    button::{Button, CheckButton},
    enums::{CallbackTrigger, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    input::Input,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};
use std::sync::mpsc;

use crate::utils::feature_catalog::{build_catalog_text_filtered, load_feature_catalog};
use crate::ui::theme;

pub struct FeatureCatalogDialog;

impl FeatureCatalogDialog {
    pub fn show() {
        enum DialogMessage {
            UpdateDisplay,
            ClearFilter,
            ReloadCatalog,
            Close,
        }

        let mut dialog = Window::default()
            .with_size(900, 700)
            .with_label("Feature Catalog")
            .center_screen();
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);

        let mut main_flex = Flex::default().with_pos(10, 10).with_size(880, 680);
        main_flex.set_type(FlexType::Column);
        main_flex.set_spacing(5);

        let mut controls_row = Flex::default();
        controls_row.set_type(FlexType::Row);
        controls_row.set_spacing(5);

        let mut filter_label = Frame::default().with_label("Filter:");
        filter_label.set_label_color(theme::text_primary());

        let mut filter_input = Input::default();
        filter_input.set_color(theme::input_bg());
        filter_input.set_text_color(theme::text_primary());
        filter_input.set_trigger(CallbackTrigger::Changed);

        let mut show_implemented = CheckButton::default().with_label("Implemented");
        show_implemented.set_label_color(theme::text_secondary());
        show_implemented.set_value(true);

        let mut show_planned = CheckButton::default().with_label("Planned");
        show_planned.set_label_color(theme::text_secondary());
        show_planned.set_value(true);

        let mut reload_btn = Button::default().with_size(90, 20).with_label("Reload");
        reload_btn.set_color(theme::button_secondary());
        reload_btn.set_label_color(theme::text_primary());
        reload_btn.set_frame(FrameType::RFlatBox);

        let mut clear_btn = Button::default().with_size(80, 20).with_label("Clear");
        clear_btn.set_color(theme::button_subtle());
        clear_btn.set_label_color(theme::text_primary());
        clear_btn.set_frame(FrameType::RFlatBox);

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
        display.set_color(theme::editor_bg());
        display.set_text_color(theme::text_primary());
        display.set_text_font(Font::Courier);
        display.set_text_size(12);

        let mut button_row = Flex::default();
        button_row.set_type(FlexType::Row);

        let _spacer = fltk::frame::Frame::default();
        let mut close_btn = Button::default().with_size(80, 20).with_label("Close");
        close_btn.set_color(theme::button_subtle());
        close_btn.set_label_color(theme::text_primary());
        close_btn.set_frame(FrameType::RFlatBox);

        button_row.fixed(&close_btn, 80);
        button_row.end();
        main_flex.fixed(&button_row, 30);

        main_flex.end();

        dialog.end();

        let (sender, receiver) = mpsc::channel::<DialogMessage>();

        let update_for_filter = sender.clone();
        filter_input.set_callback(move |_| {
            let _ = update_for_filter.send(DialogMessage::UpdateDisplay);
        });

        let update_for_impl = sender.clone();
        show_implemented.set_callback(move |_| {
            let _ = update_for_impl.send(DialogMessage::UpdateDisplay);
        });

        let update_for_planned = sender.clone();
        show_planned.set_callback(move |_| {
            let _ = update_for_planned.send(DialogMessage::UpdateDisplay);
        });

        let update_for_clear = sender.clone();
        clear_btn.set_callback(move |_| {
            let _ = update_for_clear.send(DialogMessage::ClearFilter);
        });

        let update_for_reload = sender.clone();
        reload_btn.set_callback(move |_| {
            let _ = update_for_reload.send(DialogMessage::ReloadCatalog);
        });

        let sender_for_close = sender.clone();
        close_btn.set_callback(move |_| {
            let _ = sender_for_close.send(DialogMessage::Close);
        });

        dialog.show();

        let mut filter_input = filter_input.clone();
        let show_implemented = show_implemented.clone();
        let show_planned = show_planned.clone();
        let mut display_buffer = display_buffer.clone();
        while dialog.shown() {
            fltk::app::wait();
            while let Ok(message) = receiver.try_recv() {
                match message {
                    DialogMessage::UpdateDisplay => {
                        let text = build_catalog_text_filtered(
                            &catalog.borrow(),
                            &filter_input.value(),
                            show_implemented.value(),
                            show_planned.value(),
                        );
                        display_buffer.set_text(&text);
                    }
                    DialogMessage::ClearFilter => {
                        filter_input.set_value("");
                        let text = build_catalog_text_filtered(
                            &catalog.borrow(),
                            "",
                            show_implemented.value(),
                            show_planned.value(),
                        );
                        display_buffer.set_text(&text);
                    }
                    DialogMessage::ReloadCatalog => {
                        *catalog.borrow_mut() = load_feature_catalog();
                        let text = build_catalog_text_filtered(
                            &catalog.borrow(),
                            &filter_input.value(),
                            show_implemented.value(),
                            show_planned.value(),
                        );
                        display_buffer.set_text(&text);
                    }
                    DialogMessage::Close => {
                        dialog.hide();
                    }
                }
            }
        }
    }
}
