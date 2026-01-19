use fltk::{
    enums::{Color, Shortcut},
    menu::{MenuBar, MenuFlag},
    prelude::*,
};

pub struct MenuBarBuilder;

impl MenuBarBuilder {
    pub fn build() -> MenuBar {
        let mut menu = MenuBar::default();
        menu.set_color(Color::from_rgb(45, 45, 48));
        menu.set_text_color(Color::White);
        menu.set_id("main_menu");

        // File menu
        menu.add(
            "&File/&Connect...\t",
            Shortcut::Ctrl | 'n',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&File/&Disconnect\t",
            Shortcut::Ctrl | 'd',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&File/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&File/&Open SQL File...\t",
            Shortcut::Ctrl | 'o',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&File/&Save SQL File...\t",
            Shortcut::Ctrl | 's',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&File/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&File/E&xit\t",
            Shortcut::Ctrl | 'q',
            MenuFlag::Normal,
            |_| {},
        );

        // Edit menu
        menu.add(
            "&Edit/&Undo\t",
            Shortcut::Ctrl | 'z',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/&Redo\t",
            Shortcut::Ctrl | 'y',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Edit/Cu&t\t",
            Shortcut::Ctrl | 'x',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/&Copy\t",
            Shortcut::Ctrl | 'c',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/&Paste\t",
            Shortcut::Ctrl | 'v',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Edit/Select &All\t",
            Shortcut::Ctrl | 'a',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Edit/&Find...\t",
            Shortcut::Ctrl | 'f',
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/Find &Next\t",
            Shortcut::from_key(fltk::enums::Key::F3),
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Edit/&Replace...\t",
            Shortcut::Ctrl | 'h',
            MenuFlag::Normal,
            |_| {},
        );

        // Query menu
        menu.add(
            "&Query/&Execute\t",
            Shortcut::from_key(fltk::enums::Key::F5),
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Query/Execute &Selected\t",
            Shortcut::Ctrl | fltk::enums::Key::Enter,
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Query/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Query/E&xplain Plan\t",
            Shortcut::from_key(fltk::enums::Key::F6),
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Query/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Query/&Commit\t",
            Shortcut::from_key(fltk::enums::Key::F7),
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Query/&Rollback\t",
            Shortcut::from_key(fltk::enums::Key::F8),
            MenuFlag::Normal,
            |_| {},
        );

        // Tools menu
        menu.add(
            "&Tools/&Refresh Objects\t",
            Shortcut::from_key(fltk::enums::Key::F4),
            MenuFlag::Normal,
            |_| {},
        );
        menu.add(
            "&Tools/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            |_| {},
        );
        menu.add(
            "&Tools/&Export Results...\t",
            Shortcut::Ctrl | 'e',
            MenuFlag::Normal,
            |_| {},
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
                    F4 - Refresh Objects\n\
                    Ctrl+Q - Exit",
                );
            },
        );

        menu
    }
}
