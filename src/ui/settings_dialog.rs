use fltk::{
    app,
    button::Button,
    enums::FrameType,
    frame::Frame,
    group::{Flex, FlexType},
    input::IntInput,
    menu::Choice,
    prelude::*,
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::ui::{center_on_main, font_choice_index, font_choice_labels, theme};
use crate::utils::AppConfig;

pub struct FontSettings {
    pub editor_font: String,
    pub editor_size: u32,
    pub result_font: String,
    pub result_size: u32,
}

fn validate_size(label: &str, value: &str) -> Option<u32> {
    match value.trim().parse::<u32>() {
        Ok(size) if (8..=48).contains(&size) => Some(size),
        _ => {
            fltk::dialog::alert_default(&format!(
                "{} size must be a number between 8 and 48.",
                label
            ));
            None
        }
    }
}

pub fn show_settings_dialog(config: &AppConfig) -> Option<FontSettings> {
    let current_group = fltk::group::Group::try_current();
    fltk::group::Group::set_current(None::<&fltk::group::Group>);

    let width = 380;
    let height = 240;
    let mut dialog = Window::default()
        .with_size(width, height)
        .with_label("Settings");
    center_on_main(&mut dialog);
    dialog.set_color(theme::panel_raised());
    dialog.make_modal(true);

    let mut main_flex = Flex::default()
        .with_pos(10, 10)
        .with_size(width - 20, height - 20);
    main_flex.set_type(FlexType::Column);
    main_flex.set_margin(6);
    main_flex.set_spacing(10);

    let mut editor_row = Flex::default().with_size(0, 30);
    editor_row.set_type(FlexType::Row);
    editor_row.set_spacing(8);
    let mut editor_label = Frame::default().with_label("Editor font");
    editor_label.set_label_color(theme::text_primary());
    let mut editor_font_choice = Choice::default();
    editor_font_choice.add_choice(&font_choice_labels());
    editor_font_choice.set_value(font_choice_index(&config.editor_font));
    editor_font_choice.set_color(theme::input_bg());
    editor_font_choice.set_text_color(theme::text_primary());
    let mut editor_size_input = IntInput::default();
    editor_size_input.set_value(&config.editor_font_size.to_string());
    editor_size_input.set_color(theme::input_bg());
    editor_size_input.set_text_color(theme::text_primary());
    editor_row.fixed(&editor_label, 110);
    editor_row.fixed(&editor_size_input, 60);
    editor_row.end();

    let mut result_row = Flex::default().with_size(0, 30);
    result_row.set_type(FlexType::Row);
    result_row.set_spacing(8);
    let mut result_label = Frame::default().with_label("Results font");
    result_label.set_label_color(theme::text_primary());
    let mut result_font_choice = Choice::default();
    result_font_choice.add_choice(&font_choice_labels());
    result_font_choice.set_value(font_choice_index(&config.result_font));
    result_font_choice.set_color(theme::input_bg());
    result_font_choice.set_text_color(theme::text_primary());
    let mut result_size_input = IntInput::default();
    result_size_input.set_value(&config.result_font_size.to_string());
    result_size_input.set_color(theme::input_bg());
    result_size_input.set_text_color(theme::text_primary());
    result_row.fixed(&result_label, 110);
    result_row.fixed(&result_size_input, 60);
    result_row.end();

    let mut button_row = Flex::default().with_size(0, 34);
    button_row.set_type(FlexType::Row);
    button_row.set_spacing(10);
    let spacer = Frame::default();
    button_row.resizable(&spacer);
    let mut cancel_btn = Button::default().with_size(90, 28).with_label("Cancel");
    cancel_btn.set_color(theme::button_secondary());
    cancel_btn.set_label_color(theme::text_primary());
    cancel_btn.set_frame(FrameType::RFlatBox);
    let mut ok_btn = Button::default().with_size(90, 28).with_label("Save");
    ok_btn.set_color(theme::button_primary());
    ok_btn.set_label_color(theme::text_primary());
    ok_btn.set_frame(FrameType::RFlatBox);
    button_row.fixed(&cancel_btn, 90);
    button_row.fixed(&ok_btn, 90);
    button_row.end();

    main_flex.end();
    dialog.end();
    dialog.show();
    fltk::group::Group::set_current(current_group.as_ref());

    let result = Rc::new(RefCell::new(None::<FontSettings>));
    let result_for_ok = result.clone();
    let mut dialog_handle = dialog.clone();
    let mut editor_font_choice_ok = editor_font_choice.clone();
    let mut result_font_choice_ok = result_font_choice.clone();
    let mut editor_size_input_ok = editor_size_input.clone();
    let mut result_size_input_ok = result_size_input.clone();
    ok_btn.set_callback(move |_| {
        let editor_size = match validate_size("Editor", &editor_size_input_ok.value()) {
            Some(size) => size,
            None => return,
        };
        let result_size = match validate_size("Results", &result_size_input_ok.value()) {
            Some(size) => size,
            None => return,
        };
        let editor_font = editor_font_choice_ok
            .text(editor_font_choice_ok.value())
            .unwrap_or_else(|| "Courier".to_string());
        let result_font = result_font_choice_ok
            .text(result_font_choice_ok.value())
            .unwrap_or_else(|| "Helvetica".to_string());
        *result_for_ok.borrow_mut() = Some(FontSettings {
            editor_font,
            editor_size,
            result_font,
            result_size,
        });
        dialog_handle.hide();
        app::awake();
    });

    let mut dialog_handle = dialog.clone();
    cancel_btn.set_callback(move |_| {
        dialog_handle.hide();
        app::awake();
    });

    while dialog.shown() {
        app::wait();
    }

    result.borrow_mut().take()
}
