use fltk::{
    button::Button,
    enums::{Color, Font, FrameType},
    group::{Flex, FlexType},
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

use crate::utils::feature_catalog::build_catalog_text;

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

        let mut display_buffer = TextBuffer::default();
        display_buffer.set_text(&build_catalog_text());

        let mut display = TextDisplay::default();
        display.set_buffer(display_buffer.clone());
        display.set_color(Color::from_rgb(30, 30, 30));
        display.set_text_color(Color::from_rgb(220, 220, 220));
        display.set_text_font(Font::Courier);
        display.set_text_size(12);

        main_flex.end();

        let mut button_row = Flex::default().with_pos(10, 660).with_size(880, 30);
        button_row.set_type(FlexType::Row);

        let _spacer = fltk::frame::Frame::default();
        let mut close_btn = Button::default().with_size(80, 30).with_label("Close");
        close_btn.set_color(Color::from_rgb(100, 100, 100));
        close_btn.set_label_color(Color::White);
        close_btn.set_frame(FrameType::FlatBox);

        button_row.fixed(&close_btn, 80);
        button_row.end();

        dialog.end();

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
