use fltk::{
    enums::{Color, Font},
    text::{StyleTableEntry, TextBuffer},
};

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

    pub fn is_identifier(&self, word: &str) -> bool {
        let upper = word.to_uppercase();
        self.tables.iter().any(|t| t.to_uppercase() == upper)
            || self.views.iter().any(|v| v.to_uppercase() == upper)
            || self.columns.iter().any(|c| c.to_uppercase() == upper)
    }
}

/// SQL Syntax Highlighter
pub struct SqlHighlighter {
    highlight_data: HighlightData,
}

const HIGHLIGHT_WINDOW_THRESHOLD: usize = 20_000;
const HIGHLIGHT_WINDOW_RADIUS: usize = 8_000;

impl SqlHighlighter {
    pub fn new() -> Self {
        Self {
            highlight_data: HighlightData::new(),
        }
    }

    pub fn set_highlight_data(&mut self, data: HighlightData) {
        self.highlight_data = data;
    }

    /// Highlights the given text and updates the style buffer
    pub fn highlight(&self, text: &str, style_buffer: &mut TextBuffer) {
        let style_text = self.generate_styles(text);
        style_buffer.set_text(&style_text);
    }

    /// Highlights the given text with a performance window around the cursor position.
    pub fn highlight_around_cursor(&self, text: &str, style_buffer: &mut TextBuffer, cursor_pos: usize) {
        let style_text = self.generate_styles_windowed(text, cursor_pos);
        style_buffer.set_text(&style_text);
    }

