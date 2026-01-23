use fltk::{
    enums::{Color, Shortcut},
    menu::{MenuBar, MenuFlag},
    prelude::*,
};

pub struct MenuBarBuilder;

fn forward_menu_callback(menu: &mut MenuBar) {
    menu.do_callback();
}

impl MenuBarBuilder {
    pub fn build() -> MenuBar {
        let mut menu = MenuBar::default();
        menu.set_color(Color::from_rgb(45, 45, 48));
        menu.set_text_color(Color::White);
        menu.set_id("main_menu");

        // File menu
        menu.add(
            "&File/&Connect...\t",
            Shortcut::Ctrl | Shortcut::Command | 'n',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&File/&Disconnect\t",
            Shortcut::Ctrl | Shortcut::Command | 'd',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&File/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&File/&Open SQL File...\t",
            Shortcut::Ctrl | Shortcut::Command | 'o',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&File/&Save SQL File...\t",
            Shortcut::Ctrl | Shortcut::Command | 's',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&File/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&File/E&xit\t",
            Shortcut::Ctrl | Shortcut::Command | 'q',
            MenuFlag::Normal,
            forward_menu_callback,
        );

        // Edit menu
        menu.add(
            "&Edit/&Undo\t",
            Shortcut::Ctrl | Shortcut::Command | 'z',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Redo\t",
            Shortcut::Ctrl | Shortcut::Command | 'y',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Cu&t\t",
            Shortcut::None,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Copy\t",
            Shortcut::None,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Paste\t",
            Shortcut::None,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Select &All\t",
            Shortcut::None,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Find...\t",
            Shortcut::Ctrl | Shortcut::Command | 'f',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Find &Next\t",
            Shortcut::from_key(fltk::enums::Key::F3),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Replace...\t",
            Shortcut::Ctrl | Shortcut::Command | 'h',
            MenuFlag::Normal,
            forward_menu_callback,
        );

        // Query menu
        menu.add(
            "&Query/&Execute\t",
            Shortcut::from_key(fltk::enums::Key::F5),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/Execute &Selected\t",
            Shortcut::Ctrl | fltk::enums::Key::Enter,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Query/E&xplain Plan\t",
            Shortcut::from_key(fltk::enums::Key::F6),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Query/&Commit\t",
            Shortcut::from_key(fltk::enums::Key::F7),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/&Rollback\t",
            Shortcut::from_key(fltk::enums::Key::F8),
            MenuFlag::Normal,
            forward_menu_callback,
        );

        // Tools menu
        menu.add(
            "&Tools/&Refresh Objects\t",
            Shortcut::from_key(fltk::enums::Key::F4),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/&Export Results...\t",
            Shortcut::Ctrl | Shortcut::Command | 'e',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/&Query History...\t",
            Shortcut::Ctrl | Shortcut::Command | 'h',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/Query &History...\t",
            Shortcut::from_key(fltk::enums::Key::F9),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/&Feature Catalog...\t",
            Shortcut::None,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Tools/&Auto-Commit\t",
            Shortcut::None,
            MenuFlag::Toggle,
            forward_menu_callback,
        );

        // Help menu
        menu.add(
            "&Help/&About\t",
            Shortcut::None,
            MenuFlag::Normal,
            |_| {
                fltk::dialog::message_default(
                    "Oracle Query Tool v0.1.0\n\nBuilt with Rust and FLTK\n\nA Toad-like Oracle database query tool.",
                );
            },
        );
        menu.add(
            "&Help/&Keyboard Shortcuts\t",
            Shortcut::None,
            MenuFlag::Normal,
            |_| {
                fltk::dialog::message_default(
                    "Keyboard Shortcuts:\n\n\
                    Ctrl+N - New Connection\n\
                    Ctrl+D - Disconnect\n\
                    Ctrl+O - Open SQL File\n\
                    Ctrl+S - Save SQL File\n\
                    Ctrl+F - Find\n\
                    Ctrl+H - Replace\n\
                    F3 - Find Next\n\
                    Ctrl+Space - Intellisense\n\
                    Ctrl+E - Export Results\n\
                    F5 - Execute Query\n\
                    F6 - Explain Plan\n\
                    F7 - Commit\n\
                    F8 - Rollback\n\
                    F9 - Query History\n\
                    F4 - Refresh Objects\n\
                    Ctrl+Q - Exit",
                );
            },
        );

        menu
    }
}
