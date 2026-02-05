use super::*;
use crate::ui::syntax_highlight::{STYLE_COMMENT, STYLE_KEYWORD, STYLE_STRING};

use std::fs;
use std::path::PathBuf;

fn load_test_file(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test");
    path.push(name);
    fs::read_to_string(path).unwrap_or_default()
}

fn count_slash_lines(text: &str) -> usize {
    text.lines().filter(|line| line.trim() == "/").count()
}

fn assert_contains_all(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            haystack.contains(needle),
            "Expected output to contain: {}",
            needle
        );
    }
}

#[test]
fn format_sql_preserves_script_commands_and_slashes() {
    let cases = [
        (
            "test1.txt",
            vec![
                "PROMPT 프로시저 테스트1",
                "SET SERVEROUTPUT ON",
                "SHOW ERRORS",
            ],
            vec![
                "OQT(Oracle Query Tool) - Procedure/Function Test Script",
                "-- 1) TEST DATA / TABLES",
            ],
        ),
        (
            "test2.txt",
            vec![
                "PROMPT 프로시저 테스트 2",
                "SET SERVEROUTPUT ON SIZE UNLIMITED",
                "SHOW ERRORS PACKAGE oqt_pkg",
                "SHOW ERRORS PACKAGE BODY oqt_pkg",
            ],
            vec![
                "PROMPT === [5] CALL VARIANTS: EXEC/BEGIN/DEFAULT/NAMED/POSITIONAL/NULL/UNICODE ===",
            ],
        ),
        (
            "test3.txt",
            vec![
                "PROMPT 프로시저 테스트3",
                "SET DEFINE OFF",
                "PROMPT === [B] Cleanup ===",
                "SHOW ERRORS",
            ],
            vec![
                "OQT (Oracle Query Tool) Compatibility Test Script (TOAD-like)",
            ],
        ),
    ];

    for (file, expected_lines, comment_snippets) in cases {
        let input = load_test_file(file);
        let formatted = SqlEditorWidget::format_sql_basic(&input);

        assert_contains_all(&formatted, &expected_lines);
        assert_contains_all(&formatted, &comment_snippets);

        let input_slashes = count_slash_lines(&input);
        let output_slashes = count_slash_lines(&formatted);
        assert_eq!(
            input_slashes, output_slashes,
            "Slash terminator count differs for {}",
            file
        );

        let formatted_again = SqlEditorWidget::format_sql_basic(&formatted);
        assert_eq!(
            formatted, formatted_again,
            "Formatting should be idempotent for {}",
            file
        );
    }
}

#[test]
fn format_sql_preserves_oracle_labels() {
    // Test <<loop_label>> preservation
    let input = "<<outer_loop>>\nFOR i IN 1..10 LOOP\n<<inner_loop>>\nFOR j IN 1..5 LOOP\nNULL;\nEND LOOP inner_loop;\nEND LOOP outer_loop;";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    // Labels should be preserved without extra spaces
    assert!(
        formatted.contains("<<outer_loop>>"),
        "Label <<outer_loop>> should be preserved, got: {}",
        formatted
    );
    assert!(
        formatted.contains("<<inner_loop>>"),
        "Label <<inner_loop>> should be preserved, got: {}",
        formatted
    );

    // Idempotent test
    let formatted_again = SqlEditorWidget::format_sql_basic(&formatted);
    assert_eq!(
        formatted, formatted_again,
        "Formatting should be idempotent for labels"
    );
}

#[test]
fn format_sql_preserves_q_quoted_strings() {
    // Test q'[...]' quote literal preservation
    let cases = [
        ("SELECT q'[It's a test]' FROM dual", "q'[It's a test]'"),
        ("SELECT q'{Hello World}' FROM dual", "q'{Hello World}'"),
        (
            "SELECT q'(Text with 'quotes')' FROM dual",
            "q'(Text with 'quotes')'",
        ),
        (
            "SELECT q'<Value with <brackets>>'",
            "q'<Value with <brackets>>'",
        ),
        (
            "SELECT Q'!Delimiter test!' FROM dual",
            "Q'!Delimiter test!'",
        ),
    ];

    for (input, expected_literal) in cases {
        let formatted = SqlEditorWidget::format_sql_basic(input);
        assert!(
            formatted.contains(expected_literal),
            "Q-quoted literal {} should be preserved in: {}",
            expected_literal,
            formatted
        );

        // Idempotent test
        let formatted_again = SqlEditorWidget::format_sql_basic(&formatted);
        assert_eq!(
            formatted, formatted_again,
            "Formatting should be idempotent for q-quoted string: {}",
            input
        );
    }
}

