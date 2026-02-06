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
fn format_sql_preserves_mega_torture_script() {
    let input = load_test_file("mega_torture.txt");
    let formatted = SqlEditorWidget::format_sql_basic(&input);

    let expected_lines = vec![
        "PROMPT [0] bind/substitution setup",
        "WHENEVER SQLERROR EXIT SQL.SQLCODE",
        "SHOW ERRORS PACKAGE BODY oqt_mega_pkg",
        "PROMPT [6] trigger (extra nesting surface)",
        "PROMPT [DONE]",
    ];
    let comment_snippets = vec![
        "q'[ | tokens: END; / ; /* */ -- ]'",
        "q'[ |trg tokens: END; / ; /* */ -- ]'",
        "q'[ |q-quote: END; / ; /* */ -- ]'",
    ];

    assert_contains_all(&formatted, &expected_lines);
    assert_contains_all(&formatted, &comment_snippets);

    let input_slashes = count_slash_lines(&input);
    let output_slashes = count_slash_lines(&formatted);
    assert_eq!(
        input_slashes, output_slashes,
        "Slash terminator count differs for mega_torture.txt"
    );

    let formatted_again = SqlEditorWidget::format_sql_basic(&formatted);
    assert_eq!(
        formatted, formatted_again,
        "Formatting should be idempotent for mega_torture.txt"
    );
}

#[test]
fn format_sql_preserves_whenever_sqlerror_options() {
    let input = [
        "WHENEVER SQLERROR EXIT SQL.SQLCODE",
        "WHENEVER SQLERROR EXIT FAILURE ROLLBACK",
        "WHENEVER SQLERROR EXIT SUCCESS",
        "WHENEVER SQLERROR EXIT WARNING",
        "WHENEVER SQLERROR EXIT 1",
        "WHENEVER SQLERROR CONTINUE",
        "WHENEVER SQLERROR CONTINUE ROLLBACK",
    ]
    .join("\n");

    let formatted = SqlEditorWidget::format_sql_basic(&input);
    let expected_lines = vec![
        "WHENEVER SQLERROR EXIT SQL.SQLCODE",
        "WHENEVER SQLERROR EXIT FAILURE ROLLBACK",
        "WHENEVER SQLERROR EXIT SUCCESS",
        "WHENEVER SQLERROR EXIT WARNING",
        "WHENEVER SQLERROR EXIT 1",
        "WHENEVER SQLERROR CONTINUE",
        "WHENEVER SQLERROR CONTINUE ROLLBACK",
    ];

    assert_contains_all(&formatted, &expected_lines);

    let formatted_again = SqlEditorWidget::format_sql_basic(&formatted);
    assert_eq!(
        formatted, formatted_again,
        "Formatting should be idempotent for WHENEVER SQLERROR variants"
    );
}

#[test]
fn format_sql_breaks_minified_package_body_members() {
    let input = "CREATE OR REPLACE PACKAGE BODY pkg AS PROCEDURE p IS BEGIN NULL; END; FUNCTION f RETURN NUMBER IS BEGIN RETURN 1; END; END pkg;";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("PACKAGE BODY pkg AS\n    PROCEDURE p IS"),
        "Package body should break before first procedure, got: {}",
        formatted
    );
    assert!(
        formatted.contains("END;\n\n    FUNCTION f RETURN NUMBER IS"),
        "Package body members should be separated by blank line, got: {}",
        formatted
    );
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
        formatted.contains("/* 헤더 주석 */\nSELECT 1\nFROM DUAL;"),
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
        formatted.contains("/* trailing */\nFROM DUAL;"),
        "newline after */ should be preserved, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_newline_before_line_comment() {
    let input = "SELECT 1\n-- comment\nFROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT 1\n-- comment\nFROM DUAL;"),
        "newline before -- should be preserved, got: {}",
        formatted
    );
}

