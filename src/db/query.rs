use oracle::{Connection, Error as OracleError, Row};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    #[allow(dead_code)]
    pub data_type: String,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    #[allow(dead_code)]
    pub sql: String,
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub execution_time: Duration,
    pub message: String,
    pub is_select: bool,
}

impl QueryResult {
    pub fn new_select(
        sql: &str,
        columns: Vec<ColumnInfo>,
        rows: Vec<Vec<String>>,
        execution_time: Duration,
    ) -> Self {
        let row_count = rows.len();
        Self {
            sql: sql.to_string(),
            columns,
            rows,
            row_count,
            execution_time,
            message: format!("{} rows fetched", row_count),
            is_select: true,
        }
    }

    pub fn new_select_streamed(
        sql: &str,
        columns: Vec<ColumnInfo>,
        row_count: usize,
        execution_time: Duration,
    ) -> Self {
        Self {
            sql: sql.to_string(),
            columns,
            rows: Vec::new(),
            row_count,
            execution_time,
            message: format!("{} rows fetched", row_count),
            is_select: true,
        }
    }

    pub fn new_dml(
        sql: &str,
        affected_rows: u64,
        execution_time: Duration,
        statement_type: &str,
    ) -> Self {
        Self {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: affected_rows as usize,
            execution_time,
            message: format!("{} {} row(s) affected", statement_type, affected_rows),
            is_select: false,
        }
    }

    pub fn new_error(sql: &str, error: &str) -> Self {
        Self {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time: Duration::from_secs(0),
            message: format!("Error: {}", error),
            is_select: false,
        }
    }
}

pub struct QueryExecutor;

impl QueryExecutor {
    fn strip_leading_comments(sql: &str) -> String {
        let mut remaining = sql;

        loop {
            let trimmed = remaining.trim_start();

            if trimmed.starts_with("--") {
                if let Some(line_end) = trimmed.find('\n') {
                    remaining = &trimmed[line_end + 1..];
                    continue;
                }
                return String::new();
            }

            if trimmed.starts_with("/*") {
                if let Some(block_end) = trimmed.find("*/") {
                    remaining = &trimmed[block_end + 2..];
                    continue;
                }
                return String::new();
            }

            return trimmed.to_string();
        }
    }

    fn leading_keyword(sql: &str) -> Option<String> {
        let cleaned = Self::strip_leading_comments(sql);
        cleaned
            .split_whitespace()
            .next()
            .map(|token| token.to_uppercase())
    }

    pub fn is_select_statement(sql: &str) -> bool {
        matches!(Self::leading_keyword(sql).as_deref(), Some("SELECT") | Some("WITH"))
    }

    /// Execute a single SQL statement
    pub fn execute(conn: &Connection, sql: &str) -> Result<QueryResult, OracleError> {
        let sql_trimmed = sql.trim();
        // Remove trailing semicolon if present (but keep for PL/SQL blocks)
        let sql_clean = if matches!(
            Self::leading_keyword(sql_trimmed).as_deref(),
            Some("BEGIN") | Some("DECLARE")
        )
        {
            sql_trimmed.to_string()
        } else {
            sql_trimmed.trim_end_matches(';').trim().to_string()
        };
        let sql_upper = Self::strip_leading_comments(&sql_clean).to_uppercase();

        let start = Instant::now();

        // SELECT or WITH (Common Table Expression)
        if Self::is_select_statement(&sql_clean) {
            Self::execute_select(conn, &sql_clean, start)
        }
        // DML statements
        else if sql_upper.starts_with("INSERT") {
            Self::execute_dml(conn, &sql_clean, start, "INSERT")
        } else if sql_upper.starts_with("UPDATE") {
            Self::execute_dml(conn, &sql_clean, start, "UPDATE")
        } else if sql_upper.starts_with("DELETE") {
            Self::execute_dml(conn, &sql_clean, start, "DELETE")
        } else if sql_upper.starts_with("MERGE") {
            Self::execute_dml(conn, &sql_clean, start, "MERGE")
        }
        // PL/SQL anonymous blocks
        else if sql_upper.starts_with("BEGIN") || sql_upper.starts_with("DECLARE") {
            Self::execute_plsql_block(conn, &sql_clean, start)
        }
        // Procedure calls with CALL
        else if sql_upper.starts_with("CALL") {
            Self::execute_call(conn, &sql_clean, start)
        }
        // Procedure calls with EXEC/EXECUTE (SQL*Plus style)
        else if sql_upper.starts_with("EXEC") {
            Self::execute_exec(conn, &sql_clean, start)
        }
        // DDL statements
        else if sql_upper.starts_with("CREATE")
            || sql_upper.starts_with("ALTER")
            || sql_upper.starts_with("DROP")
            || sql_upper.starts_with("TRUNCATE")
            || sql_upper.starts_with("RENAME")
            || sql_upper.starts_with("GRANT")
            || sql_upper.starts_with("REVOKE")
            || sql_upper.starts_with("COMMENT")
        {
            Self::execute_ddl(conn, &sql_clean, start)
        }
        // Transaction control
        else if sql_upper.starts_with("COMMIT") {
            match conn.commit() {
                Ok(()) => {}
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            }
            Ok(QueryResult {
                sql: sql_clean,
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: start.elapsed(),
                message: "Commit complete".to_string(),
                is_select: false,
            })
        } else if sql_upper.starts_with("ROLLBACK") {
            match conn.rollback() {
                Ok(()) => {}
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            }
            Ok(QueryResult {
                sql: sql_clean,
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: start.elapsed(),
                message: "Rollback complete".to_string(),
                is_select: false,
            })
        }
        // Everything else - try as DDL/DML
        else {
            Self::execute_ddl(conn, &sql_clean, start)
        }
    }