#[test]
fn format_sql_preserves_combined_special_syntax() {
    // Test combination of labels and q-quoted strings
    let input = r#"<<process_data>>
BEGIN
v_sql := q'[SELECT * FROM table WHERE name = 'test']';
EXECUTE IMMEDIATE v_sql;
END;
"#;
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("<<process_data>>"),
        "Label should be preserved"
    );
    assert!(
        formatted.contains("q'[SELECT * FROM table WHERE name = 'test']'"),
        "Q-quoted string should be preserved exactly"
    );
}

#[test]
fn format_sql_preserves_nq_quoted_strings() {
    // Test nq'[...]' (National Character q-quoted strings)
    let test_cases = [
        (
            "SELECT nq'[한글 문자열]' FROM dual",
            "nq'[한글 문자열]'",
            "basic nq'[...]' preservation",
        ),
        (
            "SELECT NQ'[UPPERCASE]' FROM dual",
            "NQ'[UPPERCASE]'",
            "uppercase NQ'[...]' preservation",
        ),
        (
            "SELECT Nq'[mixed case]' FROM dual",
            "Nq'[mixed case]'",
            "mixed case Nq'[...]' preservation",
        ),
        (
            "SELECT nq'(parentheses)' FROM dual",
            "nq'(parentheses)'",
            "nq'(...)' with parentheses",
        ),
        (
            "SELECT nq'{braces}' FROM dual",
            "nq'{braces}'",
            "nq'{...}' with braces",
        ),
        (
            "SELECT nq'<angle brackets>' FROM dual",
            "nq'<angle brackets>'",
            "nq'<...>' with angle brackets",
        ),
        (
            "SELECT nq'!custom!' FROM dual",
            "nq'!custom!'",
            "nq'!...!' with custom delimiter",
        ),
    ];

    for (input, expected, description) in test_cases {
        let formatted = SqlEditorWidget::format_sql_basic(input);
        assert!(
            formatted.contains(expected),
            "{}: expected '{}' in formatted output, got: {}",
            description,
            expected,
            formatted
        );
    }
}

#[test]
fn format_sql_preserves_nq_quote_with_semicolon() {
    // Test that semicolons inside nq'...' are preserved
    let input = "SELECT nq'[text with ; semicolon]' FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);
    assert!(
        formatted.contains("nq'[text with ; semicolon]'"),
        "nq'...' with semicolon should be preserved exactly, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_mixed_q_and_nq_quotes() {
    // Test both q'...' and nq'...' in same statement
    let input = "SELECT q'[regular]', nq'[national]' FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);
    assert!(
        formatted.contains("q'[regular]'"),
        "q'...' should be preserved, got: {}",
        formatted
    );
    assert!(
        formatted.contains("nq'[national]'"),
        "nq'...' should be preserved, got: {}",
        formatted
    );
}

#[test]
fn tokenize_sql_handles_nq_quotes() {
    // Direct test of tokenization for nq'...'
    let sql = "SELECT nq'[test string]' FROM dual";
    let tokens = SqlEditorWidget::tokenize_sql(sql);

    // Should have tokens: SELECT, nq'[test string]', FROM, dual
    let has_nq_string = tokens.iter().any(|t| {
        if let SqlToken::String(s) = t {
            s.contains("nq'[test string]'")
        } else {
            false
        }
    });
    assert!(
        has_nq_string,
        "Tokenizer should produce String token for nq'[...]', got: {:?}",
        tokens
    );
}

#[test]
fn format_sql_places_newline_after_inline_block_comment() {
    let input = "/* 헤더 주석 */SELECT 1 FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("/* 헤더 주석 */\nSELECT 1 FROM dual;"),
        "Inline block comment should be followed by newline before SQL, got: {}",
        formatted
    );
}

#[test]
fn format_sql_does_not_merge_end_statement_with_following_if() {
    let input = "BEGIN\nNULL;\nEND;\nIF 1 = 1 THEN\nNULL;\nEND IF;";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("END;\n\nIF 1 = 1 THEN"),
        "END; and following IF must remain separate, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_newline_after_block_comment_end() {
    let input = "SELECT 1 /* trailing */\nFROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("/* trailing */\nFROM dual;"),
        "newline after */ should be preserved, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_newline_before_line_comment() {
    let input = "SELECT 1\n-- comment\nFROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT 1\n-- comment\nFROM dual;"),
        "newline before -- should be preserved, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_newline_before_block_comment() {
    let input = "SELECT 1\n/* comment */\nFROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT 1\n/* comment */\nFROM dual;"),
        "newline before /* should be preserved, got: {}",
        formatted
    );
}

