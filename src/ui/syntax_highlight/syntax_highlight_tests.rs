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
    assert!(styles[..first_line_end].chars().all(|c| c == STYLE_COMMENT));
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
        styles1[nq_start1..nq_end1]
            .chars()
            .all(|c| c == STYLE_STRING),
        "NQ'[...]' should be string style"
    );

    // Test Nq (mixed case)
    let text2 = "SELECT Nq'[test]' FROM dual";
    let styles2 = highlighter.generate_styles(text2);
    let nq_start2 = text2.find("Nq'[").unwrap();
    let nq_end2 = text2.find("]'").unwrap() + 2;
    assert!(
        styles2[nq_start2..nq_end2]
            .chars()
            .all(|c| c == STYLE_STRING),
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
        styles[hint_start..hint_end]
            .chars()
            .all(|c| c == STYLE_HINT),
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
        styles1[comment_start..comment_end]
            .chars()
            .all(|c| c == STYLE_COMMENT),
        "Regular comment should be comment style"
    );

    // Hint should be hint style
    let text2 = "SELECT /*+ INDEX(t) */ * FROM dual";
    let styles2 = highlighter.generate_styles(text2);
    let hint_start = text2.find("/*+").unwrap();
    let hint_end = text2.find("*/").unwrap() + 2;
    assert!(
        styles2[hint_start..hint_end]
            .chars()
            .all(|c| c == STYLE_HINT),
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
        styles[hint_start..hint_end]
            .chars()
            .all(|c| c == STYLE_HINT),
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
        styles[date_start..date_end]
            .chars()
            .all(|c| c == STYLE_DATETIME_LITERAL),
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
        styles[ts_start..ts_end]
            .chars()
            .all(|c| c == STYLE_DATETIME_LITERAL),
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
        styles[int_start..int_end]
            .chars()
            .all(|c| c == STYLE_DATETIME_LITERAL),
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
        styles[hire_date_start..hire_date_end]
            .chars()
            .all(|c| c == STYLE_DEFAULT),
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
        styles[date_start..date_end]
            .chars()
            .all(|c| c == STYLE_DATETIME_LITERAL),
        "Lowercase date literal should be styled as datetime literal"
    );
}