    /// Execute multiple SQL statements separated by semicolons
    /// Returns the result of the last SELECT statement, or a summary of DML/DDL operations
    pub fn execute_batch(conn: &Connection, sql: &str) -> Result<QueryResult, OracleError> {
        let statements = Self::split_statements_with_blocks(sql);

        if statements.is_empty() {
            return Ok(QueryResult {
                sql: sql.to_string(),
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: Duration::from_secs(0),
                message: "No statements to execute".to_string(),
                is_select: false,
            });
        }

        // If only one statement, just execute it
        if statements.len() == 1 {
            return Self::execute(conn, &statements[0]);
        }

        let start = Instant::now();
        let mut last_select_result: Option<QueryResult> = None;
        let mut total_affected = 0u64;
        let mut executed_count = 0;
        let mut error_messages: Vec<String> = Vec::new();

        for (i, stmt) in statements.iter().enumerate() {
            match Self::execute(conn, stmt) {
                Ok(result) => {
                    executed_count += 1;
                    if result.is_select {
                        last_select_result = Some(result);
                    } else {
                        total_affected += result.row_count as u64;
                    }
                }
                Err(e) => {
                    error_messages.push(format!("Statement {}: {}", i + 1, e));
                }
            }
        }

        let execution_time = start.elapsed();

        // If we have a SELECT result, return it with batch info
        if let Some(mut result) = last_select_result {
            result.execution_time = execution_time;
            if executed_count > 1 {
                result.message = format!(
                    "{} (Executed {} of {} statements)",
                    result.message,
                    executed_count,
                    statements.len()
                );
            }
            if !error_messages.is_empty() {
                result.message =
                    format!("{} | Errors: {}", result.message, error_messages.join("; "));
            }
            Ok(result)
        } else {
            // Return a summary for DML/DDL batch
            let message = if error_messages.is_empty() {
                format!(
                    "Executed {} statements, {} row(s) affected",
                    executed_count, total_affected
                )
            } else {
                format!(
                    "Executed {} of {} statements, {} row(s) affected | Errors: {}",
                    executed_count,
                    statements.len(),
                    total_affected,
                    error_messages.join("; ")
                )
            };

            Ok(QueryResult {
                sql: sql.to_string(),
                columns: vec![],
                rows: vec![],
                row_count: total_affected as usize,
                execution_time,
                message,
                is_select: false,
            })
        }
    }