    /// Generates the style string for the given text
    ///
    /// IMPORTANT: FLTK TextBuffer uses byte-based indexing, so the style buffer
    /// must have one style character per byte, not per Unicode character.
    /// For multi-byte characters (like Korean, Chinese, etc.), we repeat the
    /// style character for each byte of the character.
    fn generate_styles(&self, text: &str) -> String {
        // Create a style buffer with one entry per byte
        let mut styles: Vec<char> = vec![STYLE_DEFAULT; text.len()];

        // We need to track character position
        let chars: Vec<char> = text.chars().collect();
        let mut char_idx = 0usize;

        // Helper to get byte offset for a character index
        let char_byte_offsets: Vec<usize> = text
            .char_indices()
            .map(|(byte_idx, _)| byte_idx)
            .chain(std::iter::once(text.len()))
            .collect();

        while char_idx < chars.len() {
            let byte_start = char_byte_offsets[char_idx];

            // Check for single-line comment (--)
            if char_idx + 1 < chars.len() && chars[char_idx] == '-' && chars[char_idx + 1] == '-' {
                let start_char = char_idx;
                while char_idx < chars.len() && chars[char_idx] != '\n' {
                    char_idx += 1;
                }
                // Fill styles for all bytes in this range
                let byte_end = char_byte_offsets[char_idx];
                for b in char_byte_offsets[start_char]..byte_end {
                    styles[b] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for multi-line comment (/* */)
            if char_idx + 1 < chars.len() && chars[char_idx] == '/' && chars[char_idx + 1] == '*' {
                let start_char = char_idx;
                char_idx += 2;
                while char_idx + 1 < chars.len() && !(chars[char_idx] == '*' && chars[char_idx + 1] == '/') {
                    char_idx += 1;
                }
                if char_idx + 1 < chars.len() {
                    char_idx += 2; // Skip */
                }
                let byte_end = char_byte_offsets[char_idx];
                for b in char_byte_offsets[start_char]..byte_end {
                    styles[b] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for string literals ('...')
            if chars[char_idx] == '\'' {
                let start_char = char_idx;
                char_idx += 1;
                while char_idx < chars.len() {
                    if chars[char_idx] == '\'' {
                        // Check for escaped quote ('')
                        if char_idx + 1 < chars.len() && chars[char_idx + 1] == '\'' {
                            char_idx += 2;
                            continue;
                        }
                        char_idx += 1;
                        break;
                    }
                    char_idx += 1;
                }
                let byte_end = char_byte_offsets[char_idx];
                for b in char_byte_offsets[start_char]..byte_end {
                    styles[b] = STYLE_STRING;
                }
                continue;
            }

            // Check for numbers
            if chars[char_idx].is_ascii_digit()
                || (chars[char_idx] == '.' && char_idx + 1 < chars.len() && chars[char_idx + 1].is_ascii_digit())
            {
                let start_char = char_idx;
                let mut has_dot = chars[char_idx] == '.';
                char_idx += 1;
                while char_idx < chars.len() {
                    if chars[char_idx].is_ascii_digit() {
                        char_idx += 1;
                    } else if chars[char_idx] == '.' && !has_dot {
                        has_dot = true;
                        char_idx += 1;
                    } else {
                        break;
                    }
                }
                let byte_end = char_byte_offsets[char_idx];
                for b in char_byte_offsets[start_char]..byte_end {
                    styles[b] = STYLE_NUMBER;
                }
                continue;
            }

            // Check for identifiers/keywords
            if chars[char_idx].is_alphabetic() || chars[char_idx] == '_' {
                let start_char = char_idx;
                while char_idx < chars.len() && (chars[char_idx].is_alphanumeric() || chars[char_idx] == '_' || chars[char_idx] == '$') {
                    char_idx += 1;
                }
                let word: String = chars[start_char..char_idx].iter().collect();
                let token_type = self.classify_word(&word);
                let byte_end = char_byte_offsets[char_idx];
                for b in char_byte_offsets[start_char]..byte_end {
                    styles[b] = token_type.to_style_char();
                }
                continue;
            }

            // Check for operators
            if is_operator(chars[char_idx]) {
                let byte_end = char_byte_offsets[char_idx + 1];
                for b in byte_start..byte_end {
                    styles[b] = STYLE_OPERATOR;
                }
                char_idx += 1;
                continue;
            }

            // Default: move to next character
            char_idx += 1;
        }

        styles.into_iter().collect()
    }

    fn generate_styles_windowed(&self, text: &str, cursor_pos: usize) -> String {
        if text.len() <= HIGHLIGHT_WINDOW_THRESHOLD {
            return self.generate_styles(text);
        }

        let cursor_pos = cursor_pos.min(text.len());
        let (range_start, range_end) = windowed_range(text, cursor_pos);
        let window_text = &text[range_start..range_end];
        let window_styles = self.generate_styles(window_text);
        let mut styles: Vec<char> = vec![STYLE_DEFAULT; text.len()];
        for (offset, style_char) in window_styles.chars().enumerate() {
            styles[range_start + offset] = style_char;
        }
        styles.into_iter().collect()
    }

    /// Classifies a word as keyword, function, identifier, or default
    fn classify_word(&self, word: &str) -> TokenType {
        let upper = word.to_uppercase();

        // Check if it's a SQL keyword
        if SQL_KEYWORDS.iter().any(|&kw| kw == upper) {
            return TokenType::Keyword;
        }

        // Check if it's an Oracle function
        if ORACLE_FUNCTIONS.iter().any(|&func| func == upper) {
            return TokenType::Function;
        }

        // Check if it's a known identifier (table, view, column)
        if self.highlight_data.is_identifier(word) {
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

fn windowed_range(text: &str, cursor_pos: usize) -> (usize, usize) {
    let start_candidate = cursor_pos.saturating_sub(HIGHLIGHT_WINDOW_RADIUS);
    let end_candidate = (cursor_pos + HIGHLIGHT_WINDOW_RADIUS).min(text.len());

    let mut start = start_candidate;
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = end_candidate;
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }

    let start = match text[..start].rfind('\n') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let end = match text[end..].find('\n') {
        Some(pos) => end + pos,
        None => text.len(),
    };

    (start, end)
}

/// Checks if a character is an SQL operator
fn is_operator(c: char) -> bool {
    matches!(
        c,
        '+' | '-' | '*' | '/' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '%' | '(' | ')' | '['
            | ']' | '{' | '}' | ',' | ';' | ':' | '.'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_multibyte_character_style_length() {
        // FLTK TextBuffer uses byte-based indexing, so the style buffer
        // must have one style character per byte, not per Unicode character.
        let highlighter = SqlHighlighter::new();

        // Korean text "한글" is 6 bytes in UTF-8 (3 bytes per character)
        let text = "SELECT '한글' FROM dual";
        let styles = highlighter.generate_styles(text);

        // Style buffer length must equal byte length, not character length
        assert_eq!(styles.len(), text.len(),
            "Style buffer length ({}) must match text byte length ({})",
            styles.len(), text.len());
    }

    #[test]
    fn test_multibyte_string_highlighting() {
        let highlighter = SqlHighlighter::new();

        // Test that Korean characters in string literals are properly highlighted
        let text = "'한글테스트'";
        let styles = highlighter.generate_styles(text);

        // All bytes should be string style (D)
        assert_eq!(styles.len(), text.len());
        assert!(styles.chars().all(|c| c == STYLE_STRING),
            "All bytes of multi-byte string should have string style");
    }

    #[test]
    fn test_multibyte_after_keyword() {
        let highlighter = SqlHighlighter::new();

        // Test that highlighting doesn't shift after multi-byte characters
        let text = "SELECT '가' FROM dual";
        let styles = highlighter.generate_styles(text);

        assert_eq!(styles.len(), text.len());

        // "SELECT" should be keyword (B) - first 6 bytes
        assert!(styles[..6].chars().all(|c| c == STYLE_KEYWORD));

        // Find "FROM" position and verify it's highlighted as keyword
        let from_pos = match text.find("FROM") {
            Some(pos) => pos,
            None => panic!("FROM keyword not found in test text"),
        };
        assert!(styles[from_pos..from_pos + 4].chars().all(|c| c == STYLE_KEYWORD),
            "FROM keyword should be highlighted correctly after multi-byte string");
    }

    #[test]
    fn test_windowed_highlighting_limits_scope() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT col FROM table;\n".repeat(2000);
        assert!(text.len() > HIGHLIGHT_WINDOW_THRESHOLD);
        let cursor_pos = text.len() / 2;
        let styles = highlighter.generate_styles_windowed(&text, cursor_pos);

        assert_eq!(styles.len(), text.len());

        let (range_start, range_end) = windowed_range(&text, cursor_pos);
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