#[test]
fn format_sql_indents_select_list_item_starting_with_parenthesis() {
    let input = "SELECT (a + b) AS sum_value, c FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT\n    (a + b) AS sum_value,"),
        "Select list item starting with '(' should be indented under SELECT, got: {}",
        formatted
    );
}

#[test]
fn format_sql_indents_case_expression_inside_select_clause() {
    let input = "SELECT CASE WHEN a = 1 THEN 'Y' ELSE 'N' END AS flag FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT\n    CASE\n        WHEN a = 1 THEN 'Y'"),
        "CASE inside SELECT should start deeper than SELECT and WHEN should be deeper than CASE, got: {}",
        formatted
    );
}

#[test]
fn compute_edited_range_handles_insert_delete_and_replace() {
    assert_eq!(compute_edited_range(5, 3, 0, 20), Some((5, 8)));
    assert_eq!(compute_edited_range(5, 0, 4, 20), Some((5, 9)));
    assert_eq!(compute_edited_range(5, 2, 6, 20), Some((5, 11)));
}

#[test]
fn compute_edited_range_clamps_and_handles_invalid_pos() {
    assert_eq!(compute_edited_range(-1, 3, 0, 20), None);
    assert_eq!(compute_edited_range(50, 3, 0, 20), Some((20, 20)));
    assert_eq!(compute_edited_range(18, 10, 0, 20), Some((18, 20)));
}

#[test]
fn has_stateful_sql_delimiter_detects_comment_and_string_tokens() {
    assert!(has_stateful_sql_delimiter("/* comment"));
    assert!(has_stateful_sql_delimiter("end */"));
    assert!(has_stateful_sql_delimiter("-- line"));
    assert!(has_stateful_sql_delimiter("'text'"));
    assert!(has_stateful_sql_delimiter("q'[x]'"));
    assert!(has_stateful_sql_delimiter("NQ'[x]'"));
    assert!(!has_stateful_sql_delimiter("SELECT col FROM tab"));
}

#[test]
fn needs_full_rehighlight_when_edit_creates_stateful_token_with_neighbor_char() {
    let mut buffer = TextBuffer::default();
    buffer.set_text("SELECT 1 * FROM dual");

    // Simulate typing '/' right after '*' to form '*/'.
    let pos = buffer.text().find('*').unwrap() as i32 + 1;
    buffer.insert(pos, "/");

    assert!(needs_full_rehighlight(&buffer, pos, 1, ""));
}

#[test]
fn style_before_returns_previous_style_char() {
    let mut style_buffer = TextBuffer::default();
    style_buffer.set_text("AABCDE");

    assert_eq!(style_before(&style_buffer, 0), None);
    assert_eq!(style_before(&style_buffer, 1), Some('A'));
    assert_eq!(style_before(&style_buffer, 5), Some('D'));
}

#[test]
fn expand_connected_word_range_expands_to_identifier_boundaries() {
    let mut buffer = TextBuffer::default();
    buffer.set_text("SELECT employee_name FROM dual");

    let start = "SELECT ".len() + "employee".len();
    let end = start + 1;
    let expanded = expand_connected_word_range(&buffer, start, end);

    let text = buffer.text();
    assert_eq!(&text[expanded.0..expanded.1], "employee_name");
}

#[test]
fn inserted_text_reads_current_insert_span() {
    let mut buffer = TextBuffer::default();
    buffer.set_text("SELECT FROM dual");

    let insert_pos = "SELECT ".len() as i32;
    buffer.insert(insert_pos, "col ");

    assert_eq!(inserted_text(&buffer, insert_pos, 4), "col ");
    assert_eq!(inserted_text(&buffer, insert_pos, 0), "");
}

#[test]
fn is_string_or_comment_style_matches_only_comment_or_string() {
    assert!(is_string_or_comment_style(STYLE_COMMENT));
    assert!(is_string_or_comment_style(STYLE_STRING));
    assert!(!is_string_or_comment_style(STYLE_DEFAULT));
    assert!(!is_string_or_comment_style(STYLE_KEYWORD));
}

#[test]
fn format_sql_uses_parser_depth_for_plsql_blocks() {
    let input = r#"BEGIN
IF 1 = 1 THEN
BEGIN
NULL;
END;
END IF;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN;",
        "    IF 1 = 1 THEN",
        "        BEGIN",
        "            NULL;",
        "        END;",
        "    END IF;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}
