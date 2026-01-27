use fltk::{
    enums::{Color, Font},
    text::{StyleTableEntry, TextBuffer},
};
use once_cell::sync::Lazy;
use std::collections::HashSet;

use super::intellisense::{ORACLE_FUNCTIONS, SQL_KEYWORDS};
use crate::ui::theme;

// Style characters for different token types
pub const STYLE_DEFAULT: char = 'A';
pub const STYLE_KEYWORD: char = 'B';
pub const STYLE_FUNCTION: char = 'C';
pub const STYLE_STRING: char = 'D';
pub const STYLE_COMMENT: char = 'E';
pub const STYLE_NUMBER: char = 'F';
pub const STYLE_OPERATOR: char = 'G';
pub const STYLE_IDENTIFIER: char = 'H';

static SQL_KEYWORDS_SET: Lazy<HashSet<&'static str>> =
    Lazy::new(|| SQL_KEYWORDS.iter().copied().collect());
static ORACLE_FUNCTIONS_SET: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ORACLE_FUNCTIONS.iter().copied().collect());

/// Creates the style table for SQL syntax highlighting
pub fn create_style_table() -> Vec<StyleTableEntry> {
    vec![
        // A - Default text (light gray)
        StyleTableEntry {
            color: theme::text_primary(),
            font: Font::Courier,
            size: 14,
        },
        // B - SQL Keywords (blue)
        StyleTableEntry {
            color: Color::from_rgb(86, 156, 214),
            font: Font::CourierBold,
            size: 14,
        },
        // C - Functions (light purple/magenta)
        StyleTableEntry {
            color: Color::from_rgb(220, 220, 170),
            font: Font::Courier,
            size: 14,
        },
        // D - Strings (orange)
        StyleTableEntry {
            color: Color::from_rgb(206, 145, 120),
            font: Font::Courier,
            size: 14,
        },
        // E - Comments (green)
        StyleTableEntry {
            color: Color::from_rgb(106, 153, 85),
            font: Font::CourierItalic,
            size: 14,
        },
        // F - Numbers (light green)
        StyleTableEntry {
            color: Color::from_rgb(181, 206, 168),
            font: Font::Courier,
            size: 14,
        },
        // G - Operators (white)
        StyleTableEntry {
            color: theme::text_secondary(),
            font: Font::Courier,
            size: 14,
        },
        // H - Identifiers/Table names (cyan)
        StyleTableEntry {
            color: Color::from_rgb(78, 201, 176),
            font: Font::Courier,
            size: 14,
        },
    ]
}

/// SQL Token types
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum TokenType {
    Default,
    Keyword,
    Function,
    String,
    Comment,
    Number,
    Operator,
    Identifier,
}

impl TokenType {
    fn to_style_char(self) -> char {
        match self {
            TokenType::Default => STYLE_DEFAULT,
            TokenType::Keyword => STYLE_KEYWORD,
            TokenType::Function => STYLE_FUNCTION,
            TokenType::String => STYLE_STRING,
            TokenType::Comment => STYLE_COMMENT,
            TokenType::Number => STYLE_NUMBER,
            TokenType::Operator => STYLE_OPERATOR,
            TokenType::Identifier => STYLE_IDENTIFIER,
        }
    }
}

/// Holds additional identifiers for highlighting (tables, views, etc.)
#[derive(Clone, Default)]
pub struct HighlightData {
    pub tables: Vec<String>,
    pub views: Vec<String>,
    pub columns: Vec<String>,
}

impl HighlightData {
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            views: Vec::new(),
            columns: Vec::new(),
        }
    }

}

/// SQL Syntax Highlighter
pub struct SqlHighlighter {
    highlight_data: HighlightData,
    identifier_lookup: HashSet<String>,
}

const HIGHLIGHT_WINDOW_THRESHOLD: usize = 20_000;
const HIGHLIGHT_WINDOW_RADIUS: usize = 8_000;

impl SqlHighlighter {
    pub fn new() -> Self {
        Self {
            highlight_data: HighlightData::new(),
            identifier_lookup: HashSet::new(),
        }
    }

    pub fn set_highlight_data(&mut self, data: HighlightData) {
        self.highlight_data = data;
        self.rebuild_identifier_lookup();
    }

    fn rebuild_identifier_lookup(&mut self) {
        let mut lookup = HashSet::new();
        for name in self
            .highlight_data
            .tables
            .iter()
            .chain(self.highlight_data.views.iter())
            .chain(self.highlight_data.columns.iter())
        {
            lookup.insert(name.to_uppercase());
        }
        self.identifier_lookup = lookup;
    }

