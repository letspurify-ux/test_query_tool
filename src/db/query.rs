use oracle::sql_type::{OracleType, RefCursor};
use oracle::{Connection, Error as OracleError, Row, Statement};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::db::session::{BindDataType, BindValue, CompiledObject, SessionState};

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    #[allow(dead_code)]
    pub data_type: String,
}

#[derive(Debug, Clone)]
pub struct ProcedureArgument {
    pub name: Option<String>,
    pub position: i32,
    #[allow(dead_code)]
    pub sequence: i32,
    pub data_type: Option<String>,
    pub in_out: Option<String>,
    pub data_length: Option<i32>,
    pub data_precision: Option<i32>,
    pub data_scale: Option<i32>,
    pub type_owner: Option<String>,
    pub type_name: Option<String>,
    pub pls_type: Option<String>,
    pub overload: Option<i32>,
    pub default_value: Option<String>,
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
    pub success: bool,
}

#[derive(Debug, Clone)]
pub enum ScriptItem {
    Statement(String),
    ToolCommand(ToolCommand),
}

#[derive(Debug, Clone)]
pub enum FormatItem {
    Statement(String),
    ToolCommand(ToolCommand),
    Slash,
}

#[derive(Debug, Clone)]
pub enum ToolCommand {
    Var {
        name: String,
        data_type: BindDataType,
    },
    Print {
        name: Option<String>,
    },
    SetServerOutput {
        enabled: bool,
        size: Option<u32>,
        unlimited: bool,
    },
    ShowErrors {
        object_type: Option<String>,
        object_name: Option<String>,
    },
    Prompt {
        text: String,
    },
    Pause {
        message: Option<String>,
    },
    Accept {
        name: String,
        prompt: Option<String>,
    },
    Define {
        name: String,
        value: String,
    },
    Undefine {
        name: String,
    },
    SetErrorContinue {
        enabled: bool,
    },
    SetDefine {
        enabled: bool,
    },
    SetFeedback {
        enabled: bool,
    },
    SetHeading {
        enabled: bool,
    },
    SetPageSize {
        size: u32,
    },
    SetLineSize {
        size: u32,
    },
    Spool {
        path: Option<String>,
    },
    WheneverSqlError {
        exit: bool,
    },
    Exit,
    Quit,
    RunScript {
        path: String,
        relative_to_caller: bool,
    },
    Unsupported {
        raw: String,
        message: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedBind {
    pub name: String,
    pub data_type: BindDataType,
    pub value: Option<String>,
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
            success: true,
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
            success: true,
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
            success: true,
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
            success: false,
        }
    }
}

#[derive(Default)]
struct SplitState {
    in_single_quote: bool,
    in_double_quote: bool,
    in_line_comment: bool,
    in_block_comment: bool,
    in_q_quote: bool,
    q_quote_end: Option<char>,
    block_depth: usize,
    pending_end: bool,
    token: String,
    in_create_plsql: bool,
    create_pending: bool,
    create_or_seen: bool,
    after_declare: bool, // Track if we're inside DECLARE block waiting for BEGIN
    after_as_is: bool,   // Track if we've seen AS/IS in CREATE PL/SQL (for BEGIN handling)
}

impl SplitState {
    fn is_idle(&self) -> bool {
        !self.in_single_quote
            && !self.in_double_quote
            && !self.in_block_comment
            && !self.in_q_quote
            && !self.in_line_comment
    }

    fn flush_token(&mut self) {
        if self.token.is_empty() {
            return;
        }
        let upper = self.token.to_uppercase();

        self.track_create_plsql(&upper);

        if self.pending_end {
            if !matches!(upper.as_str(), "IF" | "LOOP" | "CASE") {
                if self.block_depth > 0 {
                    self.block_depth -= 1;
                }
            }
            self.pending_end = false;
        }

        if self.after_as_is == true && matches!(upper.as_str(), "OBJECT" | "VARRAY" | "TABLE") {
            self.block_depth -= 1;
            self.after_as_is = false;
        }

        // For CREATE PL/SQL (PACKAGE, PROCEDURE, FUNCTION, TYPE, TRIGGER),
        // AS or IS starts the body/specification block
        // For nested procedures/functions inside packages, IS also increments block_depth
        if self.in_create_plsql && matches!(upper.as_str(), "AS" | "IS") {
            self.block_depth += 1;
            self.after_as_is = true;
        } else if upper == "DECLARE" {
            // Standalone DECLARE block
            self.block_depth += 1;
            self.after_declare = true;
        } else if upper == "BEGIN" {
            if self.after_declare {
                // DECLARE ... BEGIN - same block, don't increase depth
                self.after_declare = false;
            } else if self.after_as_is {
                // AS/IS ... BEGIN - same block for CREATE PL/SQL, don't increase depth
                self.after_as_is = false;
            } else {
                // Standalone BEGIN block
                self.block_depth += 1;
            }
        } else if upper == "END" {
            self.pending_end = true;
            self.reset_create_state();
        }

        self.token.clear();
    }

    fn resolve_pending_end_on_terminator(&mut self) {
        if self.pending_end {
            if self.block_depth > 0 {
                self.block_depth -= 1;
            }
            self.pending_end = false;
        }
    }

    fn resolve_pending_end_on_eof(&mut self) {
        if self.pending_end {
            if self.block_depth > 0 {
                self.block_depth -= 1;
            }
            self.pending_end = false;
        }
    }

    fn reset_create_state(&mut self) {
        self.in_create_plsql = false;
        self.create_pending = false;
        self.create_or_seen = false;
        self.after_as_is = false;
    }

    fn track_create_plsql(&mut self, upper: &str) {
        if self.in_create_plsql {
            return;
        }

        if self.create_pending {
            match upper {
                "OR" => {
                    self.create_or_seen = true;
                    return;
                }
                "REPLACE" => {
                    return;
                }
                "EDITIONABLE" | "NONEDITIONABLE" => {
                    return;
                }
                "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TYPE" | "TRIGGER" => {
                    self.in_create_plsql = true;
                    self.create_pending = false;
                    self.create_or_seen = false;
                    return;
                }
                _ => {
                    self.create_pending = false;
                    self.create_or_seen = false;
                }
            }
        }

        if upper == "CREATE" {
            self.create_pending = true;
            self.create_or_seen = false;
        }
    }

    fn start_q_quote(&mut self, delimiter: char) {
        self.in_q_quote = true;
        self.q_quote_end = Some(match delimiter {
            '[' => ']',
            '(' => ')',
            '{' => '}',
            '<' => '>',
            other => other,
        });
    }

