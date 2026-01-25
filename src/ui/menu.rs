use fltk::{
    enums::Shortcut,
    menu::{MenuBar, MenuFlag},
    prelude::*,
};

use crate::ui::theme;

pub struct MenuBarBuilder;

fn forward_menu_callback(menu: &mut MenuBar) {
    menu.do_callback();
}

impl MenuBarBuilder {
    pub fn build() -> MenuBar {
        let mut menu = MenuBar::default();
        menu.set_color(theme::panel_raised());
        menu.set_text_color(theme::text_primary());
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
            Shortcut::Ctrl | Shortcut::Command | 'x',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Copy\t",
            Shortcut::Ctrl | Shortcut::Command | 'c',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Copy with &Headers\t",
            Shortcut::Ctrl | Shortcut::Command | Shortcut::Shift | 'c',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Paste\t",
            Shortcut::Ctrl | Shortcut::Command | 'v',
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
            Shortcut::Ctrl | Shortcut::Command | 'a',
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
        menu.add(
            "&Edit/",
            Shortcut::None,
            MenuFlag::MenuDivider,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Format SQL\t",
            Shortcut::Ctrl | Shortcut::Command | Shortcut::Shift | 'f',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Toggle &Comment\t",
            Shortcut::Ctrl | Shortcut::Command | '/',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Upper&case Selection\t",
            Shortcut::Ctrl | Shortcut::Command | 'u',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/Lower&case Selection\t",
            Shortcut::Ctrl | Shortcut::Command | 'l',
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Edit/&Intellisense\t",
            Shortcut::Ctrl | Shortcut::Command | ' ',
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
            "&Query/Execute &Statement\t",
            Shortcut::Ctrl | fltk::enums::Key::Enter,
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/Execute &Selected\t",
            Shortcut::from_key(fltk::enums::Key::F9),
            MenuFlag::Normal,
            forward_menu_callback,
        );
        menu.add(
            "&Query/&Quick Describe\t",
            Shortcut::from_key(fltk::enums::Key::F4),
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
                    File:\n\
                    Ctrl+N - Connect\n\
                    Ctrl+D - Disconnect\n\
                    Ctrl+O - Open SQL File\n\
                    Ctrl+S - Save SQL File\n\
                    Ctrl+Q - Exit\n\n\
                    Edit (SQL Editor):\n\
                    Ctrl+Z - Undo\n\
                    Ctrl+Y - Redo\n\
                    Ctrl+X - Cut\n\
                    Ctrl+C - Copy\n\
                    Ctrl+Shift+C - Copy with Headers\n\
                    Ctrl+V - Paste\n\
                    Ctrl+A - Select All\n\
                    Ctrl+F - Find\n\
                    F3 - Find Next\n\
                    Ctrl+H - Replace\n\
                    Ctrl+Shift+F - Format SQL\n\
                    Ctrl+/ - Toggle Comment\n\
                    Ctrl+U - Uppercase Selection\n\
                    Ctrl+L - Lowercase Selection\n\
                    Ctrl+Space - Intellisense\n\n\
                    Query:\n\
                    Ctrl+Enter - Execute Statement\n\
                    F5 - Execute Script\n\
                    F9 - Execute Selected\n\
                    F6 - Explain Plan\n\
                    F7 - Commit\n\
                    F8 - Rollback\n\
                    F4 - Quick Describe (Editor)\n\n\
                    Tools:\n\
                    Ctrl+E - Export Results\n\
                    Ctrl+H - Query History\n\n\
                    Results Table:\n\
                    Ctrl+C - Copy Selected Cells\n\
                    Ctrl+Shift+C - Copy with Headers\n\
                    Ctrl+A - Select All\n\n\
                    Object Browser:\n\
                    Enter - Generate SELECT (tables/views)",
                );
            },
        );

        menu
    }
}