    /// Highlights using a windowed range from the buffer to avoid full-buffer scans.
    pub fn highlight_buffer_window(
        &self,
        buffer: &TextBuffer,
        style_buffer: &mut TextBuffer,
        cursor_pos: usize,
    ) {
        let text_len = buffer.length().max(0) as usize;
        if text_len == 0 {
            style_buffer.set_text("");
            return;
        }
        if text_len <= HIGHLIGHT_WINDOW_THRESHOLD {
            let text = buffer.text();
            let style_text = self.generate_styles(&text);
            style_buffer.set_text(&style_text);
            return;
        }

        if style_buffer.length() != text_len as i32 {
            let default_styles: String =
                std::iter::repeat(STYLE_DEFAULT).take(text_len).collect();
            style_buffer.set_text(&default_styles);
        }

        let cursor_pos = cursor_pos.min(text_len);
        let (range_start, range_end) = windowed_range_from_buffer(buffer, cursor_pos, text_len);
        if range_start >= range_end {
            return;
        }
        let Some(window_text) = buffer.text_range(range_start as i32, range_end as i32) else {
            return;
        };
        let window_styles = self.generate_styles(&window_text);
        if window_styles.len() != range_end - range_start {
            return;
        }
        style_buffer.replace(range_start as i32, range_end as i32, &window_styles);
    }

    /// Generates the style string for the given text
    ///
    /// IMPORTANT: FLTK TextBuffer uses byte-based indexing, so the style buffer
    /// must have one style character per byte.
    fn generate_styles(&self, text: &str) -> String {
        let mut styles: Vec<char> = vec![STYLE_DEFAULT; text.len()];
        let bytes = text.as_bytes();
        let mut idx = 0usize;

        while idx < bytes.len() {
            let byte = bytes[idx];

            // Check for single-line comment (--)
            if byte == b'-' && idx + 1 < bytes.len() && bytes[idx + 1] == b'-' {
                let start = idx;
                idx += 2;
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    idx += 1;
                }
                for b in start..idx {
                    styles[b] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for multi-line comment (/* */)
            if byte == b'/' && idx + 1 < bytes.len() && bytes[idx + 1] == b'*' {
                let start = idx;
                idx += 2;
                while idx + 1 < bytes.len() && !(bytes[idx] == b'*' && bytes[idx + 1] == b'/') {
                    idx += 1;
                }
                if idx + 1 < bytes.len() {
                    idx += 2;
                } else {
                    idx = bytes.len();
                }
                for b in start..idx {
                    styles[b] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for string literals ('...')
            if byte == b'\'' {
                let start = idx;
                idx += 1;
                while idx < bytes.len() {
                    if bytes[idx] == b'\'' {
                        if idx + 1 < bytes.len() && bytes[idx + 1] == b'\'' {
                            idx += 2;
                            continue;
                        }
                        idx += 1;
                        break;
                    }
                    idx += 1;
                }
                for b in start..idx {
                    styles[b] = STYLE_STRING;
                }
                continue;
            }

            // Check for numbers
            if byte.is_ascii_digit()
                || (byte == b'.'
                    && idx + 1 < bytes.len()
                    && bytes[idx + 1].is_ascii_digit())
            {
                let start = idx;
                let mut has_dot = byte == b'.';
                idx += 1;
                while idx < bytes.len() {
                    let next_byte = bytes[idx];
                    if next_byte.is_ascii_digit() {
                        idx += 1;
                    } else if next_byte == b'.' && !has_dot {
                        has_dot = true;
                        idx += 1;
                    } else {
                        break;
                    }
                }
                for b in start..idx {
                    styles[b] = STYLE_NUMBER;
                }
                continue;
            }

            // Check for identifiers/keywords
            if is_identifier_start_byte(byte) {
                let start = idx;
                idx += 1;
                while idx < bytes.len() && is_identifier_continue_byte(bytes[idx]) {
                    idx += 1;
                }
                let word = &text[start..idx];
                let token_type = self.classify_word(word);
                for b in start..idx {
                    styles[b] = token_type.to_style_char();
                }
                continue;
            }

            // Check for operators
            if is_operator_byte(byte) {
                styles[idx] = STYLE_OPERATOR;
                idx += 1;
                continue;
            }

            idx += 1;
        }

        styles.into_iter().collect()
    }

    /// Classifies a word as keyword, function, identifier, or default
    fn classify_word(&self, word: &str) -> TokenType {
        let upper = word.to_ascii_uppercase();

        // Check if it's a SQL keyword
        if SQL_KEYWORDS_SET.contains(upper.as_str()) {
            return TokenType::Keyword;
        }

        // Check if it's an Oracle function
        if ORACLE_FUNCTIONS_SET.contains(upper.as_str()) {
            return TokenType::Function;
        }

        // Check if it's a known identifier (table, view, column)
        if self.identifier_lookup.contains(&upper) {
            return TokenType::Identifier;
        }

        TokenType::Default
    }
}

impl Default for SqlHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

fn windowed_range_from_buffer(
    buffer: &TextBuffer,
    cursor_pos: usize,
    text_len: usize,
) -> (usize, usize) {
    let start_candidate = cursor_pos.saturating_sub(HIGHLIGHT_WINDOW_RADIUS);
    let end_candidate = (cursor_pos + HIGHLIGHT_WINDOW_RADIUS).min(text_len);

    let start = buffer.line_start(start_candidate as i32).max(0) as usize;
    let end = buffer.line_end(end_candidate as i32).max(0) as usize;

    (start.min(text_len), end.min(text_len))
}

fn is_operator_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'^' | b'%'
            | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b':' | b'.'
    )
}

fn is_identifier_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_continue_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windowed_range_for_test(text: &str, cursor_pos: usize) -> (usize, usize) {
        let start_candidate = cursor_pos.saturating_sub(HIGHLIGHT_WINDOW_RADIUS);
        let end_candidate = (cursor_pos + HIGHLIGHT_WINDOW_RADIUS).min(text.len());

        let start = match text.get(..start_candidate).and_then(|s| s.rfind('\n')) {
            Some(pos) => pos + 1,
            None => 0,
        };
        let end = match text.get(end_candidate..).and_then(|s| s.find('\n')) {
            Some(pos) => end_candidate + pos,
            None => text.len(),
        };

        (start, end)
    }