    fn q_quote_end(&self) -> Option<char> {
        self.q_quote_end
    }
}

struct StatementBuilder {
    state: SplitState,
    current: String,
    statements: Vec<String>,
}

impl StatementBuilder {
    fn new() -> Self {
        Self {
            state: SplitState::default(),
            current: String::new(),
            statements: Vec::new(),
        }
    }

    fn is_idle(&self) -> bool {
        self.state.is_idle()
    }

    fn current_is_empty(&self) -> bool {
        self.current.trim().is_empty()
    }

    fn in_create_plsql(&self) -> bool {
        self.state.in_create_plsql
    }

    fn block_depth(&self) -> usize {
        self.state.block_depth
    }

    fn process_text(&mut self, text: &str) {
        let chars: Vec<char> = text.chars().collect();
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

            if self.state.in_line_comment {
                self.current.push(c);
                if c == '\n' {
                    self.state.in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if self.state.in_block_comment {
                self.current.push(c);
                if c == '*' && next == Some('/') {
                    self.current.push('/');
                    self.state.in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if self.state.in_q_quote {
                self.current.push(c);
                if Some(c) == self.state.q_quote_end() && next == Some('\'') {
                    self.current.push('\'');
                    self.state.in_q_quote = false;
                    self.state.q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if self.state.in_single_quote {
                self.current.push(c);
                if c == '\'' {
                    if next == Some('\'') {
                        self.current.push('\'');
                        i += 2;
                        continue;
                    }
                    self.state.in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if self.state.in_double_quote {
                self.current.push(c);
                if c == '"' {
                    if next == Some('"') {
                        self.current.push('"');
                        i += 2;
                        continue;
                    }
                    self.state.in_double_quote = false;
                }
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                self.state.flush_token();
                self.state.in_line_comment = true;
                self.current.push('-');
                self.current.push('-');
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                self.state.flush_token();
                self.state.in_block_comment = true;
                self.current.push('/');
                self.current.push('*');
                i += 2;
                continue;
            }

            if (c == 'q' || c == 'Q') && next == Some('\'') {
                if let Some(delimiter) = next2 {
                    self.state.flush_token();
                    self.state.start_q_quote(delimiter);
                    self.current.push(c);
                    self.current.push('\'');
                    self.current.push(delimiter);
                    i += 3;
                    continue;
                }
            }

            if c == '\'' {
                self.state.flush_token();
                self.state.in_single_quote = true;
                self.current.push(c);
                i += 1;
                continue;
            }

            if c == '"' {
                self.state.flush_token();
                self.state.in_double_quote = true;
                self.current.push(c);
                i += 1;
                continue;
            }

            if c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '#' {
                self.state.token.push(c);
                self.current.push(c);
                i += 1;
                continue;
            }

            self.state.flush_token();

            if c == ';' {
                self.state.resolve_pending_end_on_terminator();
                if self.state.block_depth == 0 {
                    let trimmed = self.current.trim();
                    if !trimmed.is_empty() {
                        self.statements.push(trimmed.to_string());
                    }
                    self.current.clear();
                } else {
                    self.current.push(c);
                }
                i += 1;
                continue;
            }

            self.current.push(c);
            i += 1;
        }
    }

    fn force_terminate(&mut self) {
        self.state.flush_token();
        self.state.resolve_pending_end_on_eof();
        self.state.reset_create_state();
        let trimmed = self.current.trim();
        if !trimmed.is_empty() {
            self.statements.push(trimmed.to_string());
        }
        self.current.clear();
    }

    fn finalize(&mut self) {
        self.state.flush_token();
        self.state.resolve_pending_end_on_eof();
        self.state.reset_create_state();
        let trimmed = self.current.trim();
        if !trimmed.is_empty() {
            self.statements.push(trimmed.to_string());
        }
        self.current.clear();
    }

    fn take_statements(&mut self) -> Vec<String> {
        std::mem::take(&mut self.statements)
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

    fn strip_trailing_comments(sql: &str) -> String {
        let mut result = sql.to_string();

        loop {
            let trimmed = result.trim_end();
            if trimmed.is_empty() {
                return String::new();
            }

            // Check for trailing line comment (-- ... at end of line)
            // Find the last line and check if it's only a comment
            if let Some(last_newline) = trimmed.rfind('\n') {
                let last_line = trimmed[last_newline + 1..].trim();
                if last_line.starts_with("--") {
                    result = trimmed[..last_newline].to_string();
                    continue;
                }
            } else {
                // Single line - check if entire thing is a line comment
                let trimmed_start = trimmed.trim_start();
                if trimmed_start.starts_with("--") {
                    return String::new();
                }
            }

            // Check for trailing block comment
            if trimmed.ends_with("*/") {
                // Find matching /*
                // Need to scan backwards to find the opening /*
                let bytes = trimmed.as_bytes();
                let mut depth = 0;
                let mut i = bytes.len();
                let mut found_start = None;

                while i > 0 {
                    i -= 1;
                    if i > 0 && bytes[i - 1] == b'/' && bytes[i] == b'*' {
                        depth -= 1;
                        if depth < 0 {
                            found_start = Some(i - 1);
                            break;
                        }
                        i -= 1;
                    } else if i > 0 && bytes[i - 1] == b'*' && bytes[i] == b'/' {
                        depth += 1;
                        i -= 1;
                    }
                }

                if let Some(start) = found_start {
                    // Check if this block comment is at the end (only whitespace before it)
                    let before = trimmed[..start].trim_end();
                    if before.is_empty() {
                        return String::new();
                    }
                    result = before.to_string();
                    continue;
                }
            }

            return trimmed.to_string();
        }
    }

    fn strip_comments(sql: &str) -> String {
        let without_leading = Self::strip_leading_comments(sql);
        Self::strip_trailing_comments(&without_leading)
    }

    /// Strip extra trailing semicolons from a statement.
    /// "END;;" -> "END;", "SELECT 1;;" -> "SELECT 1"
    /// Preserves single trailing semicolon for PL/SQL statements.
    fn strip_extra_trailing_semicolons(sql: &str) -> String {
        let trimmed = sql.trim_end();
        if trimmed.is_empty() {
            return String::new();
        }

        // Count trailing semicolons
        let mut semicolon_count = 0;
        for c in trimmed.chars().rev() {
            if c == ';' {
                semicolon_count += 1;
            } else if c.is_whitespace() {
                continue;
            } else {
                break;
            }
        }

        if semicolon_count <= 1 {
            return trimmed.to_string();
        }

        // Remove all trailing semicolons and whitespace, then check if we need to add one back
        let without_semis = trimmed.trim_end_matches(|c: char| c == ';' || c.is_whitespace());
        if without_semis.is_empty() {
            return String::new();
        }

        // Check if this is a PL/SQL statement that needs trailing semicolon
        let upper = without_semis.to_uppercase();
        if upper.ends_with("END") || upper.contains("END ") {
            format!("{};", without_semis)
        } else {
            without_semis.to_string()
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
        matches!(
            Self::leading_keyword(sql).as_deref(),
            Some("SELECT") | Some("WITH")
        )
    }

    pub fn split_script_items(sql: &str) -> Vec<ScriptItem> {
        let mut items: Vec<ScriptItem> = Vec::new();
        let mut builder = StatementBuilder::new();

        // Helper to add statement with comment stripping and extra semicolon removal
        let add_statement = |stmt: String, items: &mut Vec<ScriptItem>| {
            let stripped = Self::strip_comments(&stmt);
            let cleaned = Self::strip_extra_trailing_semicolons(&stripped);
            if !cleaned.is_empty() {
                items.push(ScriptItem::Statement(cleaned));
            }
        };

        for line in sql.lines() {
            let trimmed = line.trim();
            let trimmed_upper = trimmed.to_uppercase();

            if builder.is_idle()
                && builder.in_create_plsql()
                && builder.block_depth() == 0
                && !builder.current_is_empty()
                && (trimmed_upper.starts_with("CREATE")
                    || trimmed_upper.starts_with("ALTER")
                    || trimmed_upper.starts_with("DROP")
                    || trimmed_upper.starts_with("TRUNCATE")
                    || trimmed_upper.starts_with("GRANT")
                    || trimmed_upper.starts_with("REVOKE")
                    || trimmed_upper.starts_with("COMMIT")
                    || trimmed_upper.starts_with("ROLLBACK")
                    || trimmed_upper.starts_with("SAVEPOINT")
                    || trimmed_upper.starts_with("SELECT")
                    || trimmed_upper.starts_with("INSERT")
                    || trimmed_upper.starts_with("UPDATE")
                    || trimmed_upper.starts_with("DELETE")
                    || trimmed_upper.starts_with("MERGE")
                    || trimmed_upper.starts_with("WITH"))
            {
                builder.force_terminate();
                for stmt in builder.take_statements() {
                    add_statement(stmt, &mut items);
                }
            }

            if builder.is_idle() && trimmed == "/" {
                if !builder.current_is_empty() {
                    builder.force_terminate();
                    for stmt in builder.take_statements() {
                        add_statement(stmt, &mut items);
                    }
                }
                continue;
            }

            // Handle lone semicolon line after CREATE PL/SQL statement
            // This prevents ";;" issue when extra ";" is on its own line
            if builder.is_idle()
                && trimmed == ";"
                && builder.in_create_plsql()
                && builder.block_depth() == 0
                && !builder.current_is_empty()
            {
                builder.force_terminate();
                for stmt in builder.take_statements() {
                    add_statement(stmt, &mut items);
                }
                continue;
            }

            if builder.is_idle() && !builder.current_is_empty() && builder.block_depth() == 0 {
                if let Some(command) = Self::parse_tool_command(trimmed) {
                    builder.force_terminate();
                    for stmt in builder.take_statements() {
                        add_statement(stmt, &mut items);
                    }
                    items.push(ScriptItem::ToolCommand(command));
                    continue;
                }
            }

            if builder.is_idle() && builder.current_is_empty() && builder.block_depth() == 0 {
                if let Some(command) = Self::parse_tool_command(trimmed) {
                    items.push(ScriptItem::ToolCommand(command));
                    continue;
                }
            }

            let mut line_with_newline = String::from(line);
            line_with_newline.push('\n');
            builder.process_text(&line_with_newline);
            for stmt in builder.take_statements() {
                add_statement(stmt, &mut items);
            }
        }

        builder.finalize();
        for stmt in builder.take_statements() {
            add_statement(stmt, &mut items);
        }

        items
    }

    pub fn split_format_items(sql: &str) -> Vec<FormatItem> {
        let mut items: Vec<FormatItem> = Vec::new();
        let mut builder = StatementBuilder::new();

        let add_statement = |stmt: String, items: &mut Vec<FormatItem>| {
            let cleaned = stmt.trim();
            if !cleaned.is_empty() {
                items.push(FormatItem::Statement(cleaned.to_string()));
            }
        };

        let mut lines = sql.lines().peekable();
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            let trimmed_upper = trimmed.to_uppercase();

            if builder.is_idle() && builder.current_is_empty() {
                if trimmed.starts_with("--") {
                    items.push(FormatItem::Statement(line.to_string()));
                    continue;
                }
                if trimmed.starts_with("/*") {
                    let mut comment = String::new();
                    comment.push_str(line);
                    if !trimmed.contains("*/") {
                        while let Some(next_line) = lines.next() {
                            comment.push('\n');
                            comment.push_str(next_line);
                            if next_line.contains("*/") {
                                break;
                            }
                        }
                    }
                    items.push(FormatItem::Statement(comment));
                    continue;
                }
            }

            if builder.is_idle()
                && builder.in_create_plsql()
                && builder.block_depth() == 0
                && !builder.current_is_empty()
                && (trimmed_upper.starts_with("CREATE")
                    || trimmed_upper.starts_with("ALTER")
                    || trimmed_upper.starts_with("DROP")
                    || trimmed_upper.starts_with("TRUNCATE")
                    || trimmed_upper.starts_with("GRANT")
                    || trimmed_upper.starts_with("REVOKE")
                    || trimmed_upper.starts_with("COMMIT")
                    || trimmed_upper.starts_with("ROLLBACK")
                    || trimmed_upper.starts_with("SAVEPOINT")
                    || trimmed_upper.starts_with("SELECT")
                    || trimmed_upper.starts_with("INSERT")
                    || trimmed_upper.starts_with("UPDATE")
                    || trimmed_upper.starts_with("DELETE")
                    || trimmed_upper.starts_with("MERGE")
                    || trimmed_upper.starts_with("WITH"))
            {
                builder.force_terminate();
                for stmt in builder.take_statements() {
                    add_statement(stmt, &mut items);
                }
            }

            if builder.is_idle() && trimmed == "/" {
                if !builder.current_is_empty() {
                    builder.force_terminate();
                    for stmt in builder.take_statements() {
                        add_statement(stmt, &mut items);
                    }
                }
                items.push(FormatItem::Slash);
                continue;
            }

            if builder.is_idle()
                && trimmed == ";"
                && builder.in_create_plsql()
                && builder.block_depth() == 0
                && !builder.current_is_empty()
            {
                builder.force_terminate();
                for stmt in builder.take_statements() {
                    add_statement(stmt, &mut items);
                }
                continue;
            }

            if builder.is_idle() && !builder.current_is_empty() && builder.block_depth() == 0 {
                if let Some(command) = Self::parse_tool_command(trimmed) {
                    builder.force_terminate();
                    for stmt in builder.take_statements() {
                        add_statement(stmt, &mut items);
                    }
                    items.push(FormatItem::ToolCommand(command));
                    continue;
                }
            }

            if builder.is_idle() && builder.current_is_empty() && builder.block_depth() == 0 {
                if let Some(command) = Self::parse_tool_command(trimmed) {
                    items.push(FormatItem::ToolCommand(command));
                    continue;
                }
            }

            let mut line_with_newline = String::from(line);
            line_with_newline.push('\n');
            builder.process_text(&line_with_newline);
            for stmt in builder.take_statements() {
                add_statement(stmt, &mut items);
            }
        }

        builder.finalize();
        for stmt in builder.take_statements() {
            add_statement(stmt, &mut items);
        }

        items
    }

    fn parse_tool_command(line: &str) -> Option<ToolCommand> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let trimmed = trimmed.trim_end_matches(';').trim();
        if trimmed.is_empty() {
            return None;
        }

        let upper = trimmed.to_uppercase();

        if upper == "VAR" || upper.starts_with("VAR ") || upper.starts_with("VARIABLE ") {
            return Some(Self::parse_var_command(trimmed));
        }

        if upper.starts_with("PRINT") {
            let rest = trimmed[5..].trim();
            let name = if rest.is_empty() {
                None
            } else {
                Some(rest.trim_start_matches(':').to_string())
            };
            return Some(ToolCommand::Print { name });
        }

        if upper.starts_with("SET SERVEROUTPUT") {
            return Some(Self::parse_serveroutput_command(trimmed));
        }

        if upper.starts_with("SHOW ERRORS") {
            return Some(Self::parse_show_errors_command(trimmed));
        }

        if upper.starts_with("PROMPT") {
            let text = trimmed[6..].trim().to_string();
            return Some(ToolCommand::Prompt { text });
        }

        if upper.starts_with("PAUSE") {
            return Some(Self::parse_pause_command(trimmed));
        }

        if upper.starts_with("ACCEPT") {
            return Some(Self::parse_accept_command(trimmed));
        }

        if upper.starts_with("DEFINE") {
            return Some(Self::parse_define_assign_command(trimmed));
        }

        if upper.starts_with("UNDEFINE") {
            return Some(Self::parse_undefine_command(trimmed));
        }

        if upper.starts_with("SPOOL") {
            return Some(Self::parse_spool_command(trimmed));
        }

        if upper.starts_with("SET ERRORCONTINUE") {
            return Some(Self::parse_errorcontinue_command(trimmed));
        }

        if upper.starts_with("SET DEFINE") {
            return Some(Self::parse_define_command(trimmed));
        }

        if upper.starts_with("SET FEEDBACK") {
            return Some(Self::parse_feedback_command(trimmed));
        }

        if upper.starts_with("SET HEADING") {
            return Some(Self::parse_heading_command(trimmed));
        }

        if upper.starts_with("SET PAGESIZE") {
            return Some(Self::parse_pagesize_command(trimmed));
        }

        if upper.starts_with("SET LINESIZE") {
            return Some(Self::parse_linesize_command(trimmed));
        }

        if trimmed.starts_with("@@")
            || trimmed.starts_with('@')
            || Self::is_start_script_command(trimmed)
        {
            return Some(Self::parse_script_command(trimmed));
        }

        if upper.starts_with("WHENEVER SQLERROR") {
            return Some(Self::parse_whenever_sqlerror_command(trimmed));
        }

        if upper == "EXIT" || upper.starts_with("EXIT ") {
            return Some(ToolCommand::Exit);
        }

        if upper == "QUIT" || upper.starts_with("QUIT ") {
            return Some(ToolCommand::Quit);
        }

        None
    }

    fn parse_var_command(raw: &str) -> ToolCommand {
        let mut parts = raw.split_whitespace();
        let _ = parts.next(); // VAR or VARIABLE
        let name = parts.next().unwrap_or_default();
        let type_str = parts.collect::<Vec<&str>>().join(" ");

        if name.is_empty() || type_str.trim().is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "VAR requires a variable name and type.".to_string(),
                is_error: true,
            };
        }

        match Self::parse_bind_type(&type_str) {
            Ok(data_type) => ToolCommand::Var {
                name: name.trim_start_matches(':').to_string(),
                data_type,
            },
            Err(message) => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message,
                is_error: true,
            },
        }
    }

    fn parse_serveroutput_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET SERVEROUTPUT requires ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mode = tokens[2].to_uppercase();
        if mode == "OFF" {
            return ToolCommand::SetServerOutput {
                enabled: false,
                size: None,
                unlimited: false,
            };
        }

        if mode != "ON" {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET SERVEROUTPUT supports only ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mut size: Option<u32> = None;
        let mut unlimited = false;
        let mut idx = 3usize;
        while idx + 1 < tokens.len() {
            if tokens[idx].eq_ignore_ascii_case("SIZE") {
                let size_val = tokens[idx + 1];
                if size_val.eq_ignore_ascii_case("UNLIMITED") {
                    unlimited = true;
                } else {
                    match size_val.parse::<u32>() {
                        Ok(val) => size = Some(val),
                        Err(_) => {
                            return ToolCommand::Unsupported {
                                raw: raw.to_string(),
                                message: "SET SERVEROUTPUT SIZE must be a number or UNLIMITED."
                                    .to_string(),
                                is_error: true,
                            };
                        }
                    }
                }
                break;
            }
            idx += 1;
        }

        ToolCommand::SetServerOutput {
            enabled: true,
            size,
            unlimited,
        }
    }

    fn parse_show_errors_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() <= 2 {
            return ToolCommand::ShowErrors {
                object_type: None,
                object_name: None,
            };
        }

        let mut idx = 2usize;
        let mut object_type = tokens[idx].to_uppercase();
        if object_type == "PACKAGE"
            && tokens
                .get(idx + 1)
                .map(|t| t.eq_ignore_ascii_case("BODY"))
                .unwrap_or(false)
        {
            object_type = "PACKAGE BODY".to_string();
            idx += 2;
        } else if object_type == "TYPE"
            && tokens
                .get(idx + 1)
                .map(|t| t.eq_ignore_ascii_case("BODY"))
                .unwrap_or(false)
        {
            object_type = "TYPE BODY".to_string();
            idx += 2;
        } else {
            idx += 1;
        }

        let name = tokens
            .get(idx)
            .map(|v| v.trim_start_matches(':').to_string());
        if name.is_none() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SHOW ERRORS requires an object name when a type is specified."
                    .to_string(),
                is_error: true,
            };
        }

        ToolCommand::ShowErrors {
            object_type: Some(object_type),
            object_name: name,
        }
    }

    fn parse_accept_command(raw: &str) -> ToolCommand {
        let rest = raw[6..].trim();
        if rest.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "ACCEPT requires a variable name.".to_string(),
                is_error: true,
            };
        }

        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        if name.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "ACCEPT requires a variable name.".to_string(),
                is_error: true,
            };
        }
        let remainder = parts.next().unwrap_or("").trim();
        let prompt = if remainder.is_empty() {
            None
        } else {
            let upper = remainder.to_uppercase();
            if let Some(idx) = upper.find("PROMPT") {
                let prompt_raw = remainder[idx + 6..].trim();
                let cleaned = prompt_raw.trim_matches('"').trim_matches('\'').to_string();
                if cleaned.is_empty() {
                    None
                } else {
                    Some(cleaned)
                }
            } else {
                None
            }
        };

        ToolCommand::Accept {
            name: name.trim_start_matches(':').to_string(),
            prompt,
        }
    }

    fn parse_pause_command(raw: &str) -> ToolCommand {
        let rest = raw[5..].trim();
        let message = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };

        ToolCommand::Pause { message }
    }

    fn parse_define_assign_command(raw: &str) -> ToolCommand {
        let rest = raw[6..].trim();
        if rest.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "DEFINE requires a variable name and value.".to_string(),
                is_error: true,
            };
        }

        let (name, value) = if let Some(eq_idx) = rest.find('=') {
            let (left, right) = rest.split_at(eq_idx);
            (left.trim(), right.trim_start_matches('=').trim())
        } else {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or("").trim();
            (name, value)
        };

        if name.is_empty() || value.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "DEFINE requires a variable name and value.".to_string(),
                is_error: true,
            };
        }

        ToolCommand::Define {
            name: name.trim_start_matches(':').to_string(),
            value: value.to_string(),
        }
    }

    fn parse_undefine_command(raw: &str) -> ToolCommand {
        let rest = raw[8..].trim();
        if rest.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "UNDEFINE requires a variable name.".to_string(),
                is_error: true,
            };
        }

        ToolCommand::Undefine {
            name: rest.trim_start_matches(':').to_string(),
        }
    }

    fn parse_spool_command(raw: &str) -> ToolCommand {
        let rest = raw[5..].trim();
        if rest.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SPOOL requires a file path or OFF.".to_string(),
                is_error: true,
            };
        }

        if rest.eq_ignore_ascii_case("OFF") {
            return ToolCommand::Spool { path: None };
        }

        let cleaned = rest.trim_matches('"').trim_matches('\'').to_string();
        if cleaned.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SPOOL requires a file path.".to_string(),
                is_error: true,
            };
        }

        ToolCommand::Spool {
            path: Some(cleaned),
        }
    }

    fn parse_whenever_sqlerror_command(raw: &str) -> ToolCommand {
        let rest = raw[17..].trim();
        if rest.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "WHENEVER SQLERROR requires EXIT or CONTINUE.".to_string(),
                is_error: true,
            };
        }
        let token = rest.split_whitespace().next().unwrap_or("").to_uppercase();
        match token.as_str() {
            "EXIT" => ToolCommand::WheneverSqlError { exit: true },
            "CONTINUE" => ToolCommand::WheneverSqlError { exit: false },
            _ => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "WHENEVER SQLERROR supports EXIT or CONTINUE.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_errorcontinue_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET ERRORCONTINUE requires ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mode = tokens[2].to_uppercase();
        match mode.as_str() {
            "ON" => ToolCommand::SetErrorContinue { enabled: true },
            "OFF" => ToolCommand::SetErrorContinue { enabled: false },
            _ => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET ERRORCONTINUE supports only ON or OFF.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_define_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET DEFINE requires ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mode = tokens[2].to_uppercase();
        match mode.as_str() {
            "ON" => ToolCommand::SetDefine { enabled: true },
            "OFF" => ToolCommand::SetDefine { enabled: false },
            _ => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET DEFINE supports only ON or OFF.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_feedback_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET FEEDBACK requires ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mode = tokens[2].to_uppercase();
        match mode.as_str() {
            "ON" => ToolCommand::SetFeedback { enabled: true },
            "OFF" => ToolCommand::SetFeedback { enabled: false },
            _ => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET FEEDBACK supports only ON or OFF.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_heading_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET HEADING requires ON or OFF.".to_string(),
                is_error: true,
            };
        }

        let mode = tokens[2].to_uppercase();
        match mode.as_str() {
            "ON" => ToolCommand::SetHeading { enabled: true },
            "OFF" => ToolCommand::SetHeading { enabled: false },
            _ => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET HEADING supports only ON or OFF.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_pagesize_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET PAGESIZE requires a number.".to_string(),
                is_error: true,
            };
        }

        match tokens[2].parse::<u32>() {
            Ok(size) => ToolCommand::SetPageSize { size },
            Err(_) => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET PAGESIZE requires a number.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_linesize_command(raw: &str) -> ToolCommand {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 3 {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET LINESIZE requires a number.".to_string(),
                is_error: true,
            };
        }

        match tokens[2].parse::<u32>() {
            Ok(size) => ToolCommand::SetLineSize { size },
            Err(_) => ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: "SET LINESIZE requires a number.".to_string(),
                is_error: true,
            },
        }
    }

    fn parse_script_command(raw: &str) -> ToolCommand {
        let trimmed = raw.trim();
        let (relative_to_caller, command_label, path) = if trimmed.starts_with("@@") {
            (
                true,
                "@@",
                trimmed.trim_start_matches("@@").trim(),
            )
        } else if trimmed.starts_with('@') {
            (false, "@", trimmed.trim_start_matches('@').trim())
        } else if Self::is_start_script_command(trimmed) {
            (
                false,
                "START",
                trimmed.get(5..).unwrap_or_default().trim(),
            )
        } else {
            (false, "@", "")
        };

        if path.is_empty() {
            return ToolCommand::Unsupported {
                raw: raw.to_string(),
                message: if command_label == "START" {
                    "START requires a path.".to_string()
                } else {
                    "@file.sql requires a path.".to_string()
                },
                is_error: true,
            };
        }

        let cleaned = path.trim_matches('"').trim_matches('\'').to_string();

        ToolCommand::RunScript {
            path: cleaned,
            relative_to_caller,
        }
    }

    fn is_start_script_command(trimmed: &str) -> bool {
        if trimmed.len() < 5 {
            return false;
        }
        let head = match trimmed.get(0..5) {
            Some(head) => head,
            None => return false,
        };
        if !head.eq_ignore_ascii_case("START") {
            return false;
        }
        let tail = match trimmed.get(5..) {
            Some(tail) => tail,
            None => return false,
        };
        tail.is_empty()
            || tail
                .chars()
                .next()
                .map(|ch| ch.is_whitespace())
                .unwrap_or(false)
    }

    fn parse_bind_type(type_str: &str) -> Result<BindDataType, String> {
        let trimmed = type_str.trim();
        if trimmed.is_empty() {
            return Err("VAR requires a data type.".to_string());
        }

        let upper = trimmed.to_uppercase();
        let compact = upper.replace(' ', "");

        if compact == "REFCURSOR" || compact == "SYS_REFCURSOR" {
            return Ok(BindDataType::RefCursor);
        }

        if upper.starts_with("NUMBER") || upper.starts_with("NUMERIC") {
            return Ok(BindDataType::Number);
        }

        if upper.starts_with("DATE") {
            return Ok(BindDataType::Date);
        }

        if upper.starts_with("TIMESTAMP") {
            let precision = Self::parse_parenthesized_u8(&upper).unwrap_or(6);
            return Ok(BindDataType::Timestamp(precision));
        }

        if upper.starts_with("CLOB") {
            return Ok(BindDataType::Clob);
        }

        if upper.starts_with("VARCHAR2")
            || upper.starts_with("VARCHAR")
            || upper.starts_with("NVARCHAR2")
        {
            let size = Self::parse_parenthesized_u32(&upper).unwrap_or(4000);
            return Ok(BindDataType::Varchar2(size));
        }

        if upper.starts_with("CHAR") || upper.starts_with("NCHAR") {
            let size = Self::parse_parenthesized_u32(&upper).unwrap_or(2000);
            return Ok(BindDataType::Varchar2(size));
        }

        Err(format!("Unsupported VAR type: {}", trimmed))
    }

    fn parse_parenthesized_u32(value: &str) -> Option<u32> {
        let start = value.find('(')?;
        let end = value[start + 1..].find(')')? + start + 1;
        value[start + 1..end].trim().parse::<u32>().ok()
    }

    fn parse_parenthesized_u8(value: &str) -> Option<u8> {
        let start = value.find('(')?;
        let end = value[start + 1..].find(')')? + start + 1;
        value[start + 1..end].trim().parse::<u8>().ok()
    }

    fn extract_bind_names(sql: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

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
        let mut has_positional = false;

        for arg in args_list {
            if arg.trim().is_empty() {
                continue;
            }
            if Self::arg_has_named_arrow(&arg) {
                has_named = true;
            } else {
                has_positional = true;
            }
        }

        if has_named && has_positional {
            return Err("Named and positional parameters cannot be mixed.".to_string());
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

        let cursor_pos = cursor_pos.min(sql.len());
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
            success: true,
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
            message: "PL/SQL procedure successfully completed".to_string(),
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
            message: "PL/SQL procedure successfully completed".to_string(),
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
    pub min_value: i64,
    pub max_value: i64,
    pub increment_by: i64,
    pub cycle_flag: String,
    pub order_flag: String,
    pub cache_size: i64,
    pub last_number: i64,
}

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

    pub fn get_sequence_info(conn: &Connection, seq_name: &str) -> Result<SequenceInfo, OracleError> {
        let sql = r#"
            SELECT
                sequence_name,
                min_value,
                max_value,
                increment_by,
                cycle_flag,
                order_flag,
                cache_size,
                last_number
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
        let min_value: i64 = row.get(1)?;
        let max_value: i64 = row.get(2)?;
        let increment_by: i64 = row.get(3)?;
        let cycle_flag: String = row.get(4)?;
        let order_flag: String = row.get(5)?;
        let cache_size: i64 = row.get(6)?;
        let last_number: i64 = row.get(7)?;

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

        let mut procedures: Vec<String> = Vec::new();
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
            procedures.push(name);
        }

        Ok(procedures)
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
        Ok(ddl)
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
        Ok(ddl)
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
        Ok(ddl)
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
        Ok(ddl)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to extract statements from ScriptItems
    fn get_statements(items: &[ScriptItem]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|item| match item {
                ScriptItem::Statement(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_simple_select() {
        let sql = "SELECT 1 FROM DUAL;";
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("SELECT 1 FROM DUAL"));
    }

    #[test]
    fn test_multiple_selects() {
        let sql = "SELECT 1 FROM DUAL;\nSELECT 2 FROM DUAL;";
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_double_semicolon() {
        let sql = "SELECT 1 FROM DUAL;;";
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
        assert!(
            !stmts[0].ends_with(";;"),
            "Should not end with ;;: {}",
            stmts[0]
        );
    }

    #[test]
    fn test_anonymous_block() {
        let sql = "DECLARE x NUMBER; BEGIN x := 1; END;";
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_procedure_simple() {
        let sql = "CREATE PROCEDURE test_proc AS BEGIN NULL; END;";
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
        assert!(stmts[0].contains("CREATE PROCEDURE"));
        assert!(stmts[0].contains("END"));
    }

    #[test]
    fn test_create_procedure_with_declare() {
        let sql = r#"CREATE PROCEDURE test_proc AS
DECLARE
  v_num NUMBER;
BEGIN
  v_num := 1;
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_or_replace_procedure() {
        let sql = r#"CREATE OR REPLACE PROCEDURE test_proc IS
BEGIN
  DBMS_OUTPUT.PUT_LINE('Hello');
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_function() {
        let sql = r#"CREATE FUNCTION add_nums(a NUMBER, b NUMBER) RETURN NUMBER IS
BEGIN
  RETURN a + b;
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_package_spec() {
        let sql = r#"CREATE PACKAGE test_pkg AS
  PROCEDURE proc1;
  FUNCTION func1 RETURN NUMBER;
END test_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
        assert!(stmts[0].contains("CREATE PACKAGE"));
        assert!(stmts[0].contains("END test_pkg"));
    }

    #[test]
    fn test_create_package_body_simple() {
        let sql = r#"CREATE PACKAGE BODY test_pkg AS
  PROCEDURE proc1 IS
  BEGIN
    NULL;
  END;
END test_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_package_body_complex() {
        let sql = r#"CREATE OR REPLACE PACKAGE BODY oqt_pkg AS
  PROCEDURE log_msg(p_tag IN VARCHAR2, p_msg IN VARCHAR2, p_n1 IN NUMBER DEFAULT NULL) IS
  BEGIN
    INSERT INTO oqt_call_log(id, tag, msg, n1, created_at)
    VALUES (oqt_call_log_seq.NEXTVAL, p_tag, p_msg, p_n1, SYSDATE);
  END;

  PROCEDURE p_basic(
    p_in_num   IN  NUMBER,
    p_in_txt   IN  VARCHAR2 DEFAULT 'DEF',
    p_out_txt  OUT VARCHAR2,
    p_inout_n  IN OUT NUMBER
  ) IS
  BEGIN
    p_out_txt := 'IN_NUM='||p_in_num||', IN_TXT='||p_in_txt||', INOUT='||p_inout_n;
    p_inout_n := NVL(p_inout_n,0) + p_in_num;

    log_msg('P_BASIC', p_out_txt, p_in_num);
    DBMS_OUTPUT.PUT_LINE('[p_basic] out='||p_out_txt||' / inout='||p_inout_n);
  END;

  PROCEDURE p_over(p_txt IN VARCHAR2) IS
  BEGIN
    log_msg('P_OVER1', p_txt);
    DBMS_OUTPUT.PUT_LINE('[p_over(txt)] '||NVL(p_txt,'<NULL>'));
  END;

  PROCEDURE p_over(p_num IN NUMBER, p_txt IN VARCHAR2) IS
  BEGIN
    log_msg('P_OVER2', p_txt, p_num);
    DBMS_OUTPUT.PUT_LINE('[p_over(num,txt)] '||p_num||' / '||NVL(p_txt,'<NULL>'));
  END;

  PROCEDURE p_refcur(p_tag IN VARCHAR2, p_rc OUT SYS_REFCURSOR) IS
  BEGIN
    OPEN p_rc FOR
      SELECT id, tag, msg, n1, created_at
      FROM oqt_call_log
      WHERE tag LIKE p_tag||'%'
      ORDER BY id DESC;
  END;

  PROCEDURE p_raise(p_mode IN VARCHAR2) IS
  BEGIN
    IF p_mode = 'NO_DATA_FOUND' THEN
      DECLARE v NUMBER;
      BEGIN
        SELECT n1 INTO v FROM oqt_call_log WHERE id = -9999;
      END;
    ELSIF p_mode = 'APP' THEN
      RAISE_APPLICATION_ERROR(-20001, 'oqt_pkg.p_raise app error');
    ELSE
      DBMS_OUTPUT.PUT_LINE('[p_raise] ok');
    END IF;
  END;

  FUNCTION f_sum(p_a IN NUMBER, p_b IN NUMBER) RETURN NUMBER IS
  BEGIN
    RETURN NVL(p_a,0) + NVL(p_b,0);
  END;

  FUNCTION f_echo(p_txt IN VARCHAR2) RETURN VARCHAR2 IS
  BEGIN
    RETURN 'ECHO:'||p_txt;
  END;

  FUNCTION f_dateadd(p_d IN DATE, p_days IN NUMBER DEFAULT 1) RETURN DATE IS
  BEGIN
    RETURN p_d + p_days;
  END;
END oqt_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(
            stmts.len(),
            1,
            "Should have 1 statement, got {} statements",
            stmts.len()
        );
        if stmts.len() > 1 {
            for (i, s) in stmts.iter().enumerate() {
                println!("Statement {}: {}", i, &s[..s.len().min(100)]);
            }
        }
        assert!(stmts[0].contains("CREATE OR REPLACE PACKAGE BODY"));
        assert!(stmts[0].contains("END oqt_pkg"));
    }

    #[test]
    fn test_nested_begin_end_in_package() {
        let sql = r#"CREATE PACKAGE BODY test_pkg AS
  PROCEDURE proc1 IS
  BEGIN
    IF TRUE THEN
      BEGIN
        NULL;
      END;
    END IF;
  END;
END test_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_package_with_nested_declare() {
        let sql = r#"CREATE PACKAGE BODY test_pkg AS
  PROCEDURE proc1 IS
  BEGIN
    DECLARE
      v_temp NUMBER;
    BEGIN
      v_temp := 1;
    END;
  END;
END test_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_package_followed_by_select() {
        let sql = r#"CREATE PACKAGE test_pkg AS
  PROCEDURE proc1;
END test_pkg;

SELECT 1 FROM DUAL;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 2, "Should have 2 statements, got: {:?}", stmts);
        assert!(stmts[0].contains("CREATE PACKAGE"));
        assert!(stmts[1].contains("SELECT"));
    }

    #[test]
    fn test_multiple_packages() {
        let sql = r#"CREATE PACKAGE pkg1 AS
  PROCEDURE proc1;
END pkg1;

CREATE PACKAGE pkg2 AS
  PROCEDURE proc2;
END pkg2;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 2, "Should have 2 statements, got: {:?}", stmts);
    }

    #[test]
    fn test_create_trigger() {
        let sql = r#"CREATE TRIGGER test_trg
BEFORE INSERT ON test_table
FOR EACH ROW
BEGIN
  :NEW.created_at := SYSDATE;
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_create_type() {
        let sql = r#"CREATE TYPE test_type AS OBJECT (
  id NUMBER,
  name VARCHAR2(100)
);"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_comments_stripped() {
        let sql = r#"-- This is a comment
SELECT 1 FROM DUAL;
-- Another comment"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
        assert!(
            !stmts[0].starts_with("--"),
            "Leading comment should be stripped"
        );
    }

    #[test]
    fn test_block_comment_stripped() {
        let sql = r#"/* Block comment */
SELECT 1 FROM DUAL;
/* Trailing comment */"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_procedure_with_loop() {
        let sql = r#"CREATE PROCEDURE test_proc AS
BEGIN
  FOR i IN 1..10 LOOP
    DBMS_OUTPUT.PUT_LINE(i);
  END LOOP;
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_procedure_with_case() {
        let sql = r#"CREATE PROCEDURE test_proc(p_val NUMBER) AS
BEGIN
  CASE p_val
    WHEN 1 THEN DBMS_OUTPUT.PUT_LINE('one');
    WHEN 2 THEN DBMS_OUTPUT.PUT_LINE('two');
    ELSE DBMS_OUTPUT.PUT_LINE('other');
  END CASE;
END;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 1, "Should have 1 statement, got: {:?}", stmts);
    }

    #[test]
    fn test_slash_terminator() {
        let sql = r#"CREATE PROCEDURE test_proc AS
BEGIN
  NULL;
END;
/
SELECT 1 FROM DUAL;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);
        assert_eq!(stmts.len(), 2, "Should have 2 statements, got: {:?}", stmts);
    }

    #[test]
    fn test_complete_package_with_spec_and_body() {
        let sql = r#"CREATE OR REPLACE PACKAGE oqt_pkg AS
  PROCEDURE log_msg(p_tag IN VARCHAR2, p_msg IN VARCHAR2, p_n1 IN NUMBER DEFAULT NULL);

  PROCEDURE p_basic(
    p_in_num   IN  NUMBER,
    p_in_txt   IN  VARCHAR2 DEFAULT 'DEF',
    p_out_txt  OUT VARCHAR2,
    p_inout_n  IN OUT NUMBER
  );

  PROCEDURE p_over(p_txt IN VARCHAR2);
  PROCEDURE p_over(p_num IN NUMBER, p_txt IN VARCHAR2);

  PROCEDURE p_refcur(p_tag IN VARCHAR2, p_rc OUT SYS_REFCURSOR);

  PROCEDURE p_raise(p_mode IN VARCHAR2);

  FUNCTION f_sum(p_a IN NUMBER, p_b IN NUMBER) RETURN NUMBER;
  FUNCTION f_echo(p_txt IN VARCHAR2) RETURN VARCHAR2;
  FUNCTION f_dateadd(p_d IN DATE, p_days IN NUMBER DEFAULT 1) RETURN DATE;
END oqt_pkg;
/
SHOW ERRORS PACKAGE oqt_pkg;

CREATE OR REPLACE PACKAGE BODY oqt_pkg AS
  PROCEDURE log_msg(p_tag IN VARCHAR2, p_msg IN VARCHAR2, p_n1 IN NUMBER DEFAULT NULL) IS
  BEGIN
    INSERT INTO oqt_call_log(id, tag, msg, n1, created_at)
    VALUES (oqt_call_log_seq.NEXTVAL, p_tag, p_msg, p_n1, SYSDATE);
  END;

  PROCEDURE p_basic(
    p_in_num   IN  NUMBER,
    p_in_txt   IN  VARCHAR2 DEFAULT 'DEF',
    p_out_txt  OUT VARCHAR2,
    p_inout_n  IN OUT NUMBER
  ) IS
  BEGIN
    p_out_txt := 'IN_NUM='||p_in_num||', IN_TXT='||p_in_txt||', INOUT='||p_inout_n;
    p_inout_n := NVL(p_inout_n,0) + p_in_num;

    log_msg('P_BASIC', p_out_txt, p_in_num);
    DBMS_OUTPUT.PUT_LINE('[p_basic] out='||p_out_txt||' / inout='||p_inout_n);
  END;

  PROCEDURE p_over(p_txt IN VARCHAR2) IS
  BEGIN
    log_msg('P_OVER1', p_txt);
    DBMS_OUTPUT.PUT_LINE('[p_over(txt)] '||NVL(p_txt,'<NULL>'));
  END;

  PROCEDURE p_over(p_num IN NUMBER, p_txt IN VARCHAR2) IS
  BEGIN
    log_msg('P_OVER2', p_txt, p_num);
    DBMS_OUTPUT.PUT_LINE('[p_over(num,txt)] '||p_num||' / '||NVL(p_txt,'<NULL>'));
  END;

  PROCEDURE p_refcur(p_tag IN VARCHAR2, p_rc OUT SYS_REFCURSOR) IS
  BEGIN
    OPEN p_rc FOR
      SELECT id, tag, msg, n1, created_at
      FROM oqt_call_log
      WHERE tag LIKE p_tag||'%'
      ORDER BY id DESC;
  END;

  PROCEDURE p_raise(p_mode IN VARCHAR2) IS
  BEGIN
    IF p_mode = 'NO_DATA_FOUND' THEN
      DECLARE v NUMBER;
      BEGIN
        SELECT n1 INTO v FROM oqt_call_log WHERE id = -9999;
      END;
    ELSIF p_mode = 'APP' THEN
      RAISE_APPLICATION_ERROR(-20001, 'oqt_pkg.p_raise app error');
    ELSE
      DBMS_OUTPUT.PUT_LINE('[p_raise] ok');
    END IF;
  END;

  FUNCTION f_sum(p_a IN NUMBER, p_b IN NUMBER) RETURN NUMBER IS
  BEGIN
    RETURN NVL(p_a,0) + NVL(p_b,0);
  END;

  FUNCTION f_echo(p_txt IN VARCHAR2) RETURN VARCHAR2 IS
  BEGIN
    RETURN 'ECHO:'||p_txt;
  END;

  FUNCTION f_dateadd(p_d IN DATE, p_days IN NUMBER DEFAULT 1) RETURN DATE IS
  BEGIN
    RETURN p_d + p_days;
  END;
END oqt_pkg;
/
SHOW ERRORS PACKAGE BODY oqt_pkg;"#;
        let items = QueryExecutor::split_script_items(sql);
        let stmts = get_statements(&items);

        // Count tool commands (SHOW ERRORS)
        let tool_cmds: Vec<_> = items
            .iter()
            .filter(|item| matches!(item, ScriptItem::ToolCommand(_)))
            .collect();

        if stmts.len() != 2 {
            println!(
                "\n=== FAILED: Expected 2 statements, got {} ===",
                stmts.len()
            );
            for (i, s) in stmts.iter().enumerate() {
                let preview = if s.len() > 100 { &s[..100] } else { s };
                println!("\n--- Statement {} ---\n{}...\n---", i, preview);
            }
        }

        assert_eq!(
            stmts.len(),
            2,
            "Should have 2 statements (package spec + body), got {}",
            stmts.len()
        );
        assert_eq!(
            tool_cmds.len(),
            2,
            "Should have 2 tool commands (SHOW ERRORS), got {}",
            tool_cmds.len()
        );

        // Verify package spec
        assert!(
            stmts[0].contains("CREATE OR REPLACE PACKAGE oqt_pkg AS"),
            "First statement should be package spec"
        );
        assert!(
            stmts[0].contains("END oqt_pkg"),
            "Package spec should end with END oqt_pkg"
        );
        assert!(
            !stmts[0].contains("PACKAGE BODY"),
            "Package spec should not contain BODY"
        );

        // Verify package body
        assert!(
            stmts[1].contains("CREATE OR REPLACE PACKAGE BODY oqt_pkg AS"),
            "Second statement should be package body"
        );
        assert!(
            stmts[1].contains("END oqt_pkg"),
            "Package body should end with END oqt_pkg"
        );
    }
}
