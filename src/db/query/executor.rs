use oracle::sql_type::{OracleType, RefCursor};
use oracle::{Connection, Error as OracleError, Row, Statement};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::db::session::{BindDataType, BindValue, CompiledObject, SessionState};

use super::{ColumnInfo, ProcedureArgument, QueryResult, ResolvedBind, ScriptItem};

pub struct QueryExecutor;

impl QueryExecutor {
    fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
        let idx = index.min(text.len());
        if text.is_char_boundary(idx) {
            return idx;
        }
        text.char_indices()
            .map(|(pos, _)| pos)
            .take_while(|pos| *pos < idx)
            .last()
            .unwrap_or(0)
    }

    /// Check if the SQL is a CREATE [OR REPLACE] TRIGGER statement.
    /// Used to skip :NEW and :OLD pseudo-records from bind scanning.
    pub(crate) fn is_create_trigger(sql: &str) -> bool {
        let cleaned = Self::strip_leading_comments(sql);
        let upper = cleaned.to_uppercase();
        let tokens: Vec<&str> = upper.split_whitespace().collect();

        // Match patterns:
        // CREATE TRIGGER ...
        // CREATE OR REPLACE TRIGGER ...
        // CREATE OR REPLACE EDITIONABLE TRIGGER ...
        // CREATE OR REPLACE NONEDITIONABLE TRIGGER ...
        // CREATE EDITIONABLE TRIGGER ...
        // CREATE NONEDITIONABLE TRIGGER ...
        if tokens.is_empty() {
            return false;
        }
        if tokens[0] != "CREATE" {
            return false;
        }

        for token in tokens.iter().skip(1) {
            match *token {
                "OR" | "REPLACE" | "EDITIONABLE" | "NONEDITIONABLE" => continue,
                "TRIGGER" => return true,
                _ => return false,
            }
        }
        false
    }

    pub(crate) fn extract_bind_names(sql: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // In CREATE TRIGGER statements, :NEW and :OLD are pseudo-records, not bind variables
        let is_trigger = Self::is_create_trigger(sql);

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut in_q_quote = false;
        let mut q_quote_end: Option<char> = None;

        let chars: Vec<char> = sql.chars().collect();
        let len = chars.len();
        let mut i = 0usize;

        while i < len {
            let c = chars[i];
            let next = if i + 1 < len {
                Some(chars[i + 1])
            } else {
                None
            };
            let next2 = if i + 2 < len {
                Some(chars[i + 2])
            } else {
                None
            };

            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_q_quote {
                if Some(c) == q_quote_end && next == Some('\'') {
                    in_q_quote = false;
                    q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                if c == '\'' {
                    if next == Some('\'') {
                        i += 2;
                        continue;
                    }
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                if c == '"' {
                    if next == Some('"') {
                        i += 2;
                        continue;
                    }
                    in_double_quote = false;
                }
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                in_line_comment = true;
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }

            // Handle nq'[...]' (National Character q-quoted strings)
            if (c == 'n' || c == 'N')
                && (next == Some('q') || next == Some('Q'))
                && i + 2 < len
                && chars[i + 2] == '\''
            {
                if let Some(&delimiter) = chars.get(i + 3) {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 4;
                    continue;
                }
            }

            // Handle q'[...]' (q-quoted strings)
            if (c == 'q' || c == 'Q') && next == Some('\'') {
                if let Some(delimiter) = next2 {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 3;
                    continue;
                }
            }

            if c == '\'' {
                in_single_quote = true;
                i += 1;
                continue;
            }

            if c == '"' {
                in_double_quote = true;
                i += 1;
                continue;
            }

            if c == ':' {
                let prev = if i > 0 { Some(chars[i - 1]) } else { None };
                if prev == Some(':') {
                    i += 1;
                    continue;
                }

                if let Some(nc) = next {
                    if nc.is_ascii_digit() {
                        let mut j = i + 1;
                        while j < len && chars[j].is_ascii_digit() {
                            j += 1;
                        }
                        let name = chars[i + 1..j].iter().collect::<String>();
                        let normalized = SessionState::normalize_name(&name);
                        if seen.insert(normalized.clone()) {
                            names.push(normalized);
                        }
                        i = j;
                        continue;
                    }

                    if nc.is_ascii_alphanumeric() || nc == '_' || nc == '$' || nc == '#' {
                        let mut j = i + 1;
                        while j < len {
                            let ch = chars[j];
                            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' || ch == '#' {
                                j += 1;
                            } else {
                                break;
                            }
                        }
                        let name = chars[i + 1..j].iter().collect::<String>();
                        let normalized = SessionState::normalize_name(&name);

                        // In CREATE TRIGGER, skip :NEW and :OLD pseudo-records
                        if is_trigger {
                            let upper_name = normalized.to_uppercase();
                            if upper_name == "NEW" || upper_name == "OLD" {
                                i = j;
                                continue;
                            }
                        }

                        if seen.insert(normalized.clone()) {
                            names.push(normalized);
                        }
                        i = j;
                        continue;
                    }
                }
            }

            i += 1;
        }

        names
    }

    pub fn resolve_binds(sql: &str, session: &SessionState) -> Result<Vec<ResolvedBind>, String> {
        let names = Self::extract_bind_names(sql);
        if names.is_empty() {
            return Ok(Vec::new());
        }

        let mut resolved: Vec<ResolvedBind> = Vec::new();
        for name in names {
            let key = SessionState::normalize_name(&name);
            let bind = session.binds.get(&key).ok_or_else(|| {
                format!(
                    "Bind variable :{} is not defined. Use VAR to declare it.",
                    name
                )
            })?;

            let value = match &bind.value {
                BindValue::Scalar(val) => val.clone(),
                BindValue::Cursor(_) => None,
            };

            resolved.push(ResolvedBind {
                name: key,
                data_type: bind.data_type.clone(),
                value,
            });
        }

        Ok(resolved)
    }

    fn bind_statement(stmt: &mut Statement, binds: &[ResolvedBind]) -> Result<(), OracleError> {
        for bind in binds {
            match bind.data_type {
                BindDataType::RefCursor => {
                    stmt.bind(bind.name.as_str(), &OracleType::RefCursor)?;
                }
                _ => {
                    let oratype = bind.data_type.oracle_type();
                    match bind.value.as_ref() {
                        Some(value) => {
                            stmt.bind(bind.name.as_str(), &(value, &oratype))?;
                        }
                        None => {
                            stmt.bind(bind.name.as_str(), &oratype)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn execute_with_binds(
        conn: &Connection,
        sql: &str,
        binds: &[ResolvedBind],
    ) -> Result<Statement, OracleError> {
        let mut stmt = conn.statement(sql).build()?;
        Self::bind_statement(&mut stmt, binds)?;
        stmt.execute(&[])?;
        Ok(stmt)
    }

    pub(crate) fn fetch_scalar_bind_updates(
        stmt: &Statement,
        binds: &[ResolvedBind],
    ) -> Result<Vec<(String, BindValue)>, OracleError> {
        let mut updates = Vec::new();
        for bind in binds {
            if matches!(bind.data_type, BindDataType::RefCursor) {
                continue;
            }
            let value: Option<String> = stmt.bind_value(bind.name.as_str())?;
            updates.push((bind.name.clone(), BindValue::Scalar(value)));
        }
        Ok(updates)
    }

    pub(crate) fn extract_ref_cursors(
        stmt: &Statement,
        binds: &[ResolvedBind],
    ) -> Result<Vec<(String, RefCursor)>, OracleError> {
        let mut cursors = Vec::new();
        for bind in binds {
            if !matches!(bind.data_type, BindDataType::RefCursor) {
                continue;
            }
            let cursor: Option<RefCursor> = stmt.bind_value(bind.name.as_str())?;
            if let Some(cursor) = cursor {
                cursors.push((bind.name.clone(), cursor));
            }
        }
        Ok(cursors)
    }

    pub(crate) fn extract_implicit_results(
        stmt: &Statement,
    ) -> Result<Vec<RefCursor>, OracleError> {
        let mut cursors = Vec::new();
        loop {
            match stmt.implicit_result()? {
                Some(cursor) => cursors.push(cursor),
                None => break,
            }
        }
        Ok(cursors)
    }

    fn exec_call_body(sql: &str) -> Option<String> {
        let cleaned = Self::strip_leading_comments(sql);
        let upper = cleaned.to_uppercase();
        let body = if upper.starts_with("EXECUTE ") {
            cleaned[8..].to_string()
        } else if upper.starts_with("EXEC ") {
            cleaned[5..].to_string()
        } else if upper.starts_with("CALL ") {
            cleaned[5..].to_string()
        } else {
            return None;
        };

        let body = body.trim().trim_end_matches(';').trim();
        if body.is_empty() {
            None
        } else {
            Some(body.to_string())
        }
    }

    pub fn normalize_exec_call(sql: &str) -> Option<String> {
        let cleaned = Self::strip_leading_comments(sql);
        let upper = cleaned.to_uppercase();
        if upper.starts_with("EXECUTE IMMEDIATE") || upper.starts_with("EXEC IMMEDIATE") {
            let body = cleaned.trim().trim_end_matches(';').trim();
            if body.is_empty() {
                return None;
            }
            return Some(format!("BEGIN {}; END;", body));
        }

        Self::exec_call_body(sql).map(|body| format!("BEGIN {}; END;", body))
    }

    pub fn check_named_positional_mix(sql: &str) -> Result<(), String> {
        let Some(body) = Self::exec_call_body(sql) else {
            return Ok(());
        };

        let Some(args) = Self::extract_call_args(&body) else {
            return Ok(());
        };

        let args_list = Self::split_call_args(&args);
        let mut has_named = false;

        for arg in args_list {
            if arg.trim().is_empty() {
                continue;
            }
            if Self::arg_has_named_arrow(&arg) {
                has_named = true;
            } else {
                if has_named {
                    return Err("Named and positional parameters cannot be mixed.".to_string());
                }
            }
        }

        Ok(())
    }

    fn extract_call_args(call_sql: &str) -> Option<String> {
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut in_q_quote = false;
        let mut q_quote_end: Option<char> = None;
        let mut depth = 0usize;
        let mut start: Option<usize> = None;

        let chars: Vec<char> = call_sql.chars().collect();
        let len = chars.len();
        let mut i = 0usize;

        while i < len {
            let c = chars[i];
            let next = if i + 1 < len {
                Some(chars[i + 1])
            } else {
                None
            };
            let next2 = if i + 2 < len {
                Some(chars[i + 2])
            } else {
                None
            };

            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_q_quote {
                if Some(c) == q_quote_end && next == Some('\'') {
                    in_q_quote = false;
                    q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                if c == '\'' {
                    if next == Some('\'') {
                        i += 2;
                        continue;
                    }
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                if c == '"' {
                    if next == Some('"') {
                        i += 2;
                        continue;
                    }
                    in_double_quote = false;
                }
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                in_line_comment = true;
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }

            // Handle nq'[...]' (National Character q-quoted strings)
            if (c == 'n' || c == 'N')
                && (next == Some('q') || next == Some('Q'))
                && i + 2 < len
                && chars[i + 2] == '\''
            {
                if let Some(&delimiter) = chars.get(i + 3) {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 4;
                    continue;
                }
            }

            // Handle q'[...]' (q-quoted strings)
            if (c == 'q' || c == 'Q') && next == Some('\'') {
                if let Some(delimiter) = next2 {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 3;
                    continue;
                }
            }

            if c == '\'' {
                in_single_quote = true;
                i += 1;
                continue;
            }

            if c == '"' {
                in_double_quote = true;
                i += 1;
                continue;
            }

            if c == '(' {
                if depth == 0 {
                    start = Some(i + 1);
                }
                depth += 1;
                i += 1;
                continue;
            }

            if c == ')' {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let start_idx = start.unwrap_or(0);
                        return Some(chars[start_idx..i].iter().collect::<String>());
                    }
                }
                i += 1;
                continue;
            }

            i += 1;
        }

        None
    }

    fn split_call_args(args: &str) -> Vec<String> {
        let mut results = Vec::new();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_q_quote = false;
        let mut q_quote_end: Option<char> = None;
        let mut depth = 0usize;

        let chars: Vec<char> = args.chars().collect();
        let len = chars.len();
        let mut i = 0usize;

        while i < len {
            let c = chars[i];
            let next = if i + 1 < len {
                Some(chars[i + 1])
            } else {
                None
            };
            let next2 = if i + 2 < len {
                Some(chars[i + 2])
            } else {
                None
            };

            if in_q_quote {
                current.push(c);
                if Some(c) == q_quote_end && next == Some('\'') {
                    current.push('\'');
                    in_q_quote = false;
                    q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                current.push(c);
                if c == '\'' {
                    if next == Some('\'') {
                        current.push('\'');
                        i += 2;
                        continue;
                    }
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                current.push(c);
                if c == '"' {
                    if next == Some('"') {
                        current.push('"');
                        i += 2;
                        continue;
                    }
                    in_double_quote = false;
                }
                i += 1;
                continue;
            }

            // Handle nq'[...]' (National Character q-quoted strings)
            if (c == 'n' || c == 'N')
                && (next == Some('q') || next == Some('Q'))
                && i + 2 < len
                && chars[i + 2] == '\''
            {
                if let Some(&delimiter) = chars.get(i + 3) {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    current.push(c);
                    current.push(chars[i + 1]);
                    current.push('\'');
                    current.push(delimiter);
                    i += 4;
                    continue;
                }
            }

            // Handle q'[...]' (q-quoted strings)
            if (c == 'q' || c == 'Q') && next == Some('\'') {
                if let Some(delimiter) = next2 {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    current.push(c);
                    current.push('\'');
                    current.push(delimiter);
                    i += 3;
                    continue;
                }
            }

            if c == '\'' {
                in_single_quote = true;
                current.push(c);
                i += 1;
                continue;
            }

            if c == '"' {
                in_double_quote = true;
                current.push(c);
                i += 1;
                continue;
            }

            if c == '(' {
                depth += 1;
                current.push(c);
                i += 1;
                continue;
            }

            if c == ')' {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(c);
                i += 1;
                continue;
            }

            if c == ',' && depth == 0 {
                results.push(current.trim().to_string());
                current.clear();
                i += 1;
                continue;
            }

            current.push(c);
            i += 1;
        }

        if !current.trim().is_empty() {
            results.push(current.trim().to_string());
        }

        results
    }

    fn arg_has_named_arrow(arg: &str) -> bool {
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_q_quote = false;
        let mut q_quote_end: Option<char> = None;

        let chars: Vec<char> = arg.chars().collect();
        let len = chars.len();
        let mut i = 0usize;

        while i < len {
            let c = chars[i];
            let next = if i + 1 < len {
                Some(chars[i + 1])
            } else {
                None
            };
            let next2 = if i + 2 < len {
                Some(chars[i + 2])
            } else {
                None
            };

            if in_q_quote {
                if Some(c) == q_quote_end && next == Some('\'') {
                    in_q_quote = false;
                    q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                if c == '\'' {
                    if next == Some('\'') {
                        i += 2;
                        continue;
                    }
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                if c == '"' {
                    if next == Some('"') {
                        i += 2;
                        continue;
                    }
                    in_double_quote = false;
                }
                i += 1;
                continue;
            }

            // Handle nq'[...]' (National Character q-quoted strings)
            if (c == 'n' || c == 'N')
                && (next == Some('q') || next == Some('Q'))
                && i + 2 < len
                && chars[i + 2] == '\''
            {
                if let Some(&delimiter) = chars.get(i + 3) {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 4;
                    continue;
                }
            }

            // Handle q'[...]' (q-quoted strings)
            if (c == 'q' || c == 'Q') && next == Some('\'') {
                if let Some(delimiter) = next2 {
                    in_q_quote = true;
                    q_quote_end = Some(match delimiter {
                        '[' => ']',
                        '(' => ')',
                        '{' => '}',
                        '<' => '>',
                        other => other,
                    });
                    i += 3;
                    continue;
                }
            }

            if c == '\'' {
                in_single_quote = true;
                i += 1;
                continue;
            }

            if c == '"' {
                in_double_quote = true;
                i += 1;
                continue;
            }

            if c == '=' && next == Some('>') {
                return true;
            }

            i += 1;
        }

        false
    }

    /// Execute a single SQL statement
    pub fn execute(conn: &Connection, sql: &str) -> Result<QueryResult, OracleError> {
        let sql_trimmed = sql.trim();
        // Remove trailing semicolon if present (but keep for PL/SQL blocks)
        let sql_clean = if matches!(
            Self::leading_keyword(sql_trimmed).as_deref(),
            Some("BEGIN") | Some("DECLARE")
        ) {
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
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
            Ok(QueryResult {
                sql: sql_clean,
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: start.elapsed(),
                message: "Commit complete".to_string(),
                is_select: false,
                success: true,
            })
        } else if sql_upper.starts_with("ROLLBACK") {
            match conn.rollback() {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
            Ok(QueryResult {
                sql: sql_clean,
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: start.elapsed(),
                message: "Rollback complete".to_string(),
                is_select: false,
                success: true,
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
                success: true,
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
                success: true,
            })
        }
    }

    #[allow(dead_code)]
    pub fn execute_batch_streaming<F, G>(
        conn: &Connection,
        sql: &str,
        mut on_select_start: F,
        mut on_row: G,
    ) -> Result<(QueryResult, bool), OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>) -> bool,
    {
        let statements = Self::split_statements_with_blocks(sql);

        if statements.is_empty() {
            return Ok((
                QueryResult {
                    sql: sql.to_string(),
                    columns: vec![],
                    rows: vec![],
                    row_count: 0,
                    execution_time: Duration::from_secs(0),
                    message: "No statements to execute".to_string(),
                    is_select: false,
                    success: true,
                },
                false,
            ));
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

            return Ok((Self::execute(conn, statement)?, false));
        }

        Ok((Self::execute_batch(conn, sql)?, false))
    }

    /// Split SQL text into individual statements by semicolons.
    /// Handles quoted strings, comments, and PL/SQL blocks (BEGIN/END, DECLARE).
    pub fn split_statements_with_blocks(sql: &str) -> Vec<String> {
        Self::split_script_items(sql)
            .into_iter()
            .filter_map(|item| match item {
                ScriptItem::Statement(statement) => Some(statement),
                ScriptItem::ToolCommand(_) => None,
            })
            .collect()
    }

    /// Return the statement containing the cursor position (character index).
    pub fn statement_at_cursor(sql: &str, cursor_pos: usize) -> Option<String> {
        if sql.trim().is_empty() {
            return None;
        }

        #[derive(Clone)]
        struct StatementSpan {
            start: usize,
            end: usize,
            text: String,
        }

        let cursor_pos = Self::clamp_to_char_boundary(sql, cursor_pos);
        let line_start = sql[..cursor_pos]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let line_end = sql[cursor_pos..]
            .find('\n')
            .map(|idx| cursor_pos + idx)
            .unwrap_or_else(|| sql.len());
        let line = &sql[line_start..line_end];
        let trimmed_line = line.trim();

        if !trimmed_line.is_empty() {
            if trimmed_line == "/" {
                let mut spans: Vec<StatementSpan> = Vec::new();
                let mut search_pos = 0usize;
                for item in Self::split_script_items(sql) {
                    if let ScriptItem::Statement(stmt) = item {
                        let stmt = stmt.trim();
                        if stmt.is_empty() {
                            continue;
                        }
                        let remaining = &sql[search_pos..];
                        let leading_ws = remaining.len() - remaining.trim_start().len();
                        if let Some(found) = remaining.trim_start().find(stmt) {
                            let start = search_pos + leading_ws + found;
                            let end = start + stmt.len();
                            spans.push(StatementSpan {
                                start,
                                end,
                                text: stmt.to_string(),
                            });
                            search_pos = end;
                        }
                    }
                }
                if let Some(prev) = spans.iter().filter(|span| span.end <= line_start).last() {
                    return Some(prev.text.clone());
                }
            }

            if Self::parse_tool_command(trimmed_line).is_some() {
                return Some(trimmed_line.to_string());
            }
        }

        let mut spans: Vec<StatementSpan> = Vec::new();
        let mut search_pos = 0usize;
        for item in Self::split_script_items(sql) {
            if let ScriptItem::Statement(stmt) = item {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                let remaining = &sql[search_pos..];
                let leading_ws = remaining.len() - remaining.trim_start().len();
                if let Some(found) = remaining.trim_start().find(stmt) {
                    let start = search_pos + leading_ws + found;
                    let end = start + stmt.len();
                    spans.push(StatementSpan {
                        start,
                        end,
                        text: stmt.to_string(),
                    });
                    search_pos = end;
                }
            }
        }

        if spans.is_empty() {
            return None;
        }

        if let Some(span) = spans
            .iter()
            .find(|span| cursor_pos >= span.start && cursor_pos <= span.end)
        {
            return Some(span.text.clone());
        }

        let mut previous: Option<&StatementSpan> = None;
        for span in spans.iter() {
            if span.start > cursor_pos {
                return Some(previous.unwrap_or(span).text.clone());
            }
            previous = Some(span);
        }

        previous.map(|span| span.text.clone())
    }

    /// Enable DBMS_OUTPUT for the session
    /// If buffer_size is None, enables unlimited buffer (DBMS_OUTPUT.ENABLE(NULL))
    #[allow(dead_code)]
    pub fn enable_dbms_output(
        conn: &Connection,
        buffer_size: Option<u32>,
    ) -> Result<(), OracleError> {
        let sql = match buffer_size {
            Some(size) => format!("BEGIN DBMS_OUTPUT.ENABLE({}); END;", size),
            None => "BEGIN DBMS_OUTPUT.ENABLE(NULL); END;".to_string(),
        };
        match conn.execute(&sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }
        Ok(())
    }

    /// Disable DBMS_OUTPUT for the session
    #[allow(dead_code)]
    pub fn disable_dbms_output(conn: &Connection) -> Result<(), OracleError> {
        match conn.execute("BEGIN DBMS_OUTPUT.DISABLE; END;", &[]) {
            Ok(_stmt) => {}
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }
        Ok(())
    }

    /// Get DBMS_OUTPUT lines using DBMS_OUTPUT.GET_LINE in a loop.
    #[allow(dead_code)]
    pub fn get_dbms_output(conn: &Connection, max_lines: u32) -> Result<Vec<String>, OracleError> {
        let mut lines = Vec::new();
        let max_lines = max_lines.max(1);

        let mut stmt = match conn
            .statement("BEGIN DBMS_OUTPUT.GET_LINE(:line, :status); END;")
            .build()
        {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        stmt.bind("line", &OracleType::Varchar2(32767))?;
        stmt.bind("status", &OracleType::Number(0, 0))?;

        for _ in 0..max_lines {
            match stmt.execute(&[]) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
            let status: i32 = match stmt.bind_value("status") {
                Ok(val) => val,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            if status != 0 {
                break;
            }
            let line: Option<String> = match stmt.bind_value("line") {
                Ok(val) => val,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            lines.push(line.unwrap_or_default());
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
        let _ = Self::enable_dbms_output(conn, Some(1000000));

        // Execute the query
        let result = match Self::execute_batch(conn, sql) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let output = Self::get_dbms_output(conn, 10000).unwrap_or_default();

        Ok((result, output))
    }

    fn execute_select(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let result_set = match stmt.query(&[]) {
            Ok(result_set) => result_set,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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

    /// Execute a SELECT statement with streaming results.
    /// on_row returns true to continue, false to stop fetching.
    /// Returns (QueryResult, was_cancelled) tuple.
    pub fn execute_select_streaming<F, G>(
        conn: &Connection,
        sql: &str,
        on_select_start: &mut F,
        on_row: &mut G,
    ) -> Result<(QueryResult, bool), OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>) -> bool,
    {
        let start = Instant::now();
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let result_set = match stmt.query(&[]) {
            Ok(result_set) => result_set,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
        let mut cancelled = false;

        for row_result in result_set {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            let should_continue = on_row(row_data);
            row_count += 1;

            if !should_continue {
                cancelled = true;
                break;
            }
        }

        let execution_time = start.elapsed();
        Ok((
            QueryResult::new_select_streamed(sql, column_info, row_count, execution_time),
            cancelled,
        ))
    }

    pub fn execute_select_streaming_with_binds<F, G>(
        conn: &Connection,
        sql: &str,
        binds: &[ResolvedBind],
        on_select_start: &mut F,
        on_row: &mut G,
    ) -> Result<(QueryResult, bool), OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>) -> bool,
    {
        let start = Instant::now();
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        if let Err(err) = Self::bind_statement(&mut stmt, binds) {
            eprintln!("Database operation failed: {err}");
            return Err(err);
        }
        let result_set = match stmt.query(&[]) {
            Ok(result_set) => result_set,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
        let mut cancelled = false;

        for row_result in result_set {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            let should_continue = on_row(row_data);
            row_count += 1;

            if !should_continue {
                cancelled = true;
                break;
            }
        }

        let execution_time = start.elapsed();
        Ok((
            QueryResult::new_select_streamed(sql, column_info, row_count, execution_time),
            cancelled,
        ))
    }

    pub fn execute_ref_cursor_streaming<F, G>(
        cursor: &mut RefCursor,
        sql: &str,
        on_select_start: &mut F,
        on_row: &mut G,
    ) -> Result<(QueryResult, bool), OracleError>
    where
        F: FnMut(&[ColumnInfo]),
        G: FnMut(Vec<String>) -> bool,
    {
        let start = Instant::now();
        let result_set = match cursor.query() {
            Ok(result_set) => result_set,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
        let mut cancelled = false;

        for row_result in result_set {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            let should_continue = on_row(row_data);
            row_count += 1;

            if !should_continue {
                cancelled = true;
                break;
            }
        }

        let execution_time = start.elapsed();
        Ok((
            QueryResult::new_select_streamed(sql, column_info, row_count, execution_time),
            cancelled,
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let affected_rows = match stmt.row_count() {
            Ok(affected_rows) => affected_rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }
        let execution_time = start.elapsed();

        // Determine the DDL type for better messaging
        let sql_upper = sql.to_uppercase();
        let message = if sql_upper.starts_with("CREATE") {
            let obj_type = Self::parse_ddl_object_type(&sql_upper);
            format!("{} created", obj_type)
        } else if sql_upper.starts_with("ALTER") {
            let obj_type = Self::parse_ddl_object_type(&sql_upper);
            format!("{} altered", obj_type)
        } else if sql_upper.starts_with("DROP") {
            let obj_type = Self::parse_ddl_object_type(&sql_upper);
            format!("{} dropped", obj_type)
        } else if sql_upper.starts_with("TRUNCATE") {
            "Table truncated".to_string()
        } else if sql_upper.starts_with("GRANT") {
            "Grant succeeded".to_string()
        } else if sql_upper.starts_with("REVOKE") {
            "Revoke succeeded".to_string()
        } else if sql_upper.starts_with("COMMENT") {
            "Comment added".to_string()
        } else {
            "Statement executed successfully".to_string()
        };

        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message,
            is_select: false,
            success: true,
        })
    }

    /// Parse the object type from a DDL statement header.
    /// Only examines the leading tokens (CREATE/ALTER/DROP + modifiers + type keyword)
    /// to avoid false matches from keywords appearing in PL/SQL bodies.
    pub fn parse_ddl_object_type(sql_upper: &str) -> &'static str {
        let tokens: Vec<&str> = sql_upper.split_whitespace().collect();
        if tokens.len() < 2 {
            return "Object";
        }

        let mut idx = 1usize; // skip CREATE/ALTER/DROP/etc.

        // For CREATE statements, skip optional modifiers
        if tokens[0] == "CREATE" {
            // Skip "OR REPLACE"
            if tokens.get(idx).map_or(false, |t| *t == "OR")
                && tokens.get(idx + 1).map_or(false, |t| *t == "REPLACE")
            {
                idx += 2;
            }
            // Skip EDITIONABLE/NONEDITIONABLE
            if tokens
                .get(idx)
                .map_or(false, |t| *t == "EDITIONABLE" || *t == "NONEDITIONABLE")
            {
                idx += 1;
            }
            // Skip FORCE / NO FORCE (for views/synonyms)
            if tokens.get(idx).map_or(false, |t| *t == "NO")
                && tokens.get(idx + 1).map_or(false, |t| *t == "FORCE")
            {
                idx += 2;
            } else if tokens.get(idx).map_or(false, |t| *t == "FORCE") {
                idx += 1;
            }
        }

        match tokens.get(idx).copied() {
            Some("TABLE") | Some("GLOBAL") => "Table",
            Some("VIEW") | Some("MATERIALIZED") => "View",
            Some("INDEX") | Some("UNIQUE") | Some("BITMAP") => "Index",
            Some("PROCEDURE") => "Procedure",
            Some("FUNCTION") => "Function",
            Some("PACKAGE") => {
                if tokens.get(idx + 1).map_or(false, |t| *t == "BODY") {
                    "Package Body"
                } else {
                    "Package"
                }
            }
            Some("TRIGGER") => "Trigger",
            Some("SEQUENCE") => "Sequence",
            Some("SYNONYM") => "Synonym",
            Some("PUBLIC") => {
                if tokens.get(idx + 1).map_or(false, |t| *t == "SYNONYM") {
                    "Synonym"
                } else if tokens.get(idx + 1).map_or(false, |t| *t == "DATABASE") {
                    "Database Link"
                } else {
                    "Object"
                }
            }
            Some("TYPE") => {
                if tokens.get(idx + 1).map_or(false, |t| *t == "BODY") {
                    "Type Body"
                } else {
                    "Type"
                }
            }
            Some("DATABASE") => "Database Link",
            Some("DIRECTORY") => "Directory",
            Some("TABLESPACE") => "Tablespace",
            Some("USER") => "User",
            Some("ROLE") => "Role",
            _ => "Object",
        }
    }

    /// Execute a PL/SQL anonymous block (BEGIN...END or DECLARE...BEGIN...END)
    fn execute_plsql_block(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        match conn.execute(sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }
        let execution_time = start.elapsed();
        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "PL/SQL block executed successfully".to_string(),
            is_select: false,
            success: true,
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
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
            success: true,
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }
        let execution_time = start.elapsed();
        Ok(QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "PL/SQL block executed successfully".to_string(),
            is_select: false,
            success: true,
        })
    }

    pub fn parse_compiled_object(sql: &str) -> Option<CompiledObject> {
        let cleaned = Self::strip_leading_comments(sql);
        let tokens: Vec<String> = cleaned.split_whitespace().map(|t| t.to_string()).collect();
        if tokens.len() < 3 {
            return None;
        }

        if !tokens[0].eq_ignore_ascii_case("CREATE") {
            return None;
        }

        let mut idx = 1usize;
        if tokens
            .get(idx)
            .map(|t| t.eq_ignore_ascii_case("OR"))
            .unwrap_or(false)
            && tokens
                .get(idx + 1)
                .map(|t| t.eq_ignore_ascii_case("REPLACE"))
                .unwrap_or(false)
        {
            idx += 2;
        }

        if tokens
            .get(idx)
            .map(|t| {
                t.eq_ignore_ascii_case("EDITIONABLE") || t.eq_ignore_ascii_case("NONEDITIONABLE")
            })
            .unwrap_or(false)
        {
            idx += 1;
        }

        let mut object_type = tokens.get(idx)?.to_uppercase();
        idx += 1;

        if object_type == "PACKAGE" {
            if tokens
                .get(idx)
                .map(|t| t.eq_ignore_ascii_case("BODY"))
                .unwrap_or(false)
            {
                object_type = "PACKAGE BODY".to_string();
                idx += 1;
            }
        } else if object_type == "TYPE" {
            if tokens
                .get(idx)
                .map(|t| t.eq_ignore_ascii_case("BODY"))
                .unwrap_or(false)
            {
                object_type = "TYPE BODY".to_string();
                idx += 1;
            }
        }

        let tracked = matches!(
            object_type.as_str(),
            "PROCEDURE"
                | "FUNCTION"
                | "PACKAGE"
                | "PACKAGE BODY"
                | "TRIGGER"
                | "TYPE"
                | "TYPE BODY"
        );
        if !tracked {
            return None;
        }

        let name_token = tokens.get(idx)?.clone();
        let (owner, name) = if let Some(dot) = name_token.find('.') {
            let (owner_raw, name_raw) = name_token.split_at(dot);
            (
                Some(Self::normalize_object_name(owner_raw)),
                Self::normalize_object_name(name_raw.trim_start_matches('.')),
            )
        } else {
            (None, Self::normalize_object_name(&name_token))
        };

        Some(CompiledObject {
            owner,
            object_type,
            name,
        })
    }

    fn normalize_object_name(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            trimmed.trim_matches('"').to_string()
        } else {
            trimmed.to_uppercase()
        }
    }

    fn split_qualified_name(value: &str) -> (Option<String>, String) {
        let trimmed = value.trim();
        let mut in_quotes = false;
        let mut split_at: Option<usize> = None;
        for (idx, ch) in trimmed.char_indices() {
            if ch == '"' {
                in_quotes = !in_quotes;
            } else if ch == '.' && !in_quotes {
                split_at = Some(idx);
                break;
            }
        }

        if let Some(idx) = split_at {
            let (owner, name) = trimmed.split_at(idx);
            (
                Some(owner.trim().to_string()),
                name.trim_start_matches('.').trim().to_string(),
            )
        } else {
            (None, trimmed.to_string())
        }
    }

    /// Describe a table or view, optionally schema-qualified (owner.object).
    pub fn describe_object(
        conn: &Connection,
        object_name: &str,
    ) -> Result<Vec<TableColumnDetail>, OracleError> {
        let (owner_raw, name_raw) = Self::split_qualified_name(object_name);
        let name = Self::normalize_object_name(&name_raw);
        let owner = owner_raw.map(|value| Self::normalize_object_name(&value));

        let sql = if owner.is_some() {
            r#"
                SELECT
                    c.column_name,
                    c.data_type,
                    c.data_length,
                    c.data_precision,
                    c.data_scale,
                    c.nullable,
                    c.data_default,
                    (SELECT 'PK' FROM all_cons_columns cc
                     JOIN all_constraints con
                       ON cc.owner = con.owner
                      AND cc.constraint_name = con.constraint_name
                     WHERE con.constraint_type = 'P'
                       AND cc.owner = c.owner
                       AND cc.table_name = c.table_name
                       AND cc.column_name = c.column_name
                       AND ROWNUM = 1) as is_pk
                FROM all_tab_columns c
                WHERE c.owner = :1
                  AND c.table_name = :2
                ORDER BY c.column_id
            "#
        } else {
            r#"
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
            "#
        };

        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let rows = if let Some(owner) = owner.as_ref() {
            match stmt.query(&[owner, &name]) {
                Ok(rows) => rows,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
        } else {
            match stmt.query(&[&name]) {
                Ok(rows) => rows,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
        };

        let mut columns: Vec<TableColumnDetail> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name = match row.get(0) {
                Ok(name) => name,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_type = match row.get(1) {
                Ok(data_type) => data_type,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_length = match row.get::<_, Option<i32>>(2) {
                Ok(value) => value.unwrap_or(0),
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_precision = match row.get::<_, Option<i32>>(3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_scale = match row.get::<_, Option<i32>>(4) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let nullable = match row.get::<_, String>(5) {
                Ok(value) => value == "Y",
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let default_value = match row.get(6) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let is_primary_key = match row.get::<_, Option<String>>(7) {
                Ok(value) => value.is_some(),
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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

    pub fn fetch_compilation_errors(
        conn: &Connection,
        object: &CompiledObject,
    ) -> Result<Vec<Vec<String>>, OracleError> {
        let query_errors = |table: &str,
                            use_owner: bool|
         -> Result<Vec<Vec<String>>, OracleError> {
            let sql = if use_owner {
                format!(
                    "SELECT line, position, text FROM {} WHERE owner = :owner AND name = :name AND type = :type ORDER BY sequence",
                    table
                )
            } else {
                format!(
                    "SELECT line, position, text FROM {} WHERE name = :name AND type = :type ORDER BY sequence",
                    table
                )
            };

            let mut stmt = conn.statement(&sql).build()?;
            if use_owner {
                if let Some(owner) = &object.owner {
                    stmt.bind("owner", owner)?;
                }
            }
            stmt.bind("name", &object.name)?;
            stmt.bind("type", &object.object_type)?;

            let result_set = stmt.query(&[])?;
            let mut rows: Vec<Vec<String>> = Vec::new();
            for row_result in result_set {
                let row: Row = row_result?;
                let line: Option<String> = row.get(0).unwrap_or(None);
                let position: Option<String> = row.get(1).unwrap_or(None);
                let text: Option<String> = row.get(2).unwrap_or(None);
                rows.push(vec![
                    line.unwrap_or_default(),
                    position.unwrap_or_default(),
                    text.unwrap_or_default(),
                ]);
            }
            Ok(rows)
        };

        let rows = if object.owner.is_some() {
            match query_errors("ALL_ERRORS", true) {
                Ok(found) => found,
                Err(_) => query_errors("USER_ERRORS", false)?,
            }
        } else {
            query_errors("USER_ERRORS", false)?
        };

        Ok(rows)
    }

    pub fn get_explain_plan(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let explain_sql = format!("EXPLAIN PLAN FOR {}", sql);
        match conn.execute(&explain_sql, &[]) {
            Ok(_stmt) => {}
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        }

        let plan_sql =
            "SELECT plan_table_output FROM TABLE(DBMS_XPLAN.DISPLAY('PLAN_TABLE', NULL, 'ALL'))";
        let mut stmt = match conn.statement(plan_sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut plan_lines: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let line: Option<String> = match row.get(0) {
                Ok(line) => line,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            if let Some(l) = line {
                plan_lines.push(l);
            }
        }

        Ok(plan_lines)
    }
}

pub struct ObjectBrowser;

#[derive(Debug, Clone)]
pub struct SequenceInfo {
    pub name: String,
    pub min_value: String,
    pub max_value: String,
    pub increment_by: String,
    pub cycle_flag: String,
    pub order_flag: String,
    pub cache_size: String,
    pub last_number: String,
}

#[derive(Debug, Clone)]
pub struct PackageRoutine {
    pub name: String,
    pub routine_type: String,
}

impl ObjectBrowser {
    fn normalize_generated_ddl(ddl: String) -> String {
        let normalized_newlines = ddl.replace("\r\n", "\n");
        let trimmed = normalized_newlines.trim_matches('\n');
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.is_empty() {
            return String::new();
        }

        let common_indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.chars().take_while(|c| *c == ' ').count())
            .min()
            .unwrap_or(0);

        let mut out = String::with_capacity(trimmed.len());
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            if line.trim().is_empty() {
                continue;
            }
            let cut = common_indent.min(line.len());
            out.push_str(&line[cut..]);
        }
        out.trim_start_matches(|c| c == ' ' || c == '\t')
            .to_string()
    }

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

    pub fn get_sequence_info(
        conn: &Connection,
        seq_name: &str,
    ) -> Result<SequenceInfo, OracleError> {
        let sql = r#"
            SELECT
                sequence_name,
                TO_CHAR(min_value),
                TO_CHAR(max_value),
                TO_CHAR(increment_by),
                cycle_flag,
                order_flag,
                TO_CHAR(cache_size),
                TO_CHAR(last_number)
            FROM user_sequences
            WHERE sequence_name = :1
        "#;
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&seq_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let name: String = row.get(0)?;
        let min_value: String = row.get(1)?;
        let max_value: String = row.get(2)?;
        let increment_by: String = row.get(3)?;
        let cycle_flag: String = row.get(4)?;
        let order_flag: String = row.get(5)?;
        let cache_size: String = row.get(6)?;
        let last_number: String = row.get(7)?;

        Ok(SequenceInfo {
            name,
            min_value,
            max_value,
            increment_by,
            cycle_flag,
            order_flag,
            cache_size,
            last_number,
        })
    }

    pub fn get_packages(conn: &Connection) -> Result<Vec<String>, OracleError> {
        let sql = "SELECT object_name FROM user_objects WHERE object_type = 'PACKAGE' ORDER BY object_name";
        Self::get_object_list(conn, sql)
    }

    pub fn get_package_routines(
        conn: &Connection,
        package_name: &str,
    ) -> Result<Vec<PackageRoutine>, OracleError> {
        let sql = r#"
            SELECT
                p.procedure_name,
                CASE
                    WHEN EXISTS (
                        SELECT 1
                        FROM user_arguments a
                        WHERE a.package_name = p.object_name
                          AND a.object_name = p.procedure_name
                          AND a.position = 0
                          AND a.argument_name IS NULL
                    ) THEN 'FUNCTION'
                    ELSE 'PROCEDURE'
                END AS routine_type
            FROM (
                SELECT DISTINCT object_name, procedure_name
                FROM user_procedures
                WHERE object_type = 'PACKAGE'
                  AND object_name = :1
                  AND procedure_name IS NOT NULL
            ) p
            ORDER BY p.procedure_name
        "#;
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&package_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut routines: Vec<PackageRoutine> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let routine_type: String = match row.get(1) {
                Ok(routine_type) => routine_type,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            routines.push(PackageRoutine { name, routine_type });
        }

        Ok(routines)
    }

    pub fn get_procedure_arguments(
        conn: &Connection,
        procedure_name: &str,
    ) -> Result<Vec<ProcedureArgument>, OracleError> {
        Self::get_procedure_arguments_inner(conn, None, procedure_name)
    }

    pub fn get_package_procedure_arguments(
        conn: &Connection,
        package_name: &str,
        procedure_name: &str,
    ) -> Result<Vec<ProcedureArgument>, OracleError> {
        Self::get_procedure_arguments_inner(conn, Some(package_name), procedure_name)
    }

    fn get_procedure_arguments_inner(
        conn: &Connection,
        package_name: Option<&str>,
        procedure_name: &str,
    ) -> Result<Vec<ProcedureArgument>, OracleError> {
        let sql = if package_name.is_some() {
            r#"
            SELECT
                argument_name,
                position,
                sequence,
                data_type,
                in_out,
                data_length,
                data_precision,
                data_scale,
                type_owner,
                type_name,
                pls_type,
                overload,
                default_value
            FROM user_arguments
            WHERE package_name = :1
              AND object_name = :2
            ORDER BY NVL(overload, 0), position, sequence
            "#
        } else {
            r#"
            SELECT
                argument_name,
                position,
                sequence,
                data_type,
                in_out,
                data_length,
                data_precision,
                data_scale,
                type_owner,
                type_name,
                pls_type,
                overload,
                default_value
            FROM user_arguments
            WHERE package_name IS NULL
              AND object_name = :1
            ORDER BY NVL(overload, 0), position, sequence
            "#
        };

        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let rows = if let Some(pkg_name) = package_name {
            match stmt.query(&[&pkg_name.to_uppercase(), &procedure_name.to_uppercase()]) {
                Ok(rows) => rows,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
        } else {
            match stmt.query(&[&procedure_name.to_uppercase()]) {
                Ok(rows) => rows,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            }
        };

        let mut arguments: Vec<ProcedureArgument> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };

            let name: Option<String> = match row.get(0) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let position: i32 = match row.get(1) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let sequence: i32 = match row.get(2) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_type: Option<String> = match row.get(3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let in_out: Option<String> = match row.get(4) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_length: Option<i32> = match row.get(5) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_precision: Option<i32> = match row.get(6) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_scale: Option<i32> = match row.get(7) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let type_owner: Option<String> = match row.get(8) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let type_name: Option<String> = match row.get(9) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let pls_type: Option<String> = match row.get(10) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let overload: Option<i32> = match row.get(11) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let default_value: Option<String> = match row.get(12) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Failed to read default_value (ignored): {err}");
                    None
                }
            };

            arguments.push(ProcedureArgument {
                name,
                position,
                sequence,
                data_type,
                in_out,
                data_length,
                data_precision,
                data_scale,
                type_owner,
                type_name,
                pls_type,
                overload,
                default_value,
            });
        }

        Ok(arguments)
    }

    #[allow(dead_code)]
    pub fn get_table_columns(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<ColumnInfo>, OracleError> {
        let sql = "SELECT column_name, data_type FROM user_tab_columns WHERE table_name = :1 ORDER BY column_id";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut columns: Vec<ColumnInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_type: String = match row.get(1) {
                Ok(data_type) => data_type,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            columns.push(ColumnInfo { name, data_type });
        }

        Ok(columns)
    }

    fn get_object_list(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut objects: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name: String = match row.get(0) {
                Ok(name) => name,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut columns: Vec<TableColumnDetail> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name = match row.get(0) {
                Ok(name) => name,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_type = match row.get(1) {
                Ok(data_type) => data_type,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_length = match row.get::<_, Option<i32>>(2) {
                Ok(value) => value.unwrap_or(0),
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_precision = match row.get::<_, Option<i32>>(3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let data_scale = match row.get::<_, Option<i32>>(4) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let nullable = match row.get::<_, String>(5) {
                Ok(value) => value == "Y",
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let default_value = match row.get(6) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let is_primary_key = match row.get::<_, Option<String>>(7) {
                Ok(value) => value.is_some(),
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut indexes: Vec<IndexInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name = match row.get(0) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let is_unique = match row.get::<_, String>(1) {
                Ok(value) => value == "UNIQUE",
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let columns = match row.get(2) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&table_name.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut constraints: Vec<ConstraintInfo> = Vec::new();
        for row_result in rows {
            let row: Row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let constraint_type: String = match row.get(1) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let name = match row.get(0) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let columns = match row.get::<_, Option<String>>(2) {
                Ok(value) => value.unwrap_or_default(),
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let ref_table = match row.get(4) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
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
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&table_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Generate DDL for a view
    pub fn get_view_ddl(conn: &Connection, view_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('VIEW', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&view_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Generate DDL for a procedure
    pub fn get_procedure_ddl(conn: &Connection, proc_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('PROCEDURE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&proc_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Generate DDL for a function
    pub fn get_function_ddl(conn: &Connection, func_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('FUNCTION', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&func_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Generate DDL for a sequence
    pub fn get_sequence_ddl(conn: &Connection, seq_name: &str) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('SEQUENCE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&seq_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Generate DDL for a package specification
    pub fn get_package_spec_ddl(
        conn: &Connection,
        package_name: &str,
    ) -> Result<String, OracleError> {
        let sql = "SELECT DBMS_METADATA.GET_DDL('PACKAGE', :1) FROM DUAL";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&package_name.to_uppercase()]) {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let ddl: String = match row.get(0) {
            Ok(ddl) => ddl,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        Ok(Self::normalize_generated_ddl(ddl))
    }

    /// Get compilation errors for a compilable object (procedure, function, package, etc.)
    pub fn get_compilation_errors(
        conn: &Connection,
        object_name: &str,
        object_type: &str,
    ) -> Result<Vec<CompilationError>, OracleError> {
        let sql = "SELECT line, position, text, attribute \
                   FROM user_errors \
                   WHERE name = :1 AND type = :2 \
                   ORDER BY sequence";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let rows = match stmt.query(&[&object_name.to_uppercase(), &object_type.to_uppercase()]) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };

        let mut errors = Vec::new();
        for row_result in rows {
            let row = match row_result {
                Ok(row) => row,
                Err(err) => {
                    eprintln!("Database operation failed: {err}");
                    return Err(err);
                }
            };
            let line: i32 = row.get(0).unwrap_or(0);
            let position: i32 = row.get(1).unwrap_or(0);
            let text: String = row
                .get::<_, Option<String>>(2)
                .unwrap_or(None)
                .unwrap_or_default();
            let attribute: String = row
                .get::<_, Option<String>>(3)
                .unwrap_or(None)
                .unwrap_or_default();

            errors.push(CompilationError {
                line,
                position,
                text: text.trim().to_string(),
                attribute,
            });
        }

        Ok(errors)
    }

    /// Get the compilation status of an object from user_objects
    pub fn get_object_status(
        conn: &Connection,
        object_name: &str,
        object_type: &str,
    ) -> Result<String, OracleError> {
        let sql = "SELECT status FROM user_objects WHERE object_name = :1 AND object_type = :2";
        let mut stmt = match conn.statement(sql).build() {
            Ok(stmt) => stmt,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let row = match stmt.query_row(&[&object_name.to_uppercase(), &object_type.to_uppercase()])
        {
            Ok(row) => row,
            Err(err) => {
                eprintln!("Database operation failed: {err}");
                return Err(err);
            }
        };
        let status: String = row
            .get::<_, Option<String>>(0)
            .unwrap_or(None)
            .unwrap_or_default();
        Ok(status)
    }
}

/// Compilation error information from USER_ERRORS
#[derive(Debug, Clone)]
pub struct CompilationError {
    pub line: i32,
    pub position: i32,
    pub text: String,
    pub attribute: String,
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
