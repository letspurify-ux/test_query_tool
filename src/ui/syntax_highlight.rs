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
pub const STYLE_HINT: char = 'I';
pub const STYLE_DATETIME_LITERAL: char = 'J';

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
        // I - Hints (gold/yellow)
        StyleTableEntry {
            color: Color::from_rgb(255, 215, 0),
            font: Font::CourierItalic,
            size: 14,
        },
        // J - DateTime literals (DATE '...', TIMESTAMP '...', INTERVAL '...')
        StyleTableEntry {
            color: Color::from_rgb(255, 160, 122),
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

        while let Some(&byte) = bytes.get(idx) {
            // Check for PROMPT command at the start of a line (SQL*Plus style)
            if idx == 0 || bytes.get(idx.saturating_sub(1)) == Some(&b'\n') {
                let line_start = idx;
                let mut scan = idx;
                while bytes
                    .get(scan)
                    .map_or(false, |&b| b == b' ' || b == b'\t')
                {
                    scan += 1;
                }
                if is_prompt_keyword(bytes, scan) {
                    let mut end = scan;
                    while let Some(&b) = bytes.get(end) {
                        if b == b'\n' {
                            break;
                        }
                        end += 1;
                    }
                    for b in line_start..end {
                        if let Some(style) = styles.get_mut(b) {
                            *style = STYLE_COMMENT;
                        }
                    }
                    idx = end;
                    continue;
                }
            }

            // Check for single-line comment (--)
            if byte == b'-' && bytes.get(idx + 1) == Some(&b'-') {
                let start = idx;
                idx += 2;
                while let Some(&b) = bytes.get(idx) {
                    if b == b'\n' {
                        break;
                    }
                    idx += 1;
                }
                for b in start..idx {
                    if let Some(style) = styles.get_mut(b) {
                        *style = STYLE_COMMENT;
                    }
                }
                continue;
            }

            // Check for multi-line comment (/* */) or hint (/*+ */)
            if byte == b'/' && bytes.get(idx + 1) == Some(&b'*') {
                let start = idx;
                // Check if this is a hint (/*+ ...)
                let is_hint = bytes.get(idx + 2) == Some(&b'+');
                idx += 2;
                loop {
                    match (bytes.get(idx), bytes.get(idx + 1)) {
                        (Some(&b'*'), Some(&b'/')) => {
                            idx += 2;
                            break;
                        }
                        (Some(_), _) => idx += 1,
                        (None, _) => break,
                    }
                }
                let style_char = if is_hint { STYLE_HINT } else { STYLE_COMMENT };
                for b in start..idx {
                    if let Some(style) = styles.get_mut(b) {
                        *style = style_char;
                    }
                }
                continue;
            }

            // Check for nq-quoted strings: nq'[...]', nq'{...}', etc. (National Character Set)
            if (byte == b'n' || byte == b'N')
                && (bytes.get(idx + 1) == Some(&b'q') || bytes.get(idx + 1) == Some(&b'Q'))
                && bytes.get(idx + 2) == Some(&b'\'')
            {
                if let Some(&delimiter) = bytes.get(idx + 3) {
                    let closing = match delimiter {
                        b'[' => b']',
                        b'(' => b')',
                        b'{' => b'}',
                        b'<' => b'>',
                        _ => delimiter,
                    };
                    let start = idx;
                    idx += 4; // Skip nq'[
                    // Find closing delimiter followed by '
                    while idx < bytes.len() {
                        if bytes.get(idx) == Some(&closing)
                            && bytes.get(idx + 1) == Some(&b'\'')
                        {
                            idx += 2; // Include ]'
                            break;
                        }
                        idx += 1;
                    }
                    for b in start..idx {
                        if let Some(style) = styles.get_mut(b) {
                            *style = STYLE_STRING;
                        }
                    }
                    continue;
                }
            }

            // Check for q-quoted strings: q'[...]', q'{...}', etc.
            if (byte == b'q' || byte == b'Q')
                && bytes.get(idx + 1) == Some(&b'\'')
            {
                if let Some(&delimiter) = bytes.get(idx + 2) {
                    let closing = match delimiter {
                        b'[' => b']',
                        b'(' => b')',
                        b'{' => b'}',
                        b'<' => b'>',
                        _ => delimiter,
                    };
                    let start = idx;
                    idx += 3; // Skip q'[
                    // Find closing delimiter followed by '
                    while idx < bytes.len() {
                        if bytes.get(idx) == Some(&closing)
                            && bytes.get(idx + 1) == Some(&b'\'')
                        {
                            idx += 2; // Include ]'
                            break;
                        }
                        idx += 1;
                    }
                    for b in start..idx {
                        if let Some(style) = styles.get_mut(b) {
                            *style = STYLE_STRING;
                        }
                    }
                    continue;
                }
            }

            // Check for string literals ('...')
            if byte == b'\'' {
                let start = idx;
                idx += 1;
                while let Some(&b) = bytes.get(idx) {
                    if b == b'\'' {
                        if bytes.get(idx + 1) == Some(&b'\'') {
                            idx += 2;
                            continue;
                        }
                        idx += 1;
                        break;
                    }
                    idx += 1;
                }
                for b in start..idx {
                    if let Some(style) = styles.get_mut(b) {
                        *style = STYLE_STRING;
                    }
                }
                continue;
            }

            // Check for numbers
            if byte.is_ascii_digit()
                || (byte == b'.'
                    && bytes.get(idx + 1).map_or(false, |b| b.is_ascii_digit()))
            {
                let start = idx;
                let mut has_dot = byte == b'.';
                idx += 1;
                while let Some(&next_byte) = bytes.get(idx) {
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
                    if let Some(style) = styles.get_mut(b) {
                        *style = STYLE_NUMBER;
                    }
                }
                continue;
            }

            // Check for identifiers/keywords
            if is_identifier_start_byte(byte) {
                let start = idx;
                idx += 1;
                while bytes.get(idx).map_or(false, |&b| is_identifier_continue_byte(b)) {
                    idx += 1;
                }
                let word = text.get(start..idx).unwrap_or("");
                let upper_word = word.to_ascii_uppercase();

                // Check for DATE/TIMESTAMP/INTERVAL literals (e.g., DATE '2024-01-01')
                if matches!(upper_word.as_str(), "DATE" | "TIMESTAMP" | "INTERVAL") {
                    // Skip whitespace to find potential string literal
                    let mut look_ahead = idx;
                    while bytes.get(look_ahead).map_or(false, |&b| b == b' ' || b == b'\t') {
                        look_ahead += 1;
                    }
                    // Check if followed by a single quote (string literal)
                    if bytes.get(look_ahead) == Some(&b'\'') {
                        // Find the end of the string literal
                        look_ahead += 1;
                        while let Some(&b) = bytes.get(look_ahead) {
                            if b == b'\'' {
                                if bytes.get(look_ahead + 1) == Some(&b'\'') {
                                    look_ahead += 2; // escaped quote
                                    continue;
                                }
                                look_ahead += 1; // include closing quote
                                break;
                            }
                            look_ahead += 1;
                        }
                        // Style the keyword and string as datetime literal
                        for b in start..look_ahead {
                            if let Some(style) = styles.get_mut(b) {
                                *style = STYLE_DATETIME_LITERAL;
                            }
                        }
                        idx = look_ahead;
                        continue;
                    }
                }

                let token_type = self.classify_word(word);
                for b in start..idx {
                    if let Some(style) = styles.get_mut(b) {
                        *style = token_type.to_style_char();
                    }
                }
                continue;
            }

            // Check for operators
            if is_operator_byte(byte) {
                if let Some(style) = styles.get_mut(idx) {
                    *style = STYLE_OPERATOR;
                }
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

fn is_prompt_keyword(bytes: &[u8], start: usize) -> bool {
    if bytes.len() < start + 6 {
        return false;
    }
    if !bytes[start..start + 6]
        .iter()
        .zip(b"PROMPT")
        .all(|(b, c)| b.to_ascii_uppercase() == *c)
    {
        return false;
    }
    matches!(
        bytes.get(start + 6),
        None | Some(b' ') | Some(b'\t') | Some(b'\n')
    )
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
    fn test_prompt_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "PROMPT Enter value for id";
        let styles = highlighter.generate_styles(text);

        assert!(styles.chars().all(|c| c == STYLE_COMMENT));
    }

    #[test]
    fn test_prompt_highlighting_with_leading_whitespace() {
        let highlighter = SqlHighlighter::new();
        let text = "  prompt Enter value\nSELECT * FROM dual";
        let styles = highlighter.generate_styles(text);

        let first_line_end = text.find('\n').unwrap();
        assert!(styles[..first_line_end]
            .chars()
            .all(|c| c == STYLE_COMMENT));
        assert!(styles[first_line_end + 1..]
            .chars()
            .any(|c| c != STYLE_COMMENT));
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

    #[test]
    fn test_q_quote_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT q'[test string]' FROM dual";
        let styles = highlighter.generate_styles(text);

        // "SELECT" (0-5) should be keyword (B)
        assert!(
            styles[0..6].chars().all(|c| c == STYLE_KEYWORD),
            "SELECT should be keyword, got: {}",
            &styles[0..6]
        );

        // "q'[test string]'" (7-22) should be string (D)
        // Find the position of q'[
        let q_start = text.find("q'[").unwrap();
        let q_end = text.find("]'").unwrap() + 2;
        assert!(
            styles[q_start..q_end].chars().all(|c| c == STYLE_STRING),
            "q'[...]' should be string style, got: {}",
            &styles[q_start..q_end]
        );
    }

    #[test]
    fn test_nq_quote_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT nq'[national string]' FROM dual";
        let styles = highlighter.generate_styles(text);

        // "SELECT" should be keyword (B)
        assert!(
            styles[0..6].chars().all(|c| c == STYLE_KEYWORD),
            "SELECT should be keyword"
        );

        // "nq'[national string]'" should be string (D)
        let nq_start = text.find("nq'[").unwrap();
        let nq_end = text.find("]'").unwrap() + 2;
        assert!(
            styles[nq_start..nq_end].chars().all(|c| c == STYLE_STRING),
            "nq'[...]' should be string style, got: {}",
            &styles[nq_start..nq_end]
        );
    }

    #[test]
    fn test_nq_quote_case_insensitive_highlighting() {
        let highlighter = SqlHighlighter::new();

        // Test NQ (uppercase)
        let text1 = "SELECT NQ'[test]' FROM dual";
        let styles1 = highlighter.generate_styles(text1);
        let nq_start1 = text1.find("NQ'[").unwrap();
        let nq_end1 = text1.find("]'").unwrap() + 2;
        assert!(
            styles1[nq_start1..nq_end1].chars().all(|c| c == STYLE_STRING),
            "NQ'[...]' should be string style"
        );

        // Test Nq (mixed case)
        let text2 = "SELECT Nq'[test]' FROM dual";
        let styles2 = highlighter.generate_styles(text2);
        let nq_start2 = text2.find("Nq'[").unwrap();
        let nq_end2 = text2.find("]'").unwrap() + 2;
        assert!(
            styles2[nq_start2..nq_end2].chars().all(|c| c == STYLE_STRING),
            "Nq'[...]' should be string style"
        );
    }

    #[test]
    fn test_q_quote_different_delimiters() {
        let highlighter = SqlHighlighter::new();

        // Test q'(...)'
        let text1 = "SELECT q'(parentheses)' FROM dual";
        let styles1 = highlighter.generate_styles(text1);
        let q_start1 = text1.find("q'(").unwrap();
        let q_end1 = text1.find(")'").unwrap() + 2;
        assert!(
            styles1[q_start1..q_end1].chars().all(|c| c == STYLE_STRING),
            "q'(...)' should be string style"
        );

        // Test q'{...}'
        let text2 = "SELECT q'{braces}' FROM dual";
        let styles2 = highlighter.generate_styles(text2);
        let q_start2 = text2.find("q'{").unwrap();
        let q_end2 = text2.find("}'").unwrap() + 2;
        assert!(
            styles2[q_start2..q_end2].chars().all(|c| c == STYLE_STRING),
            "q'{{...}}' should be string style"
        );

        // Test q'<...>'
        let text3 = "SELECT q'<angle>' FROM dual";
        let styles3 = highlighter.generate_styles(text3);
        let q_start3 = text3.find("q'<").unwrap();
        let q_end3 = text3.find(">'").unwrap() + 2;
        assert!(
            styles3[q_start3..q_end3].chars().all(|c| c == STYLE_STRING),
            "q'<...>' should be string style"
        );
    }

    #[test]
    fn test_q_quote_with_embedded_quotes() {
        let highlighter = SqlHighlighter::new();
        // q-quoted strings can contain single quotes without escaping
        let text = "SELECT q'[It's a test]' FROM dual";
        let styles = highlighter.generate_styles(text);

        let q_start = text.find("q'[").unwrap();
        let q_end = text.find("]'").unwrap() + 2;
        assert!(
            styles[q_start..q_end].chars().all(|c| c == STYLE_STRING),
            "q'[...]' with embedded quote should be string style"
        );
    }

    #[test]
    fn test_hint_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT /*+ FULL(t) */ * FROM table t";
        let styles = highlighter.generate_styles(text);

        // Find the hint position
        let hint_start = text.find("/*+").unwrap();
        let hint_end = text.find("*/").unwrap() + 2;

        assert!(
            styles[hint_start..hint_end].chars().all(|c| c == STYLE_HINT),
            "Hint /*+ ... */ should be styled as hint, got: {}",
            &styles[hint_start..hint_end]
        );
    }

    #[test]
    fn test_hint_vs_regular_comment() {
        let highlighter = SqlHighlighter::new();

        // Regular comment should be comment style
        let text1 = "SELECT /* comment */ * FROM dual";
        let styles1 = highlighter.generate_styles(text1);
        let comment_start = text1.find("/*").unwrap();
        let comment_end = text1.find("*/").unwrap() + 2;
        assert!(
            styles1[comment_start..comment_end].chars().all(|c| c == STYLE_COMMENT),
            "Regular comment should be comment style"
        );

        // Hint should be hint style
        let text2 = "SELECT /*+ INDEX(t) */ * FROM dual";
        let styles2 = highlighter.generate_styles(text2);
        let hint_start = text2.find("/*+").unwrap();
        let hint_end = text2.find("*/").unwrap() + 2;
        assert!(
            styles2[hint_start..hint_end].chars().all(|c| c == STYLE_HINT),
            "Hint should be hint style"
        );
    }

    #[test]
    fn test_complex_hint_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT /*+ PARALLEL(t,4) FULL(t) INDEX(x idx_name) */ * FROM table t";
        let styles = highlighter.generate_styles(text);

        let hint_start = text.find("/*+").unwrap();
        let hint_end = text.find("*/").unwrap() + 2;
        assert!(
            styles[hint_start..hint_end].chars().all(|c| c == STYLE_HINT),
            "Complex hint should be fully styled as hint"
        );
    }

    #[test]
    fn test_date_literal_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT DATE '2024-01-01' FROM dual";
        let styles = highlighter.generate_styles(text);

        // Find DATE literal position
        let date_start = text.find("DATE").unwrap();
        let date_end = text.find("'2024-01-01'").unwrap() + "'2024-01-01'".len();

        assert!(
            styles[date_start..date_end].chars().all(|c| c == STYLE_DATETIME_LITERAL),
            "DATE literal should be styled as datetime literal, got: {}",
            &styles[date_start..date_end]
        );
    }

    #[test]
    fn test_timestamp_literal_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT TIMESTAMP '2024-01-01 12:30:00' FROM dual";
        let styles = highlighter.generate_styles(text);

        let ts_start = text.find("TIMESTAMP").unwrap();
        let ts_end = text.find("'2024-01-01 12:30:00'").unwrap() + "'2024-01-01 12:30:00'".len();

        assert!(
            styles[ts_start..ts_end].chars().all(|c| c == STYLE_DATETIME_LITERAL),
            "TIMESTAMP literal should be styled as datetime literal"
        );
    }

    #[test]
    fn test_interval_literal_highlighting() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT INTERVAL '5' DAY FROM dual";
        let styles = highlighter.generate_styles(text);

        let int_start = text.find("INTERVAL").unwrap();
        let int_end = text.find("'5'").unwrap() + "'5'".len();

        assert!(
            styles[int_start..int_end].chars().all(|c| c == STYLE_DATETIME_LITERAL),
            "INTERVAL literal should be styled as datetime literal"
        );
    }

    #[test]
    fn test_date_keyword_without_literal() {
        let highlighter = SqlHighlighter::new();
        // DATE as column name or keyword should be keyword style
        let text = "SELECT hire_date FROM employees";
        let styles = highlighter.generate_styles(text);

        // "date" in "hire_date" should not be specially styled
        // The whole identifier should be default
        let hire_date_start = text.find("hire_date").unwrap();
        let hire_date_end = hire_date_start + "hire_date".len();
        // hire_date is not a keyword or function, should be default
        assert!(
            styles[hire_date_start..hire_date_end].chars().all(|c| c == STYLE_DEFAULT),
            "hire_date should be default style"
        );
    }

    #[test]
    fn test_lowercase_date_literal() {
        let highlighter = SqlHighlighter::new();
        let text = "SELECT date '2024-01-01' FROM dual";
        let styles = highlighter.generate_styles(text);

        let date_start = text.find("date").unwrap();
        let date_end = text.find("'2024-01-01'").unwrap() + "'2024-01-01'".len();

        assert!(
            styles[date_start..date_end].chars().all(|c| c == STYLE_DATETIME_LITERAL),
            "Lowercase date literal should be styled as datetime literal"
        );
    }
}
