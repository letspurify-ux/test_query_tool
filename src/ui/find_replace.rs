use fltk::{
    button::{Button, CheckButton},
    enums::FrameType,
    group::Flex,
    input::Input,
    prelude::*,
    text::{TextBuffer, TextEditor},
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use crate::ui::theme;

/// Find/Replace dialog
pub struct FindReplaceDialog;

impl FindReplaceDialog {
    pub fn show_find_with_registry(editor: &mut TextEditor, buffer: &mut TextBuffer, popups: Rc<RefCell<Vec<Window>>>) {
        Self::show_dialog(editor, buffer, false, popups);
    }

    /// Show find and replace dialog
    pub fn show_replace_with_registry(editor: &mut TextEditor, buffer: &mut TextBuffer, popups: Rc<RefCell<Vec<Window>>>) {
        Self::show_dialog(editor, buffer, true, popups);
    }

    fn show_dialog(editor: &mut TextEditor, buffer: &mut TextBuffer, show_replace: bool, popups: Rc<RefCell<Vec<Window>>>) {
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

        fltk::group::Group::set_current(None::<&fltk::group::Group>);
        
        let mut dialog = Window::default()
            .with_size(450, height)
            .with_label(title)
            .center_screen();
        dialog.set_color(theme::panel_raised());

        let mut main_flex = Flex::default()
            .with_pos(10, 10)
            .with_size(430, height - 20);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_spacing(5);

        // Find input row
        let mut find_flex = Flex::default();
        find_flex.set_type(fltk::group::FlexType::Row);
        let mut find_label = fltk::frame::Frame::default().with_label("Find:");
        find_label.set_label_color(theme::text_primary());
        find_flex.fixed(&find_label, 80);
        let mut find_input = Input::default();
        find_input.set_color(theme::input_bg());
        find_input.set_text_color(theme::text_primary());
        find_flex.end();
        main_flex.fixed(&find_flex, 30);

        // Replace input row (if show_replace)
        let replace_input = if show_replace {
            let mut replace_flex = Flex::default();
            replace_flex.set_type(fltk::group::FlexType::Row);
            let mut replace_label = fltk::frame::Frame::default().with_label("Replace:");
            replace_label.set_label_color(theme::text_primary());
            replace_flex.fixed(&replace_label, 80);
            let mut input = Input::default();
            input.set_color(theme::input_bg());
            input.set_text_color(theme::text_primary());
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
        case_check.set_label_color(theme::text_secondary());
        let mut whole_word_check = CheckButton::default().with_label("Whole word");
        whole_word_check.set_label_color(theme::text_secondary());
        options_flex.end();
        main_flex.fixed(&options_flex, 25);

        // Buttons row
        let mut button_flex = Flex::default();
        button_flex.set_type(fltk::group::FlexType::Row);
        button_flex.set_spacing(5);

        let _spacer = fltk::frame::Frame::default();

        let mut find_next_btn = Button::default()
            .with_size(90, 20)
            .with_label("Find Next");
        find_next_btn.set_color(theme::button_primary());
        find_next_btn.set_label_color(theme::text_primary());
        find_next_btn.set_frame(FrameType::RFlatBox);

        let replace_btn = if show_replace {
            let mut btn = Button::default()
                .with_size(90, 20)
                .with_label("Replace");
            btn.set_color(theme::button_secondary());
            btn.set_label_color(theme::text_primary());
            btn.set_frame(FrameType::RFlatBox);
            button_flex.fixed(&btn, 90);
            Some(btn)
        } else {
            None
        };

        let replace_all_btn = if show_replace {
            let mut btn = Button::default()
                .with_size(90, 20)
                .with_label("Replace All");
            btn.set_color(theme::button_secondary());
            btn.set_label_color(theme::text_primary());
            btn.set_frame(FrameType::RFlatBox);
            button_flex.fixed(&btn, 90);
            Some(btn)
        } else {
            None
        };

        let mut close_btn = Button::default()
            .with_size(70, 20)
            .with_label("Close");
        close_btn.set_color(theme::button_subtle());
        close_btn.set_label_color(theme::text_primary());
        close_btn.set_frame(FrameType::RFlatBox);

        button_flex.fixed(&find_next_btn, 90);
        button_flex.fixed(&close_btn, 70);
        button_flex.end();
        main_flex.fixed(&button_flex, 35);

        main_flex.end();
        dialog.end();

        popups.borrow_mut().push(dialog.clone());
        // State for search
        let search_pos = Rc::new(RefCell::new(0i32));

        let (sender, receiver) = std::sync::mpsc::channel::<DialogMessage>();

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
            while let Ok(message) = receiver.try_recv() {
                match message {
                    DialogMessage::FindNext {
                        search_text,
                        case_sensitive,
                    } => {
                        let text = buffer.text();
                        let start_pos = *search_pos.borrow();

                        if let Some((match_start, match_end)) =
                            find_next_match(&text, &search_text, start_pos, case_sensitive)
                        {
                            buffer.select(
                                match_start as i32,
                                match_end as i32,
                            );
                            editor.set_insert_position(match_end as i32);
                            editor.show_insert_position();
                            // Use match_end instead of match_start + 1 to avoid UTF-8 boundary issues
                            *search_pos.borrow_mut() = match_end.min(text.len()) as i32;
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
                                selected.to_ascii_lowercase() == search_text.to_ascii_lowercase()
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
                        if search_text.is_empty() {
                            fltk::dialog::message_default("Search text is empty");
                            continue;
                        }
                        let text = buffer.text();
                        let new_text = if case_sensitive {
                            text.replace(&search_text, &replace_text)
                        } else {
                            let mut result = String::with_capacity(text.len());
                            let mut search_pos = 0usize;
                            while let Some((match_start, match_end)) =
                                find_next_match(&text, &search_text, search_pos as i32, false)
                            {
                                result.push_str(&text[search_pos..match_start]);
                                result.push_str(&replace_text);
                                search_pos = match_end;
                                if search_pos >= text.len() {
                                    break;
                                }
                            }
                            result.push_str(&text[search_pos..]);
                            result
                        };

                        let count = if case_sensitive {
                            text.matches(&search_text).count()
                        } else {
                            let mut count = 0usize;
                            let mut search_pos = 0usize;
                            while let Some((_match_start, match_end)) =
                                find_next_match(&text, &search_text, search_pos as i32, false)
                            {
                                count += 1;
                                search_pos = match_end;
                                if search_pos >= text.len() {
                                    break;
                                }
                            }
                            count
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

        // Remove dialog from popups to prevent memory leak
        popups.borrow_mut().retain(|w| w.as_widget_ptr() != dialog.as_widget_ptr());
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

        if let Some((match_start, match_end)) =
            find_next_match(&text, search_text, current_pos, case_sensitive)
        {
            buffer.select(
                match_start as i32,
                match_end as i32,
            );
            editor.set_insert_position(match_end as i32);
            editor.show_insert_position();
            true
        } else {
            // Try from beginning
            if let Some((match_start, match_end)) =
                find_next_match(&text, search_text, 0, case_sensitive)
            {
                buffer.select(match_start as i32, match_end as i32);
                editor.set_insert_position(match_end as i32);
                editor.show_insert_position();
                true
            } else {
                false
            }
        }
    }
}

fn find_next_match(
    text: &str,
    search_text: &str,
    start_pos: i32,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if search_text.is_empty() || text.is_empty() {
        return None;
    }
    let start_pos = if start_pos < 0 { 0 } else { start_pos as usize };
    let Some(haystack) = text.get(start_pos..) else {
        return None;
    };
    if case_sensitive {
        let pos = haystack.find(search_text)?;
        let match_start = start_pos + pos;
        let match_end = match_start + search_text.len();
        return Some((match_start, match_end));
    }

    let haystack_lower = haystack.to_ascii_lowercase();
    let search_lower = search_text.to_ascii_lowercase();
    let pos = haystack_lower.find(&search_lower)?;
    let match_start = start_pos + pos;
    let match_end = match_start + search_text.len();
    Some((match_start, match_end))
}
