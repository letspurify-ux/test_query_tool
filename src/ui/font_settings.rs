use fltk::enums::Font;

#[derive(Clone, Copy)]
pub struct FontProfile {
    pub name: &'static str,
    pub normal: Font,
    pub bold: Font,
    pub italic: Font,
}

pub const FONT_PROFILES: &[FontProfile] = &[
    FontProfile {
        name: "Courier",
        normal: Font::Courier,
        bold: Font::CourierBold,
        italic: Font::CourierItalic,
    },
    FontProfile {
        name: "Helvetica",
        normal: Font::Helvetica,
        bold: Font::HelveticaBold,
        italic: Font::HelveticaItalic,
    },
    FontProfile {
        name: "Times",
        normal: Font::Times,
        bold: Font::TimesBold,
        italic: Font::TimesItalic,
    },
];

pub fn profile_by_name(name: &str) -> FontProfile {
    FONT_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.name.eq_ignore_ascii_case(name))
        .unwrap_or(FONT_PROFILES[0])
}

pub fn font_choice_labels() -> String {
    FONT_PROFILES
        .iter()
        .map(|profile| profile.name)
        .collect::<Vec<_>>()
        .join("|")
}

pub fn font_choice_index(name: &str) -> i32 {
    FONT_PROFILES
        .iter()
        .position(|profile| profile.name.eq_ignore_ascii_case(name))
        .map(|idx| idx as i32)
        .unwrap_or(0)
}