    #[allow(dead_code)]
    pub fn execute_batch_streaming<F, G>(
        conn: &Connection,
        sql: &str,
        mut on_select_start: F,
        mut on_row: G,
    ) -> Result<QueryResult, OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>),
    {
        let statements = Self::split_statements_with_blocks(sql);

        if statements.is_empty() {
            return Ok(QueryResult {
                sql: sql.to_string(),
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: Duration::from_secs(0),
                message: "No statements to execute".to_string(),
                is_select: false,
            });
        }

        if statements.len() == 1 {
            let statement = statements[0].trim();
            if Self::is_select_statement(statement) {
                return Self::execute_select_streaming(
                    conn,
                    statement,
                    &mut on_select_start,
                    &mut on_row,
                );
            }

            return Self::execute(conn, statement);
        }

        Self::execute_batch(conn, sql)
    }

    /// Split SQL text into individual statements by semicolons.
    /// Handles quoted strings, comments, and PL/SQL blocks (BEGIN/END, DECLARE).
    pub fn split_statements_with_blocks(sql: &str) -> Vec<String> {
        let mut statements = Vec::new();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut block_depth = 0usize;
        let mut token = String::new();
        let chars: Vec<char> = sql.chars().collect();
        let len = chars.len();
        let mut i = 0;

        let flush_token = |token: &mut String, block_depth: &mut usize| {
            if token.is_empty() {
                return;
            }
            let upper = token.to_uppercase();
            if upper == "BEGIN" || upper == "DECLARE" {
                *block_depth += 1;
            } else if upper == "END" && *block_depth > 0 {
                *block_depth -= 1;
            }
            token.clear();
        };

        while i < len {
            let c = chars[i];
            let next = if i + 1 < len {
                Some(chars[i + 1])
            } else {
                None
            };

            // Handle line comment
            if !in_single_quote && !in_double_quote && !in_block_comment {
                if c == '-' && next == Some('-') {
                    in_line_comment = true;
                    current.push(c);
                    i += 1;
                    continue;
                }
            }

            if in_line_comment {
                current.push(c);
                if c == '\n' {
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if !in_single_quote
                && !in_double_quote
                && !in_block_comment
                && (c.is_alphanumeric() || c == '_')
            {
                token.push(c);
                current.push(c);
                i += 1;
                continue;
            }

            flush_token(&mut token, &mut block_depth);

            // Handle block comment
            if !in_single_quote && !in_double_quote && !in_line_comment {
                if c == '/' && next == Some('*') {
                    in_block_comment = true;
                    current.push(c);
                    i += 1;
                    continue;
                }
            }

            if in_block_comment {
                current.push(c);
                if c == '*' && next == Some('/') {
                    current.push(chars[i + 1]);
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            // Handle quotes
            if c == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                current.push(c);
                i += 1;
                continue;
            }

            if c == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                current.push(c);
                i += 1;
                continue;
            }

            // Handle semicolon (statement separator)
            if c == ';' && !in_single_quote && !in_double_quote && block_depth == 0 {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                }
                current.clear();
                i += 1;
                continue;
            }

            current.push(c);
            i += 1;
        }

        flush_token(&mut token, &mut block_depth);

        // Don't forget the last statement
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            statements.push(trimmed.to_string());
        }

        statements
    }

    /// Return the statement containing the cursor position (character index).
    pub fn statement_at_cursor(sql: &str, cursor_pos: usize) -> Option<String> {
        if sql.trim().is_empty() {
            return None;
        }

        #[derive(Clone)]
        struct StatementSpan {
            start_idx: usize,
            end_idx: usize,
            start_line: usize,
            end_line: usize,
            text: String,
        }

        let chars: Vec<char> = sql.chars().collect();
        let len = chars.len();
        let cursor_pos = cursor_pos.min(len);
        let cursor_line = chars
            .iter()
            .take(cursor_pos)
            .filter(|c| **c == '\n')
            .count();

        let mut statements: Vec<StatementSpan> = Vec::new();
        let mut current = String::new();

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut block_depth = 0usize;
        let mut token = String::new();

        let mut line = 0usize;
        let mut statement_start = 0usize;
        let mut statement_start_line = 0usize;
        let mut first_non_ws_idx: Option<usize> = None;
        let mut first_non_ws_line: Option<usize> = None;
        let mut last_non_ws_idx: Option<usize> = None;
        let mut last_non_ws_line: Option<usize> = None;

        let flush_token = |token: &mut String, block_depth: &mut usize| {
            if token.is_empty() {
                return;
            }
            let upper = token.to_uppercase();
            if upper == "BEGIN" || upper == "DECLARE" {
                *block_depth += 1;
            } else if upper == "END" && *block_depth > 0 {
                *block_depth -= 1;
            }
            token.clear();
        };

        let mark_non_ws = |c: char,
                           idx: usize,
                           line: usize,
                           first_non_ws_idx: &mut Option<usize>,
                           first_non_ws_line: &mut Option<usize>,
                           last_non_ws_idx: &mut Option<usize>,
                           last_non_ws_line: &mut Option<usize>| {
            if !c.is_whitespace() {
                if first_non_ws_idx.is_none() {
                    *first_non_ws_idx = Some(idx);
                    *first_non_ws_line = Some(line);
                }
                *last_non_ws_idx = Some(idx);
                *last_non_ws_line = Some(line);
            }
        };

        let mut i = 0;
        while i < len {
            let c = chars[i];
            let next = if i + 1 < len { Some(chars[i + 1]) } else { None };

            // Handle line comment
            if !in_single_quote && !in_double_quote && !in_block_comment {
                if c == '-' && next == Some('-') {
                    in_line_comment = true;
                    current.push(c);
                    mark_non_ws(
                        c,
                        i,
                        line,
                        &mut first_non_ws_idx,
                        &mut first_non_ws_line,
                        &mut last_non_ws_idx,
                        &mut last_non_ws_line,
                    );
                    i += 1;
                    continue;
                }
            }

            if in_line_comment {
                current.push(c);
                mark_non_ws(
                    c,
                    i,
                    line,
                    &mut first_non_ws_idx,
                    &mut first_non_ws_line,
                    &mut last_non_ws_idx,
                    &mut last_non_ws_line,
                );
                if c == '\n' {
                    in_line_comment = false;
                    line += 1;
                }
                i += 1;
                continue;
            }

            if !in_single_quote
                && !in_double_quote
                && !in_block_comment
                && (c.is_alphanumeric() || c == '_')
            {
                token.push(c);
                current.push(c);
                mark_non_ws(
                    c,
                    i,
                    line,
                    &mut first_non_ws_idx,
                    &mut first_non_ws_line,
                    &mut last_non_ws_idx,
                    &mut last_non_ws_line,
                );
                i += 1;
                continue;
            }

            flush_token(&mut token, &mut block_depth);

            // Handle block comment
            if !in_single_quote && !in_double_quote && !in_line_comment {
                if c == '/' && next == Some('*') {
                    in_block_comment = true;
                    current.push(c);
                    mark_non_ws(
                        c,
                        i,
                        line,
                        &mut first_non_ws_idx,
                        &mut first_non_ws_line,
                        &mut last_non_ws_idx,
                        &mut last_non_ws_line,
                    );
                    i += 1;
                    continue;
                }
            }

            if in_block_comment {
                current.push(c);
                mark_non_ws(
                    c,
                    i,
                    line,
                    &mut first_non_ws_idx,
                    &mut first_non_ws_line,
                    &mut last_non_ws_idx,
                    &mut last_non_ws_line,
                );
                if c == '\n' {
                    line += 1;
                }
                if c == '*' && next == Some('/') {
                    let next_char = chars[i + 1];
                    current.push(next_char);
                    mark_non_ws(
                        next_char,
                        i + 1,
                        line,
                        &mut first_non_ws_idx,
                        &mut first_non_ws_line,
                        &mut last_non_ws_idx,
                        &mut last_non_ws_line,
                    );
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            // Handle quotes
            if c == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                current.push(c);
                mark_non_ws(
                    c,
                    i,
                    line,
                    &mut first_non_ws_idx,
                    &mut first_non_ws_line,
                    &mut last_non_ws_idx,
                    &mut last_non_ws_line,
                );
                if c == '\n' {
                    line += 1;
                }
                i += 1;
                continue;
            }

            if c == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                current.push(c);
                mark_non_ws(
                    c,
                    i,
                    line,
                    &mut first_non_ws_idx,
                    &mut first_non_ws_line,
                    &mut last_non_ws_idx,
                    &mut last_non_ws_line,
                );
                if c == '\n' {
                    line += 1;
                }
                i += 1;
                continue;
            }

            // Handle semicolon (statement separator)
            if c == ';' && !in_single_quote && !in_double_quote && block_depth == 0 {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    let start_idx = first_non_ws_idx.unwrap_or(statement_start);
                    let start_line = first_non_ws_line.unwrap_or(statement_start_line);
                    statements.push(StatementSpan {
                        start_idx,
                        end_idx: i,
                        start_line,
                        end_line: line,
                        text: trimmed.to_string(),
                    });
                }

                current.clear();
                first_non_ws_idx = None;
                first_non_ws_line = None;
                last_non_ws_idx = None;
                last_non_ws_line = None;
                statement_start = i + 1;
                statement_start_line = line;
                i += 1;
                continue;
            }

            current.push(c);
            mark_non_ws(
                c,
                i,
                line,
                &mut first_non_ws_idx,
                &mut first_non_ws_line,
                &mut last_non_ws_idx,
                &mut last_non_ws_line,
            );
            if c == '\n' {
                line += 1;
            }
            i += 1;
        }

        flush_token(&mut token, &mut block_depth);

        let trimmed = current.trim();
        if !trimmed.is_empty() {
            let start_idx = first_non_ws_idx.unwrap_or(statement_start);
            let start_line = first_non_ws_line.unwrap_or(statement_start_line);
            let end_idx = last_non_ws_idx.unwrap_or_else(|| len.saturating_sub(1));
            let end_line = last_non_ws_line.unwrap_or(start_line);
            statements.push(StatementSpan {
                start_idx,
                end_idx,
                start_line,
                end_line,
                text: trimmed.to_string(),
            });
        }

        let mut candidates: Vec<&StatementSpan> = statements
            .iter()
            .filter(|s| s.start_line <= cursor_line && cursor_line <= s.end_line)
            .collect();
        if candidates.is_empty() {
            return None;
        }

        if let Some(hit) = candidates
            .iter()
            .find(|s| s.start_idx <= cursor_pos && cursor_pos <= s.end_idx)
        {
            return Some(hit.text.clone());
        }

        let mut previous: Option<&StatementSpan> = None;
        for candidate in &candidates {
            if candidate.end_idx < cursor_pos {
                if previous
                    .map(|p| candidate.end_idx > p.end_idx)
                    .unwrap_or(true)
                {
                    previous = Some(*candidate);
                }
            }
        }

        if let Some(prev) = previous {
            return Some(prev.text.clone());
        }

        candidates.sort_by_key(|s| s.start_idx);
        candidates.first().map(|s| s.text.clone())
    }

    /// Enable DBMS_OUTPUT for the session
    #[allow(dead_code)]
    pub fn enable_dbms_output(conn: &Connection, buffer_size: u32) -> Result<(), OracleError> {
        let sql = format!("BEGIN DBMS_OUTPUT.ENABLE({}); END;", buffer_size);
        match conn.execute(&sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        Ok(())
    }

    /// Disable DBMS_OUTPUT for the session
    #[allow(dead_code)]
    pub fn disable_dbms_output(conn: &Connection) -> Result<(), OracleError> {
        match conn.execute("BEGIN DBMS_OUTPUT.DISABLE; END;", &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        Ok(())
    }

    /// Get DBMS_OUTPUT lines using a simple approach
    #[allow(dead_code)]
    pub fn get_dbms_output(_conn: &Connection) -> Result<Vec<String>, OracleError> {
        // Use a PL/SQL block that collects all output into a temporary table or returns via cursor
        // For simplicity, we'll use DBMS_OUTPUT.GET_LINES via anonymous block
        let _sql = r#"
            SELECT column_value
            FROM TABLE(
                CAST(
                    (
                        SELECT COLLECT(column_value)
                        FROM TABLE(
                            (
                                SELECT CAST(MULTISET(
                                    SELECT DBMS_OUTPUT.GET_LINE(column_value, :status)
                                    FROM DUAL
                                    CONNECT BY LEVEL <= 10000 AND :status = 0
                                ) AS SYS.ODCIVARCHAR2LIST)
                                FROM DUAL
                            )
                        )
                    ) AS SYS.ODCIVARCHAR2LIST
                )
            )
        "#;

        // Simpler approach: read lines in a loop
        let lines = Vec::new();
        let max_lines = 10000;

        for _ in 0..max_lines {
            // Use a query to check if there's output
            let _check_sql = r#"
                SELECT * FROM (
                    SELECT 1 FROM DUAL WHERE 1=0
                )
            "#;

            // Try getting one line at a time using DBMS_SQL or direct approach
            let _plsql = r#"
                DECLARE
                    v_line VARCHAR2(32767);
                    v_status INTEGER;
                BEGIN
                    DBMS_OUTPUT.GET_LINE(v_line, v_status);
                    IF v_status = 0 THEN
                        :line := v_line;
                        :done := 0;
                    ELSE
                        :line := NULL;
                        :done := 1;
                    END IF;
                END;
            "#;

            // This approach requires bind variables which can be tricky
            // Let's use a workaround with a helper table or serveroutput
            break;
        }

        Ok(lines)
    }

    /// Execute with DBMS_OUTPUT capture (simplified version)
    /// Note: Full DBMS_OUTPUT capture requires session-level setup
    #[allow(dead_code)]
    pub fn execute_with_output(
        conn: &Connection,
        sql: &str,
    ) -> Result<(QueryResult, Vec<String>), OracleError> {
        // Enable DBMS_OUTPUT before execution
        let _ = Self::enable_dbms_output(conn, 1000000);

        // Execute the query
        let result = match Self::execute_batch(conn, sql) {
            Ok(result) => result,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        // For now, return empty output - full implementation needs bind variable support
        let output: Vec<String> = Vec::new();

        Ok((result, output))
    }

    fn execute_select(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let result_set = match stmt.query(&[]) {
            Ok(result_set) => result_set,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let column_info: Vec<ColumnInfo> = result_set
            .column_info()
            .iter()
            .map(|col| ColumnInfo {
                name: col.name().to_string(),
                data_type: format!("{:?}", col.oracle_type()),
            })
            .collect();

        let mut rows: Vec<Vec<String>> = Vec::new();

        for row_result in result_set {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            rows.push(row_data);
        }

        let execution_time = start.elapsed();
        Ok(QueryResult::new_select(
            sql,
            column_info,
            rows,
            execution_time,
        ))
    }

    pub fn execute_select_streaming<F, G>(
        conn: &Connection,
        sql: &str,
        on_select_start: &mut F,
        on_row: &mut G,
    ) -> Result<QueryResult, OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>),
    {
        let start = Instant::now();
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let result_set = match stmt.query(&[]) {
            Ok(result_set) => result_set,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let column_info: Vec<ColumnInfo> = result_set
            .column_info()
            .iter()
            .map(|col| ColumnInfo {
                name: col.name().to_string(),
                data_type: format!("{:?}", col.oracle_type()),
            })
            .collect();

        on_select_start(&column_info);

        let mut row_count = 0usize;

        for row_result in result_set {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            on_row(row_data);
            row_count += 1;
        }

        let execution_time = start.elapsed();
        Ok(QueryResult::new_select_streamed(
            sql,
            column_info,
            row_count,
            execution_time,
        ))
    }

    fn execute_dml(
        conn: &Connection,
        sql: &str,
        start: Instant,
        statement_type: &str,
    ) -> Result<QueryResult, OracleError> {
        let stmt = match conn.execute(sql, &[]) {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let affected_rows = match stmt.row_count() {
            Ok(affected_rows) => affected_rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let execution_time = start.elapsed();
        Ok(QueryResult::new_dml(
            sql,
            affected_rows,
            execution_time,
            statement_type,
        ))
    }

    fn execute_ddl(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        match conn.execute(sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        let execution_time = start.elapsed();

        // Determine the DDL type for better messaging
        let sql_upper = sql.to_uppercase();
        let message = if sql_upper.starts_with("CREATE") {
            if sql_upper.contains(" TABLE ") {
                "Table created"
            } else if sql_upper.contains(" VIEW ") {
                "View created"
            } else if sql_upper.contains(" INDEX ") {
                "Index created"
            } else if sql_upper.contains(" PROCEDURE ") {
                "Procedure created"
            } else if sql_upper.contains(" FUNCTION ") {
                "Function created"
            } else if sql_upper.contains(" PACKAGE ") {
                "Package created"
            } else if sql_upper.contains(" TRIGGER ") {
                "Trigger created"
            } else if sql_upper.contains(" SEQUENCE ") {
                "Sequence created"
            } else if sql_upper.contains(" SYNONYM ") {
                "Synonym created"
            } else if sql_upper.contains(" TYPE ") {
                "Type created"
            } else {
                "Object created"
            }
        } else if sql_upper.starts_with("ALTER") {
            "Object altered"
        } else if sql_upper.starts_with("DROP") {
            "Object dropped"
        } else if sql_upper.starts_with("TRUNCATE") {
            "Table truncated"
        } else if sql_upper.starts_with("GRANT") {
            "Grant succeeded"
        } else if sql_upper.starts_with("REVOKE") {
            "Revoke succeeded"
        } else if sql_upper.starts_with("COMMENT") {
            "Comment added"
        } else {
            "Statement executed successfully"
        };

        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: message.to_string(),
            is_select: false,
        })
    }

    /// Execute a PL/SQL anonymous block (BEGIN...END or DECLARE...BEGIN...END)
    fn execute_plsql_block(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        match conn.execute(sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        let execution_time = start.elapsed();
        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "PL/SQL procedure successfully completed".to_string(),
            is_select: false,
        })
    }

    /// Execute a CALL statement (standard SQL procedure call)
    fn execute_call(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        match conn.execute(sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        let execution_time = start.elapsed();
        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "Call completed".to_string(),
            is_select: false,
        })
    }

    /// Execute EXEC/EXECUTE statement (SQL*Plus style procedure call)
    /// Converts "EXEC procedure_name(args)" to "BEGIN procedure_name(args); END;"
    fn execute_exec(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        // Remove EXEC or EXECUTE keyword and convert to PL/SQL block
        let sql_trimmed = sql.trim();
        let proc_call = if sql_trimmed.to_uppercase().starts_with("EXECUTE ") {
            &sql_trimmed[8..] // Remove "EXECUTE "
        } else if sql_trimmed.to_uppercase().starts_with("EXEC ") {
            &sql_trimmed[5..] // Remove "EXEC "
        } else {
            sql_trimmed
        };

        let plsql = format!("BEGIN {}; END;", proc_call.trim().trim_end_matches(';'));
        match conn.execute(&plsql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }
        let execution_time = start.elapsed();
        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "PL/SQL procedure successfully completed".to_string(),
            is_select: false,
        })
    }

    pub fn get_explain_plan(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let explain_sql = format!("EXPLAIN PLAN FOR {}", sql);
        match conn.execute(&explain_sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        }

        let plan_sql =
            "SELECT plan_table_output FROM TABLE(DBMS_XPLAN.DISPLAY('PLAN_TABLE', NULL, 'ALL'))";
        let mut stmt = match conn.statement(plan_sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut plan_lines: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let line: Option<String> = match row.get(0) {
                Ok(line) => line,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            if let Some(l) = line {
                plan_lines.push(l);
            }
        }

        Ok(plan_lines)
    }
}

pub struct ObjectBrowser;

impl ObjectBrowser {
    pub fn get_tables(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT table_name FROM user_tables ORDER BY table_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_views(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT view_name FROM user_views ORDER BY view_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_procedures(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT object_name FROM user_procedures WHERE object_type = 'PROCEDURE' ORDER BY object_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_functions(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT object_name FROM user_procedures WHERE object_type = 'FUNCTION' ORDER BY object_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_sequences(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT sequence_name FROM user_sequences ORDER BY sequence_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_packages(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT object_name FROM user_objects WHERE object_type = 'PACKAGE' ORDER BY object_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_package_procedures(
        conn: &Connection,
        package_name: &str,
    ) -> Result<Vec<String>, OracleError> {
        let sql = r#"
            SELECT procedure_name
            FROM user_procedures
            WHERE object_type = 'PACKAGE'
              AND object_name = :1
              AND procedure_name IS NOT NULL
            ORDER BY procedure_name
        "#;
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[&package_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut procedures: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            procedures.push(name);
        }

        Ok(procedures)
    }

    #[allow(dead_code)]
    pub fn get_table_columns(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<ColumnInfo>, OracleError> {
        let sql = "SELECT column_name, data_type FROM user_tab_columns WHERE table_name = :1 ORDER BY column_id";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut columns: Vec<ColumnInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let data_type: String = match row.get(1) {
                Ok(data_type) => data_type,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            columns.push(ColumnInfo { name, data_type });
        }

        Ok(columns)
    }

    fn get_object_list(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut objects: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            objects.push(name);
        }

        Ok(objects)
    }

    /// Get detailed column info for a table
    pub fn get_table_structure(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<TableColumnDetail>, OracleError> {
        let sql = r#"
            SELECT
                c.column_name,
                c.data_type,
                c.data_length,
                c.data_precision,
                c.data_scale,
                c.nullable,
                c.data_default,
                (SELECT 'PK' FROM user_cons_columns cc
                 JOIN user_constraints con ON cc.constraint_name = con.constraint_name
                 WHERE con.constraint_type = 'P'
                 AND cc.table_name = c.table_name
                 AND cc.column_name = c.column_name
                 AND ROWNUM = 1) as is_pk
            FROM user_tab_columns c
            WHERE c.table_name = :1
            ORDER BY c.column_id
        "#;

        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut columns: Vec<TableColumnDetail> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name = match row.get(0) {
                Ok(name) => name,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let data_type = match row.get(1) {
                Ok(data_type) => data_type,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let data_length = match row.get::<_, Option<i32>>(2) {
                Ok(value) => value.unwrap_or(0),
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let data_precision = match row.get::<_, Option<i32>>(3) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let data_scale = match row.get::<_, Option<i32>>(4) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let nullable = match row.get::<_, String>(5) {
                Ok(value) => value == "Y",
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let default_value = match row.get(6) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let is_primary_key = match row.get::<_, Option<String>>(7) {
                Ok(value) => value.is_some(),
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            columns.push(TableColumnDetail {
                name,
                data_type,
                data_length,
                data_precision,
                data_scale,
                nullable,
                default_value,
                is_primary_key,
            });
        }

        Ok(columns)
    }

    /// Get indexes for a table
    pub fn get_table_indexes(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<IndexInfo>, OracleError> {
        let sql = r#"
            SELECT
                i.index_name,
                i.uniqueness,
                LISTAGG(ic.column_name, ', ') WITHIN GROUP (ORDER BY ic.column_position) as columns
            FROM user_indexes i
            JOIN user_ind_columns ic ON i.index_name = ic.index_name
            WHERE i.table_name = :1
            GROUP BY i.index_name, i.uniqueness
            ORDER BY i.index_name
        "#;

        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut indexes: Vec<IndexInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name = match row.get(0) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let is_unique = match row.get::<_, String>(1) {
                Ok(value) => value == "UNIQUE",
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let columns = match row.get(2) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            indexes.push(IndexInfo {
                name,
                is_unique,
                columns,
            });
        }

        Ok(indexes)
    }

    /// Get constraints for a table
    pub fn get_table_constraints(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<ConstraintInfo>, OracleError> {
        let sql = r#"
            SELECT
                c.constraint_name,
                c.constraint_type,
                LISTAGG(cc.column_name, ', ') WITHIN GROUP (ORDER BY cc.position) as columns,
                c.r_constraint_name,
                (SELECT table_name FROM user_constraints WHERE constraint_name = c.r_constraint_name) as ref_table
            FROM user_constraints c
            LEFT JOIN user_cons_columns cc ON c.constraint_name = cc.constraint_name
            WHERE c.table_name = :1
            GROUP BY c.constraint_name, c.constraint_type, c.r_constraint_name
            ORDER BY c.constraint_type, c.constraint_name
        "#;

        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };

        let mut constraints: Vec<ConstraintInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let constraint_type: String = match row.get(1) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let name = match row.get(0) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let columns = match row.get::<_, Option<String>>(2) {
                Ok(value) => value.unwrap_or_default(),
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            let ref_table = match row.get(4) {
                Ok(value) => value,
                Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
            };
            constraints.push(ConstraintInfo {
                name,
                constraint_type: match constraint_type.as_str() {
                    "P" => "PRIMARY KEY".to_string(),
                    "R" => "FOREIGN KEY".to_string(),
                    "U" => "UNIQUE".to_string(),
                    "C" => "CHECK".to_string(),
                    _ => constraint_type,
                },
                columns,
                ref_table,
            });
        }

        Ok(constraints)
    }

    /// Generate DDL for a table
    pub fn get_table_ddl(conn: &Connection, table_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('TABLE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let row = match stmt.query_row(&[&table_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        Ok(ddl)
    }

    /// Generate DDL for a view
    pub fn get_view_ddl(conn: &Connection, view_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('VIEW', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let row = match stmt.query_row(&[&view_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        Ok(ddl)
    }

    /// Generate DDL for a procedure
    pub fn get_procedure_ddl(conn: &Connection, proc_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('PROCEDURE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let row = match stmt.query_row(&[&proc_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        Ok(ddl)
    }

    /// Generate DDL for a function
    pub fn get_function_ddl(conn: &Connection, func_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('FUNCTION', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let row = match stmt.query_row(&[&func_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        Ok(ddl)
    }

    /// Generate DDL for a sequence
    pub fn get_sequence_ddl(conn: &Connection, seq_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('SEQUENCE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let row = match stmt.query_row(&[&seq_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => { eprintln!("Database operation failed: {err}"); return Err(err); },
        };
        Ok(ddl)
    }
}

/// Detailed column information for table structure
#[derive(Debug, Clone)]
pub struct TableColumnDetail {
    pub name: String,
    pub data_type: String,
    pub data_length: i32,
    pub data_precision: Option<i32>,
    pub data_scale: Option<i32>,
    pub nullable: bool,
    #[allow(dead_code)]
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}

impl TableColumnDetail {
    pub fn get_type_display(&self) -> String {
        match self.data_type.as_str() {
            "NUMBER" => {
                if let (Some(p), Some(s)) = (self.data_precision, self.data_scale) {
                    if s > 0 {
                        format!("NUMBER({},{})", p, s)
                    } else {
                        format!("NUMBER({})", p)
                    }
                } else if let Some(p) = self.data_precision {
                    format!("NUMBER({})", p)
                } else {
                    "NUMBER".to_string()
                }
            }
            "VARCHAR2" | "CHAR" | "NVARCHAR2" | "NCHAR" => {
                format!("{}({})", self.data_type, self.data_length)
            }
            _ => self.data_type.clone(),
        }
    }
}

/// Index information
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub is_unique: bool,
    pub columns: String,
}

/// Constraint information
#[derive(Debug, Clone)]
pub struct ConstraintInfo {
    pub name: String,
    pub constraint_type: String,
    pub columns: String,
    pub ref_table: Option<String>,
}