    fn generate_styles_windowed_for_test(
        highlighter: &SqlHighlighter,
        text: &str,
        cursor_pos: usize,
    ) -> String {
        if text.len() <= HIGHLIGHT_WINDOW_THRESHOLD {
            return highlighter.generate_styles(text);
        }

        let cursor_pos = cursor_pos.min(text.len());
        let (range_start, range_end) = windowed_range_for_test(text, cursor_pos);
        let window_text = &text[range_start..range_end];
        let window_styles = highlighter.generate_styles(window_text);
        let mut styles: Vec<char> = vec![STYLE_DEFAULT; text.len()];
        for (offset, style_char) in window_styles.chars().enumerate() {
            styles[range_start + offset] = style_char;
        }
        styles.into_iter().collect()
    }

    #[test]
    fn test_keyword_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT * FROM";
        let styles = highlighter.generate_styles(text);

        // "SELECT" should be keyword (B)
        assert!(styles.starts_with("BBBBBB"));
    }

    #[test]
    fn test_string_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "'hello world'";
        let styles = highlighter.generate_styles(text);

        // Entire string should be string style (D)
        assert!(styles.chars().all(|c| c == STYLE_STRING));
    }

    #[test]
    fn test_comment_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "-- this is a comment";
        let styles = highlighter.generate_styles(text);

        // Entire line should be comment style (E)
        assert!(styles.chars().all(|c| c == STYLE_COMMENT));
    }

    #[test]
    fn test_windowed_highlighting_limits_scope() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT col FROM table;\n".repeat(2000);
        assert!(text.len() > HIGHLIGHT_WINDOW_THRESHOLD);
        let cursor_pos = text.len() / 2;
        let styles = generate_styles_windowed_for_test(&highlighter, &text, cursor_pos);

        assert_eq!(styles.len(), text.len());

        let (range_start, range_end) = windowed_range_for_test(&text, cursor_pos);
        assert!(range_start > 0);
        assert!(range_end <= text.len());

        let outside_select_pos = text.find("SELECT").unwrap();
        if outside_select_pos + 6 < range_start {
            assert!(styles[outside_select_pos..outside_select_pos + 6]
                .chars()
                .all(|c| c == STYLE_DEFAULT));
        }

        let inside_select_pos = text[range_start..range_end]
            .find("SELECT")
            .map(|pos| range_start + pos)
            .unwrap();
        assert!(styles[inside_select_pos..inside_select_pos + 6]
            .chars()
            .all(|c| c == STYLE_KEYWORD));
    }
}