#[test]
fn format_sql_preserves_newline_before_block_comment() {
    let input = "SELECT 1\n/* comment */\nFROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT 1\n/* comment */\nFROM DUAL;"),
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
fn format_sql_open_cursor_for_select_indentation() {
    let input = r#"BEGIN
OPEN p_rc
FOR
SELECT empno,
ename,
deptno,
salary
FROM oqt_emp
WHERE deptno = p_deptno
ORDER BY empno;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    OPEN p_rc FOR",
        "        SELECT empno,",
        "            ename,",
        "            deptno,",
        "            salary",
        "        FROM oqt_emp",
        "        WHERE deptno = p_deptno",
        "        ORDER BY empno;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_fetch_into_list_indentation() {
    let input = r#"BEGIN
FETCH c
INTO v_empno,
v_ename,
v_dept,
v_sal;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    FETCH c",
        "    INTO v_empno,",
        "        v_ename,",
        "        v_dept,",
        "        v_sal;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_keeps_insert_into_together() {
    let input = "INSERT\nINTO oqt_call_log (id, tag, msg, n1)\nVALUES (1, 'T', 'M', 10)";
    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "INSERT INTO oqt_call_log (id, tag, msg, n1)",
        "VALUES (1, 'T', 'M', 10);",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
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
        "BEGIN",
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

#[test]
fn format_sql_pre_dedents_else_elsif_exception_lines() {
    let input = r#"BEGIN
IF v_flag = 'Y' THEN
NULL;
ELSIF v_flag = 'N' THEN
NULL;
ELSE
NULL;
END IF;
EXCEPTION
WHEN OTHERS THEN
NULL;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    IF v_flag = 'Y' THEN",
        "        NULL;",
        "    ELSIF v_flag = 'N' THEN",
        "        NULL;",
        "    ELSE",
        "        NULL;",
        "    END IF;",
        "EXCEPTION",
        "    WHEN OTHERS THEN",
        "        NULL;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_parser_depth_indents_if_and_case_one_level_more() {
    let input = r#"BEGIN
IF v_flag = 'Y' THEN
CASE
WHEN v_num = 1 THEN
NULL;
ELSE
NULL;
END CASE;
END IF;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    IF v_flag = 'Y' THEN",
        "        CASE",
        "            WHEN v_num = 1 THEN",
        "                NULL;",
        "",
        "            ELSE",
        "                NULL;",
        "        END CASE;",
        "    END IF;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_case_branches_with_blank_lines() {
    let input = r#"BEGIN
CASE
WHEN p_n < 0 THEN
v := p_n * p_n;
WHEN p_n BETWEEN 0 AND 10 THEN
x := p_n + 100;
v := x - 50;
ELSE
v := p_n + 999;
END CASE;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    CASE",
        "        WHEN p_n < 0 THEN",
        "            v := p_n * p_n;",
        "",
        "        WHEN p_n BETWEEN 0 AND 10 THEN",
        "            x := p_n + 100;",
        "            v := x - 50;",
        "",
        "        ELSE",
        "            v := p_n + 999;",
        "    END CASE;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_keeps_comments_together() {
    let input = r#"BEGIN
-- first
-- second
NULL;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    -- first",
        "    -- second",
        "    NULL;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_does_not_insert_blank_line_between_line_comments() {
    let input = "-- first\n-- second\nSELECT 1 FROM dual;";

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = ["-- first", "-- second", "", "SELECT 1", "FROM dual;"].join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_does_not_insert_blank_line_between_prompt_commands() {
    let input = "PROMPT one\nPROMPT two\nSELECT 1 FROM dual;";

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = ["PROMPT one", "PROMPT two", "", "SELECT 1", "FROM dual;"].join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_indents_line_comments_to_depth() {
    let input = r#"BEGIN
IF 1 = 1 THEN
-- inside if
NULL;
END IF;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    IF 1 = 1 THEN",
        "        -- inside if",
        "        NULL;",
        "    END IF;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_does_not_apply_depth_indent_to_block_comments() {
    let input = r#"BEGIN
IF 1 = 1 THEN
/* block comment
still block comment */
NULL;
END IF;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "BEGIN",
        "    IF 1 = 1 THEN",
        "/* block comment",
        "still block comment */",
        "        NULL;",
        "    END IF;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_select_case_inside_sum_is_indented() {
    let input = r#"SELECT grp,
COUNT (*) AS cnt,
SUM (
CASE
WHEN MOD (n, 2) = 0 THEN 1
ELSE 0
END) AS even_cnt,
SUM (
CASE
WHEN INSTR (txt, 'END;') > 0 THEN 1
ELSE 0
END) AS has_end_token_cnt
FROM oqt_t_test
GROUP BY grp
ORDER BY grp;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "SELECT grp,",
        "    COUNT (*) AS cnt,",
        "    SUM (",
        "        CASE",
        "            WHEN MOD (n, 2) = 0 THEN 1",
        "            ELSE 0",
        "        END) AS even_cnt,",
        "    SUM (",
        "        CASE",
        "            WHEN INSTR (txt, 'END;') > 0 THEN 1",
        "            ELSE 0",
        "        END) AS has_end_token_cnt",
        "FROM oqt_t_test",
        "GROUP BY grp",
        "ORDER BY grp;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_declare_begin_pre_dedent() {
    let input = r#"DECLARE
v_old_sal NUMBER;
BEGIN
NULL;
END;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);
    let expected = [
        "DECLARE",
        "    v_old_sal NUMBER;",
        "BEGIN",
        "    NULL;",
        "END;",
    ]
    .join("\n");

    assert_eq!(formatted, expected);
}

#[test]
fn format_sql_parser_depth_covers_loop_subquery_with_and_package_body() {
    let input = r#"CREATE OR REPLACE PACKAGE BODY pkg_demo AS
PROCEDURE run_demo IS
BEGIN
FOR r IN (
SELECT id
FROM (
SELECT id FROM dual
)
) LOOP
NULL;
END LOOP;
END run_demo;
END pkg_demo;

WITH cte AS (
SELECT 1 AS n FROM dual
)
SELECT * FROM cte;"#;

    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("PACKAGE BODY pkg_demo AS\n    PROCEDURE run_demo IS"),
        "Package body scope should increase depth, got: {}",
        formatted
    );
    assert!(
        formatted.contains("PROCEDURE run_demo IS\n    BEGIN"),
        "Procedure BEGIN should align with procedure declaration, got: {}",
        formatted
    );
    assert!(
        formatted.contains("        FOR r IN (\n            SELECT id"),
        "Subquery SELECT should increase depth, got: {}",
        formatted
    );
    assert!(
        formatted.contains("        ) LOOP\n            NULL;\n        END LOOP;"),
        "LOOP body should be indented one level deeper, got: {}",
        formatted
    );
    assert!(
        formatted
            .contains("WITH cte AS (\n    SELECT 1 AS n\n    FROM DUAL\n)\nSELECT *\nFROM cte;"),
        "WITH CTE block should increase depth and restore on main SELECT, got: {}",
        formatted
    );
}

#[test]
fn format_sql_resets_paren_comma_suppression_after_top_level_semicolon() {
    let input = "SELECT func(a, b;\nSELECT c, d FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT\n    c,\n    d\nFROM DUAL;"),
        "Comma wrapping should recover for next top-level statement after ';', got: {}",
        formatted
    );
}

#[test]
fn format_sql_recovers_when_slash_appears_in_comment() {
    let input = "SELECT 1 FROM dual;
-- /
SELECT a, b FROM dual;";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("SELECT\n    a,\n    b\nFROM DUAL;"),
        "Formatter should recover top-level statement splitting after comment slash, got: {}",
        formatted
    );
}
#[test]
fn format_sql_comment_parenthesis_does_not_affect_comma_newline() {
    let input = "SELECT a /* (comment) */, b FROM dual";
    let formatted = SqlEditorWidget::format_sql_basic(input);

    assert!(
        formatted.contains("/* (comment) */,\n    b"),
        "Parenthesis inside comments must not keep comma on one line, got: {}",
        formatted
    );
}
