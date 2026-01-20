use fltk::{
    enums::{Color, Font},
    text::{StyleTableEntry, TextBuffer},
};

use super::intellisense::{ORACLE_FUNCTIONS, SQL_KEYWORDS};

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
            color: Color::from_rgb(220, 220, 220),
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
            color: Color::from_rgb(212, 212, 212),
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

    /// Generates the style string for the given text
    fn generate_styles(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut styles: Vec<char> = vec![STYLE_DEFAULT; chars.len()];
        let mut i = 0;

        while i < chars.len() {
            // Check for single-line comment (--)
            if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] == '-' {
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                for j in start..i {
                    styles[j] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for multi-line comment (/* */)
            if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
                let start = i;
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                if i + 1 < chars.len() {
                    i += 2; // Skip */
                }
                for j in start..i {
                    styles[j] = STYLE_COMMENT;
                }
                continue;
            }

            // Check for string literals ('...')
            if chars[i] == '\'' {
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        // Check for escaped quote ('')
                        if i + 1 < chars.len() && chars[i + 1] == '\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                for j in start..i {
                    styles[j] = STYLE_STRING;
                }
                continue;
            }

            // Check for numbers
            if chars[i].is_ascii_digit()
                || (chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
            {
                let start = i;
                let mut has_dot = chars[i] == '.';
                i += 1;
                while i < chars.len() {
                    if chars[i].is_ascii_digit() {
                        i += 1;
                    } else if chars[i] == '.' && !has_dot {
                        has_dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                for j in start..i {
                    styles[j] = STYLE_NUMBER;
                }
                continue;
            }

            // Check for identifiers/keywords
            if chars[i].is_alphabetic() || chars[i] == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let token_type = self.classify_word(&word);
                for j in start..i {
                    styles[j] = token_type.to_style_char();
                }
                continue;
            }

            // Check for operators
            if is_operator(chars[i]) {
                styles[i] = STYLE_OPERATOR;
                i += 1;
                continue;
            }

            // Default: move to next character
            i += 1;
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
}
