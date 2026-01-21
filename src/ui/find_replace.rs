use fltk::{
    button::{Button, CheckButton},
    enums::{Color, FrameType},
    group::Flex,
    input::Input,
    prelude::*,
    text::{TextBuffer, TextEditor},
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Find/Replace dialog
pub struct FindReplaceDialog;

impl FindReplaceDialog {
    /// Show find dialog
    pub fn show_find(editor: &mut TextEditor, buffer: &mut TextBuffer) {
        Self::show_dialog(editor, buffer, false);
    }

    /// Show find and replace dialog
    pub fn show_replace(editor: &mut TextEditor, buffer: &mut TextBuffer) {
        Self::show_dialog(editor, buffer, true);
    }

    fn show_dialog(editor: &mut TextEditor, buffer: &mut TextBuffer, show_replace: bool) {
        enum DialogMessage {
            FindNext {
                search_text: String,
                case_sensitive: bool,
            },
            Replace {
                search_text: String,
                replace_text: String,
                case_sensitive: bool,
            },
            ReplaceAll {
                search_text: String,
                replace_text: String,
                case_sensitive: bool,
            },
            Close,
        }

        let title = if show_replace {
            "Find and Replace"
        } else {
            "Find"
        };
        let height = if show_replace { 180 } else { 130 };

        let mut dialog = Window::default()
            .with_size(450, height)
            .with_label(title);
        dialog.set_color(Color::from_rgb(45, 45, 48));

        let mut main_flex = Flex::default()
            .with_pos(10, 10)
            .with_size(430, height - 20);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_spacing(8);

        // Find input row
        let mut find_flex = Flex::default();
        find_flex.set_type(fltk::group::FlexType::Row);
        let mut find_label = fltk::frame::Frame::default().with_label("Find:");
        find_label.set_label_color(Color::White);
        find_flex.fixed(&find_label, 80);
        let mut find_input = Input::default();
        find_input.set_color(Color::from_rgb(60, 60, 63));
        find_input.set_text_color(Color::White);
        find_flex.end();
        main_flex.fixed(&find_flex, 30);

        // Replace input row (if show_replace)
        let replace_input = if show_replace {
            let mut replace_flex = Flex::default();
            replace_flex.set_type(fltk::group::FlexType::Row);
            let mut replace_label = fltk::frame::Frame::default().with_label("Replace:");
            replace_label.set_label_color(Color::White);
            replace_flex.fixed(&replace_label, 80);
            let mut input = Input::default();
            input.set_color(Color::from_rgb(60, 60, 63));
            input.set_text_color(Color::White);
            replace_flex.end();
            main_flex.fixed(&replace_flex, 30);
            Some(input)
        } else {
            None
        };

        // Options row
        let mut options_flex = Flex::default();
        options_flex.set_type(fltk::group::FlexType::Row);
        let mut case_check = CheckButton::default().with_label("Case sensitive");
        case_check.set_label_color(Color::White);
        let mut whole_word_check = CheckButton::default().with_label("Whole word");
        whole_word_check.set_label_color(Color::White);
        options_flex.end();
        main_flex.fixed(&options_flex, 25);

        // Buttons row
        let mut button_flex = Flex::default();
        button_flex.set_type(fltk::group::FlexType::Row);
        button_flex.set_spacing(10);

        let _spacer = fltk::frame::Frame::default();

        let mut find_next_btn = Button::default()
            .with_size(90, 30)
            .with_label("Find Next");
        find_next_btn.set_color(Color::from_rgb(0, 122, 204));
        find_next_btn.set_label_color(Color::White);
        find_next_btn.set_frame(FrameType::FlatBox);

        let replace_btn = if show_replace {
            let mut btn = Button::default()
                .with_size(90, 30)
                .with_label("Replace");
            btn.set_color(Color::from_rgb(104, 33, 122));
            btn.set_label_color(Color::White);
            btn.set_frame(FrameType::FlatBox);
            button_flex.fixed(&btn, 90);
            Some(btn)
        } else {
            None
        };

        let replace_all_btn = if show_replace {
            let mut btn = Button::default()
                .with_size(90, 30)
                .with_label("Replace All");
            btn.set_color(Color::from_rgb(104, 33, 122));
            btn.set_label_color(Color::White);
            btn.set_frame(FrameType::FlatBox);
            button_flex.fixed(&btn, 90);
            Some(btn)
        } else {
            None
        };

        let mut close_btn = Button::default()
            .with_size(70, 30)
            .with_label("Close");
        close_btn.set_color(Color::from_rgb(100, 100, 100));
        close_btn.set_label_color(Color::White);
        close_btn.set_frame(FrameType::FlatBox);

        button_flex.fixed(&find_next_btn, 90);
        button_flex.fixed(&close_btn, 70);
        button_flex.end();
        main_flex.fixed(&button_flex, 35);

        main_flex.end();
        dialog.end();

        // State for search
        let search_pos = Rc::new(RefCell::new(0i32));

        let (sender, receiver) = fltk::app::channel::<DialogMessage>();

        // Find Next callback
        let sender_for_find = sender.clone();
        let find_input_clone = find_input.clone();
        let case_check_clone = case_check.clone();
        find_next_btn.set_callback(move |_| {
            let search_text = find_input_clone.value();
            if search_text.is_empty() {
                return;
            }

            let _ = sender_for_find.send(DialogMessage::FindNext {
                search_text,
                case_sensitive: case_check_clone.value(),
            });
        });

        // Replace callback
        if let Some(mut replace_btn) = replace_btn {
            let sender_for_replace = sender.clone();
            let find_input_clone = find_input.clone();
            let replace_input_clone = match replace_input.clone() {
                Some(input) => input,
                None => {
                    eprintln!("Replace input not available for replace action.");
                    return;
                }
            };
            let case_check_clone = case_check.clone();

            replace_btn.set_callback(move |_| {
                let search_text = find_input_clone.value();
                let replace_text = replace_input_clone.value();

                if search_text.is_empty() {
                    return;
                }

                let _ = sender_for_replace.send(DialogMessage::Replace {
                    search_text,
                    replace_text,
                    case_sensitive: case_check_clone.value(),
                });
            });
        }

        // Replace All callback
        if let Some(mut replace_all_btn) = replace_all_btn {
            let sender_for_replace_all = sender.clone();
            let find_input_clone = find_input.clone();
            let replace_input_clone = match replace_input.clone() {
                Some(input) => input,
                None => {
                    eprintln!("Replace input not available for replace-all action.");
                    return;
                }
            };
            let case_check_clone = case_check.clone();

            replace_all_btn.set_callback(move |_| {
                let search_text = find_input_clone.value();
                let replace_text = replace_input_clone.value();

                if search_text.is_empty() {
                    return;
                }

                let _ = sender_for_replace_all.send(DialogMessage::ReplaceAll {
                    search_text,
                    replace_text,
                    case_sensitive: case_check_clone.value(),
                });
            });
        }

        // Close callback
        let sender_for_close = sender.clone();
        close_btn.set_callback(move |_| {
            let _ = sender_for_close.send(DialogMessage::Close);
        });

        dialog.show();

        let mut buffer = buffer.clone();
        let mut editor = editor.clone();
        while dialog.shown() {
            fltk::app::wait();
            while let Some(message) = receiver.recv() {
                match message {
                    DialogMessage::FindNext {
                        search_text,
                        case_sensitive,
                    } => {
                        let text = buffer.text();
                        let start_pos = *search_pos.borrow();

                        let found_pos = if case_sensitive {
                            text[start_pos as usize..].find(&search_text)
                        } else {
                            text[start_pos as usize..]
                                .to_lowercase()
                                .find(&search_text.to_lowercase())
                        };

                        if let Some(pos) = found_pos {
                            let absolute_pos = start_pos as usize + pos;
                            buffer.select(
                                absolute_pos as i32,
                                (absolute_pos + search_text.len()) as i32,
                            );
                            editor.set_insert_position((absolute_pos + search_text.len()) as i32);
                            editor.show_insert_position();
                            *search_pos.borrow_mut() = (absolute_pos + 1) as i32;
                        } else if start_pos > 0 {
                            *search_pos.borrow_mut() = 0;
                            fltk::dialog::message_default(
                                "Reached end, searching from beginning...",
                            );
                        } else {
                            fltk::dialog::message_default("Text not found");
                        }
                    }
                    DialogMessage::Replace {
                        search_text,
                        replace_text,
                        case_sensitive,
                    } => {
                        if let Some((start, end)) = buffer.selection_position() {
                            let selected = buffer.text_range(start, end).unwrap_or_default();
                            let matches = if case_sensitive {
                                selected == search_text
                            } else {
                                selected.to_lowercase() == search_text.to_lowercase()
                            };

                            if matches {
                                buffer.remove(start, end);
                                buffer.insert(start, &replace_text);
                                editor.set_insert_position(start + replace_text.len() as i32);
                            }
                        }
                    }
                    DialogMessage::ReplaceAll {
                        search_text,
                        replace_text,
                        case_sensitive,
                    } => {
                        let text = buffer.text();
                        let new_text = if case_sensitive {
                            text.replace(&search_text, &replace_text)
                        } else {
                            let mut result = text.clone();
                            let lower_text = text.to_lowercase();
                            let lower_search = search_text.to_lowercase();
                            let mut offset: i32 = 0;

                            for (pos, _) in lower_text.match_indices(&lower_search) {
                                let actual_pos = (pos as i32 + offset) as usize;
                                result = format!(
                                    "{}{}{}",
                                    &result[..actual_pos],
                                    replace_text,
                                    &result[actual_pos + search_text.len()..]
                                );
                                offset += replace_text.len() as i32 - search_text.len() as i32;
                            }
                            result
                        };

                        let count = if case_sensitive {
                            text.matches(&search_text).count()
                        } else {
                            text.to_lowercase()
                                .matches(&search_text.to_lowercase())
                                .count()
                        };

                        buffer.set_text(&new_text);
                        fltk::dialog::message_default(&format!(
                            "Replaced {} occurrences",
                            count
                        ));
                    }
                    DialogMessage::Close => {
                        dialog.hide();
                    }
                }
            }
        }
    }

    /// Find next occurrence (for F3 shortcut)
    #[allow(dead_code)]
    pub fn find_next(
        editor: &mut TextEditor,
        buffer: &mut TextBuffer,
        search_text: &str,
        case_sensitive: bool,
    ) -> bool {
        if search_text.is_empty() {
            return false;
        }

        let current_pos = editor.insert_position();
        let text = buffer.text();

        let found_pos = if case_sensitive {
            text[current_pos as usize..].find(search_text)
        } else {
            text[current_pos as usize..]
                .to_lowercase()
                .find(&search_text.to_lowercase())
        };

        if let Some(pos) = found_pos {
            let absolute_pos = current_pos as usize + pos;
            buffer.select(
                absolute_pos as i32,
                (absolute_pos + search_text.len()) as i32,
            );
            editor.set_insert_position((absolute_pos + search_text.len()) as i32);
            editor.show_insert_position();
            true
        } else {
            // Try from beginning
            let found_pos = if case_sensitive {
                text.find(search_text)
            } else {
                text.to_lowercase().find(&search_text.to_lowercase())
            };

            if let Some(pos) = found_pos {
                buffer.select(pos as i32, (pos + search_text.len()) as i32);
                editor.set_insert_position((pos + search_text.len()) as i32);
                editor.show_insert_position();
                true
            } else {
                false
            }
        }
    }
}
