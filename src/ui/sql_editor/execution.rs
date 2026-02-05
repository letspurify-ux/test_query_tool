use fltk::{
    app,
    button::Button,
    draw::set_cursor,
    enums::{Align, CallbackTrigger, Cursor, FrameType},
    frame::Frame,
    group::{Flex, FlexType},
    input::Input,
    prelude::*,
};
use oracle::{Connection, Error as OracleError};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::db::{
    lock_connection, BindValue, BindVar, ColumnInfo, CursorResult, FormatItem, QueryExecutor,
    QueryResult, ScriptItem, SessionState, ToolCommand,
};
use crate::ui::SQL_KEYWORDS;

use super::*;

impl SqlEditorWidget {
    pub fn execute_sql_text(&self, sql: &str) {
        self.execute_sql(sql, false);
    }

    pub fn focus(&mut self) {
        self.group.show();
        let _ = self.editor.take_focus();
    }

    pub fn execute_current(&self) {
        // Check if there's a selection
        let selected_text = self.buffer.selection_text();
        if !selected_text.is_empty() {
            // Execute selected text
            self.execute_sql(&selected_text, false);
        } else {
            // Execute all text
            let sql = self.buffer.text();
            self.execute_sql(&sql, true);
        }
    }

    pub fn execute_statement_at_cursor(&self) {
        // Check if there's a selection
        let selected_text = self.buffer.selection_text();
        if !selected_text.is_empty() {
            // Execute selected text
            self.execute_sql(&selected_text, false);
        } else {
            // Execute statement at cursor position
            let sql = self.buffer.text();
            let cursor_pos = self.editor.insert_position() as usize;
            if let Some(statement) = QueryExecutor::statement_at_cursor(&sql, cursor_pos) {
                let items = QueryExecutor::split_script_items(&statement);
                if items.len() > 1 {
                    if let Some(ScriptItem::Statement(stmt)) = items
                        .iter()
                        .find(|item| matches!(item, ScriptItem::Statement(_)))
                    {
                        self.execute_sql(stmt, false);
                        return;
                    }
                }
                self.execute_sql(&statement, false);
            } else {
                fltk::dialog::alert_default("No SQL at cursor");
            }
        }
    }

    pub fn execute_selected(&self) {
        let mut buffer = self.buffer.clone();
        if !buffer.selected() {
            fltk::dialog::alert_default("No SQL selected");
            return;
        }

        let selection = buffer.selection_position();
        let insert_pos = self.editor.insert_position();
        let sql = buffer.selection_text();
        self.execute_sql(&sql, false);
        if let Some((start, end)) = selection {
            buffer.select(start, end);
            let mut editor = self.editor.clone();
            editor.set_insert_position(insert_pos);
            editor.show_insert_position();
        }
    }

    pub fn format_selected_sql(&self) {
        let mut buffer = self.buffer.clone();
        let selection = buffer.selection_position();
        let (start, end, source, select_formatted) = match selection {
            Some((start, end)) if start != end => {
                let (start, end) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                (start, end, buffer.selection_text(), true)
            }
            _ => {
                let text = buffer.text();
                let end = buffer.length();
                (0, end, text, false)
            }
        };

        let formatted = Self::format_sql_basic(&source);
        if formatted == source {
            return;
        }

        let mut editor = self.editor.clone();
        let original_pos = editor.insert_position();
        buffer.replace(start, end, &formatted);

        if select_formatted {
            buffer.select(start, start + formatted.len() as i32);
            editor.set_insert_position(start + formatted.len() as i32);
        } else {
            let new_pos = (original_pos as usize).min(formatted.len()) as i32;
            editor.set_insert_position(new_pos);
        }
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    pub fn toggle_comment(&self) {
        let mut buffer = self.buffer.clone();
        let mut editor = self.editor.clone();
        let selection = buffer.selection_position();
        let had_selection = matches!(selection, Some((start, end)) if start != end);
        let original_pos = editor.insert_position();

        let (start, end) = if let Some((start, end)) = selection {
            if start <= end {
                (start, end)
            } else {
                (end, start)
            }
        } else {
            let line_start = buffer.line_start(original_pos);
            let line_end = buffer.line_end(original_pos);
            (line_start, line_end)
        };

        let line_start = buffer.line_start(start);
        let line_end = buffer.line_end(end);
        let text = buffer.text_range(line_start, line_end).unwrap_or_default();
        let ends_with_newline = text.ends_with('\n');
        let lines: Vec<&str> = text.lines().collect();

        let all_commented = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with("--"));

        let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
        for line in lines {
            if line.trim().is_empty() {
                new_lines.push(line.to_string());
                continue;
            }

            let prefix_len = line.len() - line.trim_start().len();
            let prefix = &line[..prefix_len];
            let trimmed = &line[prefix_len..];

            if all_commented {
                let uncommented = trimmed.strip_prefix("--").unwrap_or(trimmed);
                let uncommented = if uncommented.starts_with(' ') {
                    &uncommented[1..]
                } else {
                    uncommented
                };
                new_lines.push(format!("{}{}", prefix, uncommented));
            } else if trimmed.starts_with("--") {
                new_lines.push(line.to_string());
            } else {
                new_lines.push(format!("{}-- {}", prefix, trimmed));
            }
        }

        let mut new_text = new_lines.join("\n");
        if ends_with_newline {
            new_text.push('\n');
        }

        buffer.replace(line_start, line_end, &new_text);
        let new_end = line_start + new_text.len() as i32;
        if had_selection {
            buffer.select(line_start, new_end);
            editor.set_insert_position(new_end);
        } else {
            let delta = new_text.len() as i32 - (line_end - line_start);
            let new_pos = if original_pos >= line_start {
                original_pos + delta
            } else {
                original_pos
            };
            editor.set_insert_position(new_pos);
        }
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    pub fn convert_selection_case(&self, to_upper: bool) {
        let mut buffer = self.buffer.clone();
        let selection = buffer.selection_position();
        let (start, end) = match selection {
            Some((start, end)) if start != end => {
                if start <= end {
                    (start, end)
                } else {
                    (end, start)
                }
            }
            _ => {
                fltk::dialog::alert_default("No SQL selected");
                return;
            }
        };

        let selected = buffer.selection_text();
        let converted = if to_upper {
            selected.to_uppercase()
        } else {
            selected.to_lowercase()
        };

        if converted == selected {
            return;
        }

        buffer.replace(start, end, &converted);
        buffer.select(start, start + converted.len() as i32);

        let mut editor = self.editor.clone();
        editor.set_insert_position(start + converted.len() as i32);
        editor.show_insert_position();
        self.refresh_highlighting();
    }

    pub(crate) fn format_sql_basic(sql: &str) -> String {
        let mut formatted = String::new();
        let items = QueryExecutor::split_format_items(sql);
        if items.is_empty() {
            return String::new();
        }

        for (idx, item) in items.iter().enumerate() {
            match item {
                FormatItem::Statement(statement) => {
                    let formatted_statement = Self::format_statement(statement);
                    let has_code = Self::statement_has_code(statement);
                    formatted.push_str(&formatted_statement);
                    if has_code && !Self::statement_ends_with_semicolon(&formatted_statement) {
                        formatted.push(';');
                    }
                }
                FormatItem::ToolCommand(command) => {
                    formatted.push_str(&Self::format_tool_command(command));
                }
                FormatItem::Slash => {
                    formatted.push('/');
                }
            }

            if idx + 1 < items.len() {
                if matches!(items[idx + 1], FormatItem::Slash) {
                    formatted.push('\n');
                } else if matches!(item, FormatItem::Slash) {
                    formatted.push_str("\n\n");
                } else {
                    formatted.push_str("\n\n");
                }
            }
        }

        formatted
    }

    fn statement_has_code(statement: &str) -> bool {
        let tokens = Self::tokenize_sql(statement);
        tokens
            .iter()
            .any(|token| !matches!(token, SqlToken::Comment(_)))
    }

    fn statement_ends_with_semicolon(statement: &str) -> bool {
        let tokens = Self::tokenize_sql(statement);
        for token in tokens.iter().rev() {
            match token {
                SqlToken::Comment(_) => continue,
                SqlToken::Symbol(sym) if sym == ";" => return true,
                _ => return false,
            }
        }
        false
    }

    fn format_tool_command(command: &ToolCommand) -> String {
        match command {
            ToolCommand::Var { name, data_type } => {
                format!("VAR {} {}", name, data_type.display())
            }
            ToolCommand::Print { name } => match name {
                Some(name) => format!("PRINT {}", name),
                None => "PRINT".to_string(),
            },
            ToolCommand::SetServerOutput {
                enabled,
                size,
                unlimited,
            } => {
                if !*enabled {
                    "SET SERVEROUTPUT OFF".to_string()
                } else if *unlimited {
                    "SET SERVEROUTPUT ON SIZE UNLIMITED".to_string()
                } else if let Some(size) = size {
                    format!("SET SERVEROUTPUT ON SIZE {}", size)
                } else {
                    "SET SERVEROUTPUT ON".to_string()
                }
            }
            ToolCommand::ShowErrors {
                object_type,
                object_name,
            } => {
                if let (Some(obj_type), Some(obj_name)) = (object_type, object_name) {
                    format!("SHOW ERRORS {} {}", obj_type, obj_name)
                } else {
                    "SHOW ERRORS".to_string()
                }
            }
            ToolCommand::ShowUser => "SHOW USER".to_string(),
            ToolCommand::ShowAll => "SHOW ALL".to_string(),
            ToolCommand::Describe { name } => format!("DESCRIBE {}", name),
            ToolCommand::Prompt { text } => {
                if text.trim().is_empty() {
                    "PROMPT".to_string()
                } else {
                    format!("PROMPT {}", text)
                }
            }
            ToolCommand::Pause { message } => match message {
                Some(text) if !text.trim().is_empty() => format!("PAUSE {}", text),
                _ => "PAUSE".to_string(),
            },
            ToolCommand::Accept { name, prompt } => match prompt {
                Some(text) => format!("ACCEPT {} PROMPT '{}'", name, text),
                None => format!("ACCEPT {}", name),
            },
            ToolCommand::Define { name, value } => format!("DEFINE {} = {}", name, value),
            ToolCommand::Undefine { name } => format!("UNDEFINE {}", name),
            ToolCommand::SetErrorContinue { enabled } => {
                if *enabled {
                    "SET ERRORCONTINUE ON".to_string()
                } else {
                    "SET ERRORCONTINUE OFF".to_string()
                }
            }
            ToolCommand::SetAutoCommit { enabled } => {
                if *enabled {
                    "SET AUTOCOMMIT ON".to_string()
                } else {
                    "SET AUTOCOMMIT OFF".to_string()
                }
            }
            ToolCommand::SetDefine {
                enabled,
                define_char,
            } => {
                if let Some(ch) = define_char {
                    format!("SET DEFINE '{}'", ch)
                } else if *enabled {
                    "SET DEFINE ON".to_string()
                } else {
                    "SET DEFINE OFF".to_string()
                }
            }
            ToolCommand::SetScan { enabled } => {
                if *enabled {
                    "SET SCAN ON".to_string()
                } else {
                    "SET SCAN OFF".to_string()
                }
            }
            ToolCommand::SetVerify { enabled } => {
                if *enabled {
                    "SET VERIFY ON".to_string()
                } else {
                    "SET VERIFY OFF".to_string()
                }
            }
            ToolCommand::SetEcho { enabled } => {
                if *enabled {
                    "SET ECHO ON".to_string()
                } else {
                    "SET ECHO OFF".to_string()
                }
            }
            ToolCommand::SetTiming { enabled } => {
                if *enabled {
                    "SET TIMING ON".to_string()
                } else {
                    "SET TIMING OFF".to_string()
                }
            }
            ToolCommand::SetFeedback { enabled } => {
                if *enabled {
                    "SET FEEDBACK ON".to_string()
                } else {
                    "SET FEEDBACK OFF".to_string()
                }
            }
            ToolCommand::SetHeading { enabled } => {
                if *enabled {
                    "SET HEADING ON".to_string()
                } else {
                    "SET HEADING OFF".to_string()
                }
            }
            ToolCommand::SetPageSize { size } => format!("SET PAGESIZE {}", size),
            ToolCommand::SetLineSize { size } => format!("SET LINESIZE {}", size),
            ToolCommand::Spool { path } => match path {
                Some(path) => format!("SPOOL {}", path),
                None => "SPOOL OFF".to_string(),
            },
            ToolCommand::WheneverSqlError { exit } => {
                if *exit {
                    "WHENEVER SQLERROR EXIT".to_string()
                } else {
                    "WHENEVER SQLERROR CONTINUE".to_string()
                }
            }
            ToolCommand::Exit => "EXIT".to_string(),
            ToolCommand::Quit => "QUIT".to_string(),
            ToolCommand::RunScript {
                path,
                relative_to_caller,
            } => {
                if *relative_to_caller {
                    format!("@@{}", path)
                } else {
                    format!("@{}", path)
                }
            }
            ToolCommand::Connect {
                username,
                password,
                host,
                port,
                service_name,
                ..
            } => format!(
                "CONNECT {}/{}@{}:{}/{}",
                username, password, host, port, service_name
            ),
            ToolCommand::Disconnect => "DISCONNECT".to_string(),
            ToolCommand::Unsupported { raw, .. } => raw.clone(),
        }
    }

    fn format_statement(statement: &str) -> String {
        if let Some(formatted) = Self::format_create_table(statement) {
            return formatted;
        }

        let clause_keywords = [
            "SELECT",
            "FROM",
            "WHERE",
            "GROUP",
            "HAVING",
            "ORDER",
            "UNION",
            "INTERSECT",
            "MINUS",
            "INSERT",
            "UPDATE",
            "DELETE",
            "MERGE",
            "VALUES",
            "SET",
            "INTO",
            "WITH",
        ];
        let join_modifiers = ["LEFT", "RIGHT", "FULL", "INNER", "CROSS"];
        let join_keyword = "JOIN";
        let outer_keyword = "OUTER";
        let condition_keywords = ["ON", "AND", "OR", "WHEN"]; // ELSE handled separately for IF blocks
                                                              // BEGIN is handled separately to support DECLARE ... BEGIN ... END blocks
                                                              // CASE is handled separately for SELECT vs PL/SQL context
                                                              // LOOP is handled separately for FOR ... LOOP on same line
        let block_start_keywords = ["DECLARE", "IF"];
        let block_end_qualifiers = ["LOOP", "IF", "CASE"]; // END LOOP, END IF, END CASE

        let tokens = Self::tokenize_sql(statement);
        let mut out = String::new();
        let mut indent_level = 0usize;
        let mut suppress_comma_break_depth = 0usize;
        let mut paren_stack: Vec<bool> = Vec::new();
        let mut block_stack: Vec<String> = Vec::new(); // Track which block keywords started blocks
        let mut at_line_start = true;
        let mut needs_space = false;
        let mut line_indent = 0usize;
        let mut join_modifier_active = false;
        let mut after_for_while = false; // Track FOR/WHILE for LOOP on same line
        let mut in_plsql_block = false; // Track if we're in PL/SQL block (for CASE handling)
        let mut prev_word_upper: Option<String> = None;
        let mut create_pending = false;
        let mut create_object: Option<String> = None;
        let mut routine_decl_pending = false;
        let mut create_table_paren_expected = false;
        let mut column_list_stack: Vec<bool> = Vec::new();
        let mut current_clause: Option<String> = None;
        let mut pending_package_member_separator = false;
        let mut open_cursor_pending = false;
        let mut in_open_cursor_sql = false;
        let mut open_cursor_sql_indent = 0usize;
        let mut case_branch_started: Vec<bool> = Vec::new();
        let mut between_pending = false;
        let mut last_line_is_comment = false;

        let newline_with = |out: &mut String,
                            indent_level: usize,
                            extra: usize,
                            at_line_start: &mut bool,
                            needs_space: &mut bool,
                            line_indent: &mut usize| {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            *line_indent = indent_level + extra;
            *at_line_start = true;
            *needs_space = false;
        };

        let base_indent =
            |indent_level: usize, in_open_cursor_sql: bool, open_cursor_sql_indent: usize| {
                if in_open_cursor_sql {
                    open_cursor_sql_indent
                } else {
                    indent_level
                }
            };

        let ensure_indent = |out: &mut String, at_line_start: &mut bool, line_indent: usize| {
            if *at_line_start {
                out.push_str(&" ".repeat(line_indent * 4));
                *at_line_start = false;
            }
        };

        let trim_trailing_space = |out: &mut String| {
            while out.ends_with(' ') {
                out.pop();
            }
        };

        let mut idx = 0;
        while idx < tokens.len() {
            let token = tokens[idx].clone();
            let next_word_upper = tokens[idx + 1..].iter().find_map(|t| match t {
                SqlToken::Word(w) => Some(w.to_uppercase()),
                _ => None,
            });
            let end_qualifier = {
                let mut qualifier = None;
                for t in &tokens[idx + 1..] {
                    match t {
                        SqlToken::Comment(comment) => {
                            if comment.contains('\n') {
                                break;
                            }
                        }
                        SqlToken::Word(w) => {
                            qualifier = Some(w.to_uppercase());
                            break;
                        }
                        SqlToken::Symbol(sym) if sym == ";" => break,
                        _ => break,
                    }
                }
                qualifier
            };

            match token {
                SqlToken::Word(word) => {
                    let upper = word.to_uppercase();
                    let is_keyword = SQL_KEYWORDS.iter().any(|&kw| kw == upper);
                    let is_or_in_create = upper == "OR"
                        && matches!(prev_word_upper.as_deref(), Some("CREATE"))
                        && matches!(next_word_upper.as_deref(), Some("REPLACE"));
                    let is_insert_into =
                        upper == "INTO" && matches!(prev_word_upper.as_deref(), Some("INSERT"));
                    let mut newline_after_keyword = false;
                    let is_between_and = upper == "AND" && between_pending;
                    if upper == "END" {
                        // Check if this is END LOOP, END IF, END CASE, etc.
                        let qualifier = end_qualifier.as_deref();
                        let is_qualified_end = matches!(qualifier, Some("LOOP" | "IF" | "CASE"));
                        let paren_extra = if suppress_comma_break_depth > 0 { 1 } else { 0 };

                        let case_expression_end =
                            !is_qualified_end && block_stack.last().is_some_and(|s| s == "CASE");

                        if is_qualified_end {
                            // END LOOP, END IF, END CASE - pop matching block
                            if let Some(top) = block_stack.last() {
                                if block_end_qualifiers.contains(&top.as_str()) {
                                    block_stack.pop();
                                }
                            }
                            if qualifier == Some("CASE") && !case_branch_started.is_empty() {
                                case_branch_started.pop();
                            }
                        } else {
                            if case_expression_end {
                                block_stack.pop();
                                if !case_branch_started.is_empty() {
                                    case_branch_started.pop();
                                }
                            } else {
                                // Plain END - closes BEGIN or DECLARE/PACKAGE_BODY block
                                // Pop until we find BEGIN or DECLARE/PACKAGE_BODY
                                let mut closed_block = None;
                                while let Some(top) = block_stack.pop() {
                                    if top == "BEGIN" || top == "DECLARE" || top == "PACKAGE_BODY" {
                                        closed_block = Some(top);
                                        break;
                                    }
                                }
                                if matches!(closed_block.as_deref(), Some("BEGIN" | "DECLARE"))
                                    && block_stack.last().is_some_and(|s| s == "PACKAGE_BODY")
                                {
                                    pending_package_member_separator = true;
                                }
                            }
                        }

                        if indent_level > 0 {
                            indent_level -= 1;
                        }
                        let end_extra = if !in_plsql_block && case_expression_end {
                            1
                        } else {
                            0
                        };
                        newline_with(
                            &mut out,
                            indent_level,
                            end_extra + paren_extra,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );

                        // Output "END"
                        ensure_indent(&mut out, &mut at_line_start, line_indent);
                        out.push_str("END");

                        // If qualified (END LOOP, END IF, etc.), output the qualifier and skip it
                        if is_qualified_end {
                            if let Some(q) = qualifier {
                                out.push(' ');
                                out.push_str(q);
                                // Skip the next word token (LOOP, IF, CASE)
                                idx += 1;
                                while idx < tokens.len() {
                                    if let SqlToken::Word(_) = &tokens[idx] {
                                        break;
                                    }
                                    idx += 1;
                                }
                            }
                        }
                        needs_space = true;
                        idx += 1;
                        continue;
                    } else if clause_keywords.contains(&upper.as_str()) && !is_insert_into {
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                        current_clause = Some(upper.clone());
                        if upper == "SELECT" && in_open_cursor_sql {
                            // Keep OPEN ... FOR SELECT inside the cursor SQL context.
                            open_cursor_pending = false;
                        }
                    } else if condition_keywords.contains(&upper.as_str())
                        && !is_or_in_create
                        && !is_between_and
                    {
                        let paren_extra = if suppress_comma_break_depth > 0 { 1 } else { 0 };
                        if upper == "WHEN"
                            && block_stack.last().is_some_and(|s| s == "CASE")
                            && case_branch_started.last().is_some()
                        {
                            let started = case_branch_started.last().copied().unwrap_or(false);
                            if started && !last_line_is_comment {
                                if !out.ends_with('\n') {
                                    out.push('\n');
                                }
                                if !out.ends_with("\n\n") {
                                    out.push('\n');
                                }
                            }
                            if let Some(last) = case_branch_started.last_mut() {
                                *last = true;
                            }
                        }
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            1 + paren_extra,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if upper == "CREATE" {
                        create_pending = true;
                        create_object = None;
                    } else if create_pending && (upper == "OR" || upper == "REPLACE") {
                        // part of CREATE OR REPLACE
                    } else if create_pending && upper == "PACKAGE" {
                        if matches!(next_word_upper.as_deref(), Some("BODY")) {
                            create_object = Some("PACKAGE_BODY".to_string());
                        } else {
                            create_object = Some("PACKAGE".to_string());
                        }
                        create_pending = false;
                    } else if create_pending && upper == "TABLE" {
                        create_table_paren_expected = true;
                        create_pending = false;
                    } else if create_pending
                        && matches!(
                            upper.as_str(),
                            "PROCEDURE" | "FUNCTION" | "TYPE" | "TRIGGER"
                        )
                    {
                        create_object = Some(upper.clone());
                        create_pending = false;
                    } else if matches!(upper.as_str(), "PROCEDURE" | "FUNCTION")
                        && block_stack.iter().any(|s| s == "PACKAGE_BODY")
                    {
                        routine_decl_pending = true;
                    } else if upper == "ELSE" || upper == "ELSIF" {
                        // ELSE/ELSIF in IF block: same level as IF
                        let in_if_block = block_stack.last().is_some_and(|s| s == "IF");
                        let in_case_block = block_stack.last().is_some_and(|s| s == "CASE");
                        let paren_extra = if suppress_comma_break_depth > 0 { 1 } else { 0 };
                        if upper == "ELSE"
                            && in_case_block
                            && case_branch_started.last().is_some()
                            && !in_if_block
                        {
                            let started = case_branch_started.last().copied().unwrap_or(false);
                            if started && !last_line_is_comment {
                                if !out.ends_with('\n') {
                                    out.push('\n');
                                }
                                if !out.ends_with("\n\n") {
                                    out.push('\n');
                                }
                            }
                            if let Some(last) = case_branch_started.last_mut() {
                                *last = true;
                            }
                        }
                        if in_if_block {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level.saturating_sub(1),
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        } else {
                            // ELSE in CASE or other context
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                1 + paren_extra,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        if upper == "ELSE"
                            && in_plsql_block
                            && !matches!(current_clause.as_deref(), Some("SELECT"))
                        {
                            newline_after_keyword = true;
                        }
                    } else if upper == "THEN" {
                        if in_plsql_block && !matches!(current_clause.as_deref(), Some("SELECT")) {
                            newline_after_keyword = true;
                        }
                    } else if upper == join_keyword {
                        if !join_modifier_active {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        join_modifier_active = false;
                    } else if join_modifiers.contains(&upper.as_str()) {
                        if matches!(next_word_upper.as_deref(), Some("JOIN" | "OUTER")) {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            join_modifier_active = true;
                        }
                    } else if upper == outer_keyword {
                        if matches!(next_word_upper.as_deref(), Some("JOIN"))
                            && !join_modifier_active
                        {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                1,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            join_modifier_active = true;
                        }
                    } else if upper == "OPEN" {
                        open_cursor_pending = true;
                    } else if upper == "FOR" || upper == "WHILE" {
                        if upper == "FOR" && open_cursor_pending {
                            open_cursor_pending = false;
                            in_open_cursor_sql = true;
                            open_cursor_sql_indent = indent_level.saturating_add(1);
                            newline_after_keyword = true;
                        } else {
                            // FOR/WHILE starts a line, LOOP will follow on same line
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            after_for_while = true;
                        }
                    } else if upper == "LOOP" {
                        // LOOP after FOR/WHILE stays on same line
                        if !after_for_while {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        after_for_while = false;
                        if in_plsql_block {
                            newline_after_keyword = true;
                        }
                    } else if upper == "CASE" {
                        // CASE in PL/SQL block vs SELECT context
                        if in_plsql_block {
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        } else if matches!(current_clause.as_deref(), Some("SELECT")) {
                            let paren_extra = if suppress_comma_break_depth > 0 { 1 } else { 0 };
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                1 + paren_extra,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                        // In SELECT context, CASE stays inline
                    } else if block_start_keywords.contains(&upper.as_str()) {
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if upper == "BEGIN" {
                        // BEGIN handling: check if we're inside a DECLARE block
                        let inside_declare = block_stack
                            .last()
                            .map_or(false, |s| s == "DECLARE" || s == "PACKAGE_BODY");
                        if inside_declare {
                            // DECLARE ... BEGIN - BEGIN is at same level as DECLARE
                            // Don't increase indent, just newline at current level
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level.saturating_sub(1),
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        } else {
                            // Standalone BEGIN block
                            newline_with(
                                &mut out,
                                base_indent(
                                    indent_level,
                                    in_open_cursor_sql,
                                    open_cursor_sql_indent,
                                ),
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                        }
                    }

                    let started_line = at_line_start;
                    ensure_indent(&mut out, &mut at_line_start, line_indent);
                    if needs_space {
                        out.push(' ');
                    }
                    if is_keyword {
                        out.push_str(&upper);
                    } else {
                        out.push_str(&word);
                    }
                    needs_space = true;
                    if started_line {
                        last_line_is_comment = false;
                    }

                    if create_table_paren_expected
                        && upper == "AS"
                        && matches!(next_word_upper.as_deref(), Some("SELECT" | "WITH"))
                    {
                        create_table_paren_expected = false;
                    }

                    let starts_create_block = matches!(upper.as_str(), "AS" | "IS")
                        && (create_object.is_some() || routine_decl_pending);

                    // Handle block start - push to stack and increase indent
                    if block_start_keywords.contains(&upper.as_str()) {
                        block_stack.push(upper.clone());
                        indent_level += 1;
                        if upper == "DECLARE" || upper == "IF" {
                            in_plsql_block = true;
                        }
                    } else if upper == "BEGIN" {
                        let inside_declare = block_stack.last().map_or(false, |s| s == "DECLARE");
                        if inside_declare {
                            // Replace DECLARE with BEGIN on the stack (same block continues)
                            block_stack.pop();
                            block_stack.push("BEGIN".to_string());
                            // indent_level stays the same
                        } else {
                            // Standalone BEGIN block
                            block_stack.push("BEGIN".to_string());
                            indent_level += 1;
                        }
                        in_plsql_block = true;
                    } else if upper == "LOOP" {
                        block_stack.push("LOOP".to_string());
                        indent_level += 1;
                    } else if upper == "CASE" {
                        block_stack.push("CASE".to_string());
                        if in_plsql_block && current_clause.is_none() {
                            case_branch_started.push(false);
                        }
                        indent_level += 1;
                    } else if starts_create_block {
                        // Treat AS/IS in CREATE PACKAGE/PROC/FUNC/TYPE/TRIGGER and package-body routines as declaration section start
                        let is_package_body =
                            matches!(create_object.as_deref(), Some("PACKAGE_BODY"));
                        if is_package_body {
                            block_stack.push("PACKAGE_BODY".to_string());
                        } else {
                            block_stack.push("DECLARE".to_string());
                        }
                        indent_level += 1;
                        in_plsql_block = true;
                        create_object = None;
                        routine_decl_pending = false;
                        newline_with(
                            &mut out,
                            indent_level,
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    }

                    if upper == "DECLARE" || upper == "BEGIN" {
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    }

                    if newline_after_keyword {
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    }

                    if upper == "BETWEEN" {
                        between_pending = true;
                    } else if upper == "AND" && between_pending {
                        between_pending = false;
                    }

                    prev_word_upper = Some(upper);
                }
                SqlToken::String(literal) => {
                    let started_line = at_line_start;
                    ensure_indent(&mut out, &mut at_line_start, line_indent);
                    if needs_space {
                        out.push(' ');
                    }
                    out.push_str(&literal);
                    needs_space = true;
                    if literal.contains('\n') {
                        at_line_start = true;
                    }
                    if started_line {
                        last_line_is_comment = false;
                    }
                }
                SqlToken::Comment(comment) => {
                    let has_leading_newline = comment.starts_with('\n');
                    let comment_body = if has_leading_newline {
                        &comment[1..]
                    } else {
                        comment.as_str()
                    };
                    let trimmed_comment = comment_body.trim_end_matches('\n');
                    let is_block_comment =
                        trimmed_comment.starts_with("/*") && trimmed_comment.ends_with("*/");
                    let next_is_word_like = matches!(
                        tokens.get(idx + 1),
                        Some(SqlToken::Word(_) | SqlToken::String(_))
                    );

                    if has_leading_newline {
                        newline_with(
                            &mut out,
                            base_indent(indent_level, in_open_cursor_sql, open_cursor_sql_indent),
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                    } else if !at_line_start {
                        out.push(' ');
                    }

                    let comment_starts_line = at_line_start;
                    if is_block_comment {
                        if at_line_start {
                            at_line_start = false;
                        }
                    } else {
                        if comment_starts_line {
                            line_indent = base_indent(
                                indent_level,
                                in_open_cursor_sql,
                                open_cursor_sql_indent,
                            );
                        }
                        ensure_indent(&mut out, &mut at_line_start, line_indent);
                    }

                    out.push_str(comment_body);

                    needs_space = true;
                    if comment_body.ends_with('\n') || comment_body.contains('\n') {
                        at_line_start = true;
                        needs_space = false;
                        if comment_starts_line {
                            last_line_is_comment = true;
                        }
                    } else if is_block_comment && next_is_word_like {
                        newline_with(
                            &mut out,
                            indent_level,
                            0,
                            &mut at_line_start,
                            &mut needs_space,
                            &mut line_indent,
                        );
                        last_line_is_comment = false;
                    } else if comment_starts_line {
                        last_line_is_comment = true;
                    }
                }
                SqlToken::Symbol(sym) => {
                    let started_line = at_line_start;
                    match sym.as_str() {
                        "," => {
                            trim_trailing_space(&mut out);
                            out.push(',');
                            between_pending = false;
                            if column_list_stack.last().copied().unwrap_or(false) {
                                newline_with(
                                    &mut out,
                                    base_indent(
                                        indent_level,
                                        in_open_cursor_sql,
                                        open_cursor_sql_indent,
                                    ),
                                    1,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            } else if suppress_comma_break_depth == 0 {
                                newline_with(
                                    &mut out,
                                    base_indent(
                                        indent_level,
                                        in_open_cursor_sql,
                                        open_cursor_sql_indent,
                                    ),
                                    1,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            } else {
                                out.push(' ');
                                needs_space = false;
                            }
                        }
                        ";" => {
                            trim_trailing_space(&mut out);
                            out.push(';');
                            current_clause = None;
                            open_cursor_pending = false;
                            in_open_cursor_sql = false;
                            open_cursor_sql_indent = 0;
                            between_pending = false;
                            if pending_package_member_separator
                                && matches!(
                                    next_word_upper.as_deref(),
                                    Some("PROCEDURE" | "FUNCTION")
                                )
                            {
                                out.push_str("\n\n");
                            }
                            pending_package_member_separator = false;
                            routine_decl_pending = false;
                            if indent_level == 0 {
                                // Recover newline/comma wrapping behavior for the next top-level section
                                // even if we encountered an unmatched parenthesis earlier in the statement.
                                suppress_comma_break_depth = 0;
                                paren_stack.clear();
                                column_list_stack.clear();
                            }
                            newline_with(
                                &mut out,
                                indent_level,
                                0,
                                &mut at_line_start,
                                &mut needs_space,
                                &mut line_indent,
                            );
                            if indent_level == 0 {
                                out.push('\n');
                                at_line_start = true;
                                needs_space = false;
                            }
                        }
                        "(" => {
                            if matches!(current_clause.as_deref(), Some("SELECT"))
                                && matches!(prev_word_upper.as_deref(), Some("SELECT"))
                            {
                                newline_with(
                                    &mut out,
                                    base_indent(
                                        indent_level,
                                        in_open_cursor_sql,
                                        open_cursor_sql_indent,
                                    ),
                                    1,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            }

                            ensure_indent(&mut out, &mut at_line_start, line_indent);
                            let is_subquery = matches!(
                                next_word_upper.as_deref(),
                                Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "MERGE")
                            );
                            if needs_space {
                                out.push(' ');
                            }
                            out.push('(');
                            let is_column_list = create_table_paren_expected;
                            create_table_paren_expected = false;
                            paren_stack.push(is_subquery);
                            column_list_stack.push(is_column_list);
                            if is_subquery || is_column_list {
                                indent_level += 1;
                                newline_with(
                                    &mut out,
                                    base_indent(
                                        indent_level,
                                        in_open_cursor_sql,
                                        open_cursor_sql_indent,
                                    ),
                                    0,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                            } else {
                                suppress_comma_break_depth += 1;
                            }
                            needs_space = false;
                        }
                        ")" => {
                            trim_trailing_space(&mut out);
                            let was_subquery = paren_stack.pop().unwrap_or(false);
                            let was_column_list = column_list_stack.pop().unwrap_or(false);
                            if was_subquery || was_column_list {
                                if indent_level > 0 {
                                    indent_level -= 1;
                                }
                                newline_with(
                                    &mut out,
                                    base_indent(
                                        indent_level,
                                        in_open_cursor_sql,
                                        open_cursor_sql_indent,
                                    ),
                                    0,
                                    &mut at_line_start,
                                    &mut needs_space,
                                    &mut line_indent,
                                );
                                ensure_indent(&mut out, &mut at_line_start, line_indent);
                            } else if suppress_comma_break_depth > 0 {
                                suppress_comma_break_depth -= 1;
                            }
                            out.push(')');
                            needs_space = true;
                        }
                        "." => {
                            trim_trailing_space(&mut out);
                            out.push('.');
                            needs_space = false;
                        }
                        _ => {
                            ensure_indent(&mut out, &mut at_line_start, line_indent);
                            if needs_space {
                                out.push(' ');
                            }
                            out.push_str(&sym);
                            // For bind variables (:name) and assignment (:=), don't add space after colon
                            // Check if this is ":" and next token is a Word (bind variable)
                            let is_bind_var_colon = sym == ":"
                                && tokens
                                    .get(idx + 1)
                                    .map_or(false, |t| matches!(t, SqlToken::Word(_)));
                            needs_space = !is_bind_var_colon;
                        }
                    }
                    if started_line {
                        last_line_is_comment = false;
                    }
                }
            }

            idx += 1;
        }

        let formatted = out.trim_end().to_string();
        Self::apply_parser_depth_indentation(&formatted)
    }

    fn apply_parser_depth_indentation(formatted: &str) -> String {
        if formatted.is_empty() || !Self::is_plsql_like_statement(formatted) {
            return formatted.to_string();
        }

        let depths = QueryExecutor::line_block_depths(formatted);
        if depths.len() != formatted.lines().count() {
            return formatted.to_string();
        }

        let mut out = String::new();
        let mut into_list_active = false;
        let mut in_dml_statement = false;
        for (idx, (line, depth)) in formatted.lines().zip(depths.iter()).enumerate() {
            if idx > 0 {
                out.push('\n');
            }

            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                out.push_str(trimmed);
                continue;
            }

            let is_comment =
                trimmed.starts_with("--") || trimmed.starts_with("/*") || trimmed == "*/";
            if is_comment {
                let leading_spaces = line.len().saturating_sub(trimmed.len());
                let existing_indent = leading_spaces / 4;
                let extra_indent = if into_list_active { 1 } else { 0 };
                let effective_depth = (*depth + extra_indent).max(existing_indent);
                out.push_str(&" ".repeat(effective_depth * 4));
                out.push_str(trimmed);
                continue;
            }

            let trimmed_upper = trimmed.to_uppercase();
            let starts_dml = trimmed_upper.starts_with("SELECT ")
                || trimmed_upper.starts_with("INSERT ")
                || trimmed_upper.starts_with("UPDATE ")
                || trimmed_upper.starts_with("DELETE ")
                || trimmed_upper.starts_with("MERGE ");
            if starts_dml {
                in_dml_statement = true;
            }
            let starts_into = trimmed_upper.starts_with("INTO ");
            let starts_into_ender = trimmed_upper.starts_with("FROM ")
                || trimmed_upper.starts_with("WHERE ")
                || trimmed_upper.starts_with("ORDER ")
                || trimmed_upper.starts_with("VALUES ")
                || trimmed_upper.starts_with("END")
                || trimmed_upper.starts_with("EXCEPTION")
                || trimmed_upper.starts_with("ELSIF")
                || trimmed_upper.starts_with("ELSE")
                || trimmed_upper.starts_with("WHEN ")
                || trimmed_upper.starts_with("BEGIN")
                || trimmed_upper.starts_with("LOOP")
                || trimmed_upper.starts_with("CASE")
                || trimmed_upper.starts_with("SELECT ")
                || trimmed_upper.starts_with("INSERT ")
                || trimmed_upper.starts_with("UPDATE ")
                || trimmed_upper.starts_with("DELETE ")
                || trimmed_upper.starts_with("MERGE ")
                || trimmed_upper.starts_with("FETCH ")
                || trimmed_upper.starts_with("OPEN ")
                || trimmed_upper.starts_with("CLOSE ")
                || trimmed_upper.starts_with("RETURN ")
                || trimmed_upper.starts_with("EXIT");
            let extra_indent = if into_list_active && !starts_into_ender {
                1
            } else {
                0
            };
            let force_block_depth = !in_dml_statement
                && (trimmed_upper.starts_with("EXCEPTION")
                    || trimmed_upper.starts_with("WHEN ")
                    || trimmed_upper.starts_with("ELSE")
                    || trimmed_upper.starts_with("ELSIF")
                    || trimmed_upper.starts_with("END")
                    || trimmed_upper.starts_with("BEGIN")
                    || trimmed_upper.starts_with("CASE")
                    || trimmed_upper.starts_with("IF ")
                    || trimmed_upper.starts_with("LOOP")
                    || trimmed_upper.starts_with("FOR ")
                    || trimmed_upper.starts_with("WHILE ")
                    || trimmed_upper.starts_with("DECLARE"));

            let leading_spaces = line.len().saturating_sub(trimmed.len());
            let existing_indent = leading_spaces / 4;
            let effective_depth = if force_block_depth {
                *depth + extra_indent
            } else {
                (*depth + extra_indent).max(existing_indent)
            };
            out.push_str(&" ".repeat(effective_depth * 4));
            out.push_str(trimmed);

            if starts_into_ender {
                into_list_active = false;
            }
            if starts_into {
                into_list_active = true;
            }
            if trimmed.ends_with(';') {
                in_dml_statement = false;
            }
        }

        out
    }

    fn is_plsql_like_statement(statement: &str) -> bool {
        let upper = statement.to_uppercase();
        upper.contains("BEGIN")
            || upper.contains("DECLARE")
            || upper.contains("CREATE OR REPLACE")
            || upper.contains("CREATE PACKAGE")
            || upper.contains("CREATE PROCEDURE")
            || upper.contains("CREATE FUNCTION")
            || upper.contains("CREATE TRIGGER")
    }

    fn format_create_table(statement: &str) -> Option<String> {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            return None;
        }

        let tokens = Self::tokenize_sql(trimmed);
        if tokens.is_empty() {
            return None;
        }

        let mut seen_create = false;
        let mut seen_table = false;
        let mut ctas = false;
        let mut depth = 0i32;
        let mut open_idx: Option<usize> = None;
        let mut close_idx: Option<usize> = None;
        let mut idx = 0usize;

        while idx < tokens.len() {
            let token = &tokens[idx];
            match token {
                SqlToken::Word(word) => {
                    let upper = word.to_uppercase();
                    if upper == "CREATE" {
                        seen_create = true;
                    } else if seen_create && upper == "TABLE" {
                        seen_table = true;
                    } else if seen_table && upper == "AS" {
                        if tokens[idx + 1..]
                            .iter()
                            .find_map(|t| match t {
                                SqlToken::Word(w) => Some(w.to_uppercase()),
                                _ => None,
                            })
                            .is_some_and(|w| w == "SELECT" || w == "WITH")
                        {
                            ctas = true;
                        }
                    }
                }
                SqlToken::Symbol(sym) if sym == "(" => {
                    if depth == 0 && seen_create && seen_table && !ctas && open_idx.is_none() {
                        open_idx = Some(idx);
                    }
                    depth += 1;
                }
                SqlToken::Symbol(sym) if sym == ")" => {
                    depth -= 1;
                    if depth == 0 && open_idx.is_some() && close_idx.is_none() {
                        close_idx = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }

        let (open_idx, close_idx) = match (open_idx, close_idx) {
            (Some(open_idx), Some(close_idx)) => (open_idx, close_idx),
            _ => return None,
        };

        let prefix_tokens = &tokens[..open_idx];
        let column_tokens = &tokens[open_idx + 1..close_idx];
        let suffix_tokens = &tokens[close_idx + 1..];

        let mut columns: Vec<Vec<SqlToken>> = Vec::new();
        let mut current: Vec<SqlToken> = Vec::new();
        let mut col_depth = 0i32;

        for token in column_tokens {
            match token {
                SqlToken::Symbol(sym) if sym == "(" => {
                    col_depth += 1;
                    current.push(token.clone());
                }
                SqlToken::Symbol(sym) if sym == ")" => {
                    col_depth = col_depth.saturating_sub(1);
                    current.push(token.clone());
                }
                SqlToken::Symbol(sym) if sym == "," && col_depth == 0 => {
                    if !current.is_empty() {
                        columns.push(current);
                        current = Vec::new();
                    }
                }
                _ => current.push(token.clone()),
            }
        }
        if !current.is_empty() {
            columns.push(current);
        }

        if columns.is_empty() {
            return None;
        }

        let mut formatted_cols: Vec<(bool, String, String, String)> = Vec::new();
        let mut max_name = 0usize;
        let mut max_type = 0usize;

        for column in &columns {
            let mut iter = column.iter().filter(|t| !matches!(t, SqlToken::Comment(_)));
            let first = iter.next();
            let is_constraint = match first {
                Some(SqlToken::Word(word)) => {
                    matches!(
                        word.to_uppercase().as_str(),
                        "CONSTRAINT" | "PRIMARY" | "UNIQUE" | "FOREIGN" | "CHECK"
                    )
                }
                _ => false,
            };

            if is_constraint {
                let text = Self::join_tokens_spaced(column, 0);
                formatted_cols.push((true, text, String::new(), String::new()));
                continue;
            }

            let mut tokens_iter = column.iter().peekable();
            let name_token = tokens_iter.next();
            let name = name_token.map(|t| Self::token_text(t)).unwrap_or_default();

            let mut type_tokens: Vec<SqlToken> = Vec::new();
            let mut rest_tokens: Vec<SqlToken> = Vec::new();
            let mut in_type = true;
            let constraint_keywords = [
                "CONSTRAINT",
                "NOT",
                "NULL",
                "DEFAULT",
                "PRIMARY",
                "UNIQUE",
                "CHECK",
                "REFERENCES",
                "ENABLE",
                "DISABLE",
                "USING",
                "COLLATE",
                "GENERATED",
                "IDENTITY",
            ];

            for token in tokens_iter {
                let is_constraint_token = match token {
                    SqlToken::Word(word) => {
                        constraint_keywords.contains(&word.to_uppercase().as_str())
                    }
                    _ => false,
                };
                if in_type && is_constraint_token {
                    in_type = false;
                }
                if in_type {
                    type_tokens.push(token.clone());
                } else {
                    rest_tokens.push(token.clone());
                }
            }

            let type_str = Self::join_tokens_compact(&type_tokens);
            let rest_str = Self::join_tokens_spaced(&rest_tokens, 0);

            max_name = max_name.max(name.len());
            max_type = max_type.max(type_str.len());
            formatted_cols.push((false, name, type_str, rest_str));
        }

        let mut out = String::new();
        let prefix = Self::join_tokens_spaced(prefix_tokens, 0);
        out.push_str(prefix.trim_end());
        out.push_str(" (\n");

        let indent = " ".repeat(4);
        for (idx, (is_constraint, name, type_str, rest_str)) in
            formatted_cols.into_iter().enumerate()
        {
            out.push_str(&indent);
            if is_constraint {
                out.push_str(&name);
            } else {
                let name_pad = max_name.saturating_sub(name.len());
                let type_pad = max_type.saturating_sub(type_str.len());
                out.push_str(&name);
                if !type_str.is_empty() {
                    out.push_str(&" ".repeat(name_pad + 1));
                    out.push_str(&type_str);
                    if !rest_str.is_empty() {
                        out.push_str(&" ".repeat(type_pad + 1));
                        out.push_str(&rest_str);
                    }
                }
            }
            if idx + 1 < columns.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push(')');

        let suffix = Self::format_create_suffix(suffix_tokens);
        if !suffix.is_empty() {
            out.push('\n');
            out.push_str(&suffix);
        }

        Some(out.trim_end().to_string())
    }

    fn token_text(token: &SqlToken) -> String {
        match token {
            SqlToken::Word(word) => {
                let upper = word.to_uppercase();
                if SQL_KEYWORDS.iter().any(|&kw| kw == upper) {
                    upper
                } else {
                    word.clone()
                }
            }
            SqlToken::String(literal) => literal.clone(),
            SqlToken::Comment(comment) => comment.clone(),
            SqlToken::Symbol(sym) => sym.clone(),
        }
    }

    fn join_tokens_compact(tokens: &[SqlToken]) -> String {
        let mut out = String::new();
        let mut needs_space = false;
        for token in tokens {
            let text = Self::token_text(token);
            match token {
                SqlToken::Symbol(sym) if sym == "(" => {
                    out.push_str(&text);
                    needs_space = false;
                }
                SqlToken::Symbol(sym) if sym == ")" => {
                    out.push_str(&text);
                    needs_space = true;
                }
                SqlToken::Symbol(sym) if sym == "," => {
                    out.push_str(&text);
                    out.push(' ');
                    needs_space = false;
                }
                _ => {
                    if needs_space {
                        out.push(' ');
                    }
                    out.push_str(&text);
                    needs_space = true;
                }
            }
        }
        out.trim().to_string()
    }

    fn join_tokens_spaced(tokens: &[SqlToken], indent_level: usize) -> String {
        let mut out = String::new();
        let mut needs_space = false;
        let indent = " ".repeat(indent_level * 4);
        let mut at_line_start = true;

        for token in tokens {
            let text = Self::token_text(token);
            match token {
                SqlToken::Comment(comment) => {
                    if !at_line_start {
                        out.push(' ');
                    } else if !indent.is_empty() {
                        out.push_str(&indent);
                    }
                    out.push_str(comment);
                    if comment.ends_with('\n') {
                        at_line_start = true;
                        needs_space = false;
                    } else {
                        at_line_start = false;
                        needs_space = true;
                    }
                }
                SqlToken::Symbol(sym) if sym == "." => {
                    out.push('.');
                    needs_space = false;
                    at_line_start = false;
                }
                SqlToken::Symbol(sym) if sym == "(" => {
                    out.push('(');
                    needs_space = false;
                    at_line_start = false;
                }
                SqlToken::Symbol(sym) if sym == ")" => {
                    out.push(')');
                    needs_space = true;
                    at_line_start = false;
                }
                SqlToken::Symbol(sym) if sym == "," => {
                    out.push(',');
                    out.push(' ');
                    needs_space = false;
                    at_line_start = false;
                }
                SqlToken::Symbol(sym) => {
                    if needs_space {
                        out.push(' ');
                    }
                    out.push_str(sym);
                    needs_space = true;
                    at_line_start = false;
                }
                _ => {
                    if at_line_start && !indent.is_empty() {
                        out.push_str(&indent);
                    }
                    if needs_space {
                        out.push(' ');
                    }
                    out.push_str(&text);
                    needs_space = true;
                    at_line_start = false;
                }
            }
        }

        out.trim().to_string()
    }

    fn format_create_suffix(tokens: &[SqlToken]) -> String {
        if tokens.is_empty() {
            return String::new();
        }

        let break_keywords = [
            "PCTFREE",
            "PCTUSED",
            "INITRANS",
            "MAXTRANS",
            "COMPRESS",
            "NOCOMPRESS",
            "LOGGING",
            "NOLOGGING",
            "STORAGE",
            "TABLESPACE",
            "USING",
            "ENABLE",
            "DISABLE",
            "CACHE",
            "NOCACHE",
            "PARALLEL",
            "NOPARALLEL",
            "MONITORING",
            "NOMONITORING",
            "ORGANIZATION",
            "INCLUDING",
            "LOB",
            "PARTITION",
            "SUBPARTITION",
            "SHARING",
        ];

        let mut parts: Vec<Vec<SqlToken>> = Vec::new();
        let mut current: Vec<SqlToken> = Vec::new();
        let mut depth = 0i32;

        for token in tokens {
            match token {
                SqlToken::Symbol(sym) if sym == "(" => {
                    depth += 1;
                    current.push(token.clone());
                }
                SqlToken::Symbol(sym) if sym == ")" => {
                    depth = depth.saturating_sub(1);
                    current.push(token.clone());
                }
                SqlToken::Word(word) if depth == 0 => {
                    let upper = word.to_uppercase();
                    if break_keywords.contains(&upper.as_str()) && !current.is_empty() {
                        parts.push(current);
                        current = Vec::new();
                    }
                    current.push(token.clone());
                }
                _ => current.push(token.clone()),
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }

        let mut out = String::new();
        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&Self::join_tokens_spaced(part, 0));
        }
        out.trim().to_string()
    }

    pub fn tokenize_sql(sql: &str) -> Vec<SqlToken> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = sql.chars().collect();
        let mut i = 0;
        let mut current = String::new();

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut in_q_quote = false;
        let mut q_quote_end: Option<char> = None;
        let mut pending_newline = false;

        let flush_word = |current: &mut String, tokens: &mut Vec<SqlToken>| {
            if !current.is_empty() {
                tokens.push(SqlToken::Word(std::mem::take(current)));
            }
        };

        while i < chars.len() {
            let c = chars[i];
            let next = if i + 1 < chars.len() {
                Some(chars[i + 1])
            } else {
                None
            };

            if in_line_comment {
                current.push(c);
                if c == '\n' {
                    tokens.push(SqlToken::Comment(std::mem::take(&mut current)));
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                current.push(c);
                if c == '*' && next == Some('/') {
                    current.push('/');
                    if i + 2 < chars.len() && chars[i + 2] == '\n' {
                        current.push('\n');
                        i += 1;
                    }
                    tokens.push(SqlToken::Comment(std::mem::take(&mut current)));
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_q_quote {
                current.push(c);
                if Some(c) == q_quote_end && next == Some('\'') {
                    current.push('\'');
                    tokens.push(SqlToken::String(std::mem::take(&mut current)));
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
                    tokens.push(SqlToken::String(std::mem::take(&mut current)));
                    in_single_quote = false;
                    i += 1;
                    continue;
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
                    tokens.push(SqlToken::String(std::mem::take(&mut current)));
                    in_double_quote = false;
                    i += 1;
                    continue;
                }
                i += 1;
                continue;
            }

            if c.is_whitespace() {
                flush_word(&mut current, &mut tokens);
                if c == '\n' {
                    pending_newline = true;
                }
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                flush_word(&mut current, &mut tokens);
                in_line_comment = true;
                if pending_newline {
                    current.push('\n');
                }
                current.push('-');
                current.push('-');
                pending_newline = false;
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                flush_word(&mut current, &mut tokens);
                in_block_comment = true;
                if pending_newline {
                    current.push('\n');
                }
                current.push('/');
                current.push('*');
                pending_newline = false;
                i += 2;
                continue;
            }

            pending_newline = false;

            // Handle nq-quoted strings: nq'[...]', nq'{...}', etc. (National Character Set)
            if (c == 'n' || c == 'N')
                && (next == Some('q') || next == Some('Q'))
                && i + 2 < chars.len()
                && chars[i + 2] == '\''
                && i + 3 < chars.len()
            {
                let delimiter = chars[i + 3];
                let closing = match delimiter {
                    '[' => ']',
                    '{' => '}',
                    '(' => ')',
                    '<' => '>',
                    _ => delimiter,
                };
                flush_word(&mut current, &mut tokens);
                current.push(c);
                current.push(chars[i + 1]);
                current.push('\'');
                current.push(delimiter);
                in_q_quote = true;
                q_quote_end = Some(closing);
                i += 4;
                continue;
            }

            // Handle q-quoted strings: q'[...]', q'{...}', q'(...)', q'<...>', q'!...!'
            if (c == 'q' || c == 'Q') && next == Some('\'') && i + 2 < chars.len() {
                let delimiter = chars[i + 2];
                let closing = match delimiter {
                    '[' => ']',
                    '{' => '}',
                    '(' => ')',
                    '<' => '>',
                    _ => delimiter,
                };
                flush_word(&mut current, &mut tokens);
                current.push(c);
                current.push('\'');
                current.push(delimiter);
                in_q_quote = true;
                q_quote_end = Some(closing);
                i += 3;
                continue;
            }

            if c == '\'' {
                flush_word(&mut current, &mut tokens);
                in_single_quote = true;
                current.push('\'');
                i += 1;
                continue;
            }

            if c == '"' {
                flush_word(&mut current, &mut tokens);
                in_double_quote = true;
                current.push('"');
                i += 1;
                continue;
            }

            if c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '#' {
                current.push(c);
                i += 1;
                continue;
            }

            flush_word(&mut current, &mut tokens);

            // Handle <<label>> (Oracle PL/SQL labels)
            if c == '<' && next == Some('<') {
                let mut label = String::from("<<");
                let mut j = i + 2;
                while j < chars.len() {
                    let ch = chars[j];
                    label.push(ch);
                    if ch == '>' && j + 1 < chars.len() && chars[j + 1] == '>' {
                        label.push('>');
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                tokens.push(SqlToken::Word(label));
                i = j;
                continue;
            }

            let sym = match (c, next) {
                ('<', Some('=')) => Some("<=".to_string()),
                ('>', Some('=')) => Some(">=".to_string()),
                ('<', Some('>')) => Some("<>".to_string()),
                ('!', Some('=')) => Some("!=".to_string()),
                ('|', Some('|')) => Some("||".to_string()),
                (':', Some('=')) => Some(":=".to_string()),
                ('=', Some('>')) => Some("=>".to_string()),
                _ => None,
            };

            if let Some(sym) = sym {
                tokens.push(SqlToken::Symbol(sym));
                i += 2;
                continue;
            }

            tokens.push(SqlToken::Symbol(c.to_string()));
            i += 1;
        }

        if in_line_comment || in_block_comment {
            if !current.is_empty() {
                tokens.push(SqlToken::Comment(std::mem::take(&mut current)));
            }
        } else if in_single_quote || in_double_quote || in_q_quote {
            if !current.is_empty() {
                tokens.push(SqlToken::String(std::mem::take(&mut current)));
            }
        } else {
            flush_word(&mut current, &mut tokens);
        }
        tokens
    }

    fn execute_sql(&self, sql: &str, script_mode: bool) {
        if sql.trim().is_empty() {
            return;
        }

        if *self.query_running.borrow() {
            fltk::dialog::alert_default("A query is already running");
            return;
        }

        // Check if this is a CONNECT or DISCONNECT command
        // These commands should work even when not connected
        let sql_upper = sql.trim().to_uppercase();
        let is_connect_command = sql_upper.starts_with("CONNECT")
            || sql_upper.starts_with("CONN ")
            || sql_upper.starts_with("DISCONNECT")
            || sql_upper.starts_with("DISC")
            || sql_upper.starts_with('@');

        let conn_guard = lock_connection(&self.connection);

        // Only check connection status if this is not a CONNECT/DISCONNECT command
        if !is_connect_command && !conn_guard.is_connected() {
            drop(conn_guard);
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        let conn_name = if conn_guard.is_connected() {
            conn_guard.get_info().name.clone()
        } else {
            String::new()
        };
        let auto_commit = conn_guard.auto_commit();
        let shared_connection = self.connection.clone();
        let query_timeout = Self::parse_timeout(&self.timeout_input.value());

        let db_conn_opt = conn_guard.get_connection();
        let session = conn_guard.session_state();

        // For normal commands (not CONNECT/DISCONNECT), we need a connection
        if !is_connect_command && db_conn_opt.is_none() {
            drop(conn_guard);
            fltk::dialog::alert_default("Not connected to database");
            return;
        }

        drop(conn_guard); // Release the lock before spawning thread

        let sql_text = sql.to_string();
        let sender = self.progress_sender.clone();
        let conn_opt = db_conn_opt; // Option<Arc<Connection>>
        let query_running = self.query_running.clone();

        *query_running.borrow_mut() = true;

        set_cursor(Cursor::Wait);
        app::flush();

        thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                struct ScriptFrame {
                    items: Vec<ScriptItem>,
                    index: usize,
                    base_dir: PathBuf,
                }

                let mut conn_opt = conn_opt;
                let mut conn_name = conn_name;

                let items = QueryExecutor::split_script_items(&sql_text);
                if items.is_empty() {
                    let _ = sender.send(QueryProgress::BatchFinished);
                    app::awake();
                    return;
                }

                let _ = sender.send(QueryProgress::BatchStart);
                app::awake();

                // Set timeout only if we have a connection
                let mut previous_timeout = conn_opt
                    .as_ref()
                    .and_then(|c| c.call_timeout().ok())
                    .flatten();

                if let Some(conn) = conn_opt.as_ref() {
                    if let Err(err) = conn.set_call_timeout(query_timeout) {
                        if script_mode {
                            let result = QueryResult::new_error(&sql_text, &err.to_string());
                            SqlEditorWidget::emit_script_result(
                                &sender, &conn_name, 0, result, false,
                            );
                        } else {
                            SqlEditorWidget::append_spool_output(&session, &[err.to_string()]);
                            let _ = sender.send(QueryProgress::StatementFinished {
                                index: 0,
                                result: QueryResult::new_error(&sql_text, &err.to_string()),
                                connection_name: conn_name.clone(),
                                timed_out: false,
                            });
                        }
                        let _ = sender.send(QueryProgress::BatchFinished);
                        app::awake();
                        let _ = conn.set_call_timeout(previous_timeout);
                        return;
                    }
                }

                let mut result_index = 0usize;
                let mut auto_commit = auto_commit;
                let mut continue_on_error = match session.lock() {
                    Ok(guard) => guard.continue_on_error,
                    Err(poisoned) => {
                        eprintln!("Warning: session state lock was poisoned; recovering.");
                        poisoned.into_inner().continue_on_error
                    }
                };
                let mut stop_execution = false;
                let working_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let mut frames = vec![ScriptFrame {
                    items,
                    index: 0,
                    base_dir: working_dir.clone(),
                }];

                while let Some(frame) = frames.last_mut() {
                    if stop_execution {
                        break;
                    }

                    if frame.index >= frame.items.len() {
                        frames.pop();
                        continue;
                    }

                    let item = frame.items[frame.index].clone();
                    frame.index += 1;

                    let echo_enabled = match session.lock() {
                        Ok(guard) => guard.echo_enabled,
                        Err(poisoned) => {
                            eprintln!("Warning: session state lock was poisoned; recovering.");
                            poisoned.into_inner().echo_enabled
                        }
                    };
                    if echo_enabled {
                        let echo_line = match &item {
                            ScriptItem::Statement(statement) => statement.trim().to_string(),
                            ScriptItem::ToolCommand(command) => {
                                SqlEditorWidget::format_tool_command(command)
                            }
                        };
                        if !echo_line.trim().is_empty() {
                            SqlEditorWidget::emit_script_output(&sender, &session, vec![echo_line]);
                        }
                    }

                    match item {
                        ScriptItem::ToolCommand(command) => {
                            let mut command_error = false;
                            match command {
                                ToolCommand::Var { name, data_type } => {
                                    let normalized = SessionState::normalize_name(&name);
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.binds.insert(
                                            normalized.clone(),
                                            BindVar::new(data_type.clone()),
                                        );
                                    }
                                    let message = format!(
                                        "Variable :{} declared as {}",
                                        normalized,
                                        data_type.display()
                                    );
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        &format!("VAR {} {}", normalized, data_type.display()),
                                        &message,
                                    );
                                }
                                ToolCommand::Print { name } => {
                                    let binds_snapshot = match session.lock() {
                                        Ok(guard) => guard.binds.clone(),
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            poisoned.into_inner().binds.clone()
                                        }
                                    };
                                    let (heading_enabled, _feedback_enabled) =
                                        SqlEditorWidget::current_output_settings(&session);

                                    if let Some(name) = name {
                                        let key = SessionState::normalize_name(&name);
                                        if let Some(bind) = binds_snapshot.get(&key) {
                                            match &bind.value {
                                                BindValue::Scalar(value) => {
                                                    let columns = vec![
                                                        "NAME".to_string(),
                                                        "VALUE".to_string(),
                                                    ];
                                                    let rows = vec![vec![
                                                        key.clone(),
                                                        value
                                                            .clone()
                                                            .unwrap_or_else(|| "NULL".to_string()),
                                                    ]];
                                                    SqlEditorWidget::emit_script_table(
                                                        &sender,
                                                        &session,
                                                        &format!("PRINT {}", key),
                                                        columns,
                                                        rows,
                                                        heading_enabled,
                                                    );
                                                }
                                                BindValue::Cursor(Some(cursor)) => {
                                                    let columns = cursor.columns.clone();
                                                    SqlEditorWidget::emit_script_table(
                                                        &sender,
                                                        &session,
                                                        &format!("PRINT {}", key),
                                                        columns,
                                                        cursor.rows.clone(),
                                                        heading_enabled,
                                                    );
                                                }
                                                BindValue::Cursor(None) => {
                                                    SqlEditorWidget::emit_script_message(
                                                        &sender,
                                                        &session,
                                                        &format!("PRINT {}", key),
                                                        &format!(
                                                        "Error: Cursor :{} has no data to print.",
                                                        key
                                                    ),
                                                    );
                                                    command_error = true;
                                                }
                                            }
                                        } else {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                &format!("PRINT {}", key),
                                                &format!(
                                                    "Error: Bind variable :{} is not defined.",
                                                    key
                                                ),
                                            );
                                            command_error = true;
                                        }
                                    } else if binds_snapshot.is_empty() {
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "PRINT",
                                            "No bind variables declared.",
                                        );
                                    } else {
                                        let mut summary_rows: Vec<Vec<String>> = Vec::new();
                                        let mut cursor_results: Vec<(String, CursorResult)> =
                                            Vec::new();

                                        for (name, bind) in binds_snapshot {
                                            let value_display = match &bind.value {
                                                BindValue::Scalar(value) => value
                                                    .clone()
                                                    .unwrap_or_else(|| "NULL".to_string()),
                                                BindValue::Cursor(Some(cursor)) => {
                                                    cursor_results
                                                        .push((name.clone(), cursor.clone()));
                                                    format!(
                                                        "REFCURSOR ({} rows)",
                                                        cursor.rows.len()
                                                    )
                                                }
                                                BindValue::Cursor(None) => {
                                                    "REFCURSOR (empty)".to_string()
                                                }
                                            };

                                            summary_rows.push(vec![
                                                name.clone(),
                                                bind.data_type.display(),
                                                value_display,
                                            ]);
                                        }

                                        SqlEditorWidget::emit_script_table(
                                            &sender,
                                            &session,
                                            "PRINT",
                                            vec![
                                                "NAME".to_string(),
                                                "TYPE".to_string(),
                                                "VALUE".to_string(),
                                            ],
                                            summary_rows,
                                            heading_enabled,
                                        );

                                        for (cursor_name, cursor) in cursor_results {
                                            let columns = cursor.columns.clone();
                                            SqlEditorWidget::emit_script_table(
                                                &sender,
                                                &session,
                                                &format!("PRINT {}", cursor_name),
                                                columns,
                                                cursor.rows.clone(),
                                                heading_enabled,
                                            );
                                        }
                                    }
                                }
                                ToolCommand::SetServerOutput {
                                    enabled,
                                    size,
                                    unlimited,
                                } => {
                                    // This command needs a connection
                                    let conn = match conn_opt.as_ref() {
                                        Some(c) => c,
                                        None => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "SET SERVEROUTPUT",
                                                "Error: Not connected to database",
                                            );
                                            continue;
                                        }
                                    };

                                    let default_size = 1_000_000u32;
                                    let current_size = match session.lock() {
                                        Ok(guard) => guard.server_output.size,
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            poisoned.into_inner().server_output.size
                                        }
                                    };
                                    let mut message = String::new();
                                    let mut success = true;

                                    if enabled {
                                        if unlimited {
                                            // SIZE UNLIMITED: pass None to enable unlimited buffer
                                            let enable_result = QueryExecutor::enable_dbms_output(
                                                conn.as_ref(),
                                                None,
                                            );

                                            match enable_result {
                                                Ok(()) => {
                                                    let mut guard = match session.lock() {
                                                        Ok(guard) => guard,
                                                        Err(poisoned) => {
                                                            eprintln!("Warning: session state lock was poisoned; recovering.");
                                                            poisoned.into_inner()
                                                        }
                                                    };
                                                    guard.server_output.enabled = true;
                                                    guard.server_output.size = 0; // 0 indicates unlimited
                                                    message =
                                                        "SERVEROUTPUT enabled (size UNLIMITED)"
                                                            .to_string();
                                                }
                                                Err(err) => {
                                                    success = false;
                                                    message = format!(
                                                        "SERVEROUTPUT enable failed: {}",
                                                        err
                                                    );
                                                }
                                            }
                                        } else {
                                            let desired_size = size.unwrap_or(current_size);
                                            let mut applied_size = desired_size;
                                            let mut enable_result =
                                                QueryExecutor::enable_dbms_output(
                                                    conn.as_ref(),
                                                    Some(desired_size),
                                                );

                                            if enable_result.is_err()
                                                && size.is_some()
                                                && desired_size != default_size
                                            {
                                                if QueryExecutor::enable_dbms_output(
                                                    conn.as_ref(),
                                                    Some(default_size),
                                                )
                                                .is_ok()
                                                {
                                                    applied_size = default_size;
                                                    message = format!(
                                                        "SERVEROUTPUT enabled with size {} (requested {} not supported)",
                                                        applied_size, desired_size
                                                    );
                                                    enable_result = Ok(());
                                                }
                                            }

                                            match enable_result {
                                                Ok(()) => {
                                                    let mut guard = match session.lock() {
                                                        Ok(guard) => guard,
                                                        Err(poisoned) => {
                                                            eprintln!("Warning: session state lock was poisoned; recovering.");
                                                            poisoned.into_inner()
                                                        }
                                                    };
                                                    guard.server_output.enabled = true;
                                                    guard.server_output.size = applied_size;
                                                    if message.is_empty() {
                                                        message = format!(
                                                            "SERVEROUTPUT enabled (size {})",
                                                            applied_size
                                                        );
                                                    }
                                                }
                                                Err(err) => {
                                                    success = false;
                                                    message = format!(
                                                        "SERVEROUTPUT enable failed: {}",
                                                        err
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        match QueryExecutor::disable_dbms_output(conn.as_ref()) {
                                            Ok(()) => {
                                                let mut guard = match session.lock() {
                                                    Ok(guard) => guard,
                                                    Err(poisoned) => {
                                                        eprintln!("Warning: session state lock was poisoned; recovering.");
                                                        poisoned.into_inner()
                                                    }
                                                };
                                                guard.server_output.enabled = false;
                                                message = "SERVEROUTPUT disabled".to_string();
                                            }
                                            Err(err) => {
                                                success = false;
                                                message =
                                                    format!("SERVEROUTPUT disable failed: {}", err);
                                            }
                                        }
                                    }

                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET SERVEROUTPUT",
                                        &message,
                                    );
                                    if !success {
                                        command_error = true;
                                    }
                                }
                                ToolCommand::ShowErrors {
                                    object_type,
                                    object_name,
                                } => {
                                    // This command needs a connection
                                    let conn = match conn_opt.as_ref() {
                                        Some(c) => c,
                                        None => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "SHOW ERRORS",
                                                "Error: Not connected to database",
                                            );
                                            continue;
                                        }
                                    };

                                    let mut target = None;
                                    if object_type.is_none() {
                                        target = match session.lock() {
                                            Ok(guard) => guard.last_compiled.clone(),
                                            Err(poisoned) => {
                                                eprintln!("Warning: session state lock was poisoned; recovering.");
                                                poisoned.into_inner().last_compiled.clone()
                                            }
                                        };
                                    } else if let (Some(obj_type), Some(obj_name)) =
                                        (object_type.clone(), object_name.clone())
                                    {
                                        let (owner, name) = if let Some(dot) = obj_name.find('.') {
                                            let (owner_raw, name_raw) = obj_name.split_at(dot);
                                            (
                                                Some(SqlEditorWidget::normalize_object_name(
                                                    owner_raw,
                                                )),
                                                SqlEditorWidget::normalize_object_name(
                                                    name_raw.trim_start_matches('.'),
                                                ),
                                            )
                                        } else {
                                            (
                                                None,
                                                SqlEditorWidget::normalize_object_name(&obj_name),
                                            )
                                        };

                                        target = Some(crate::db::CompiledObject {
                                            owner,
                                            object_type: obj_type.to_uppercase(),
                                            name,
                                        });
                                    }

                                    if let Some(object) = target {
                                        match QueryExecutor::fetch_compilation_errors(
                                            conn.as_ref(),
                                            &object,
                                        ) {
                                            Ok(rows) => {
                                                if rows.is_empty() {
                                                    SqlEditorWidget::emit_script_message(
                                                        &sender,
                                                        &session,
                                                        "SHOW ERRORS",
                                                        "No errors found.",
                                                    );
                                                } else {
                                                    let (heading_enabled, _feedback_enabled) =
                                                        SqlEditorWidget::current_output_settings(
                                                            &session,
                                                        );
                                                    SqlEditorWidget::emit_script_table(
                                                        &sender,
                                                        &session,
                                                        "SHOW ERRORS",
                                                        vec![
                                                            "LINE".to_string(),
                                                            "POSITION".to_string(),
                                                            "TEXT".to_string(),
                                                        ],
                                                        rows,
                                                        heading_enabled,
                                                    );
                                                }
                                            }
                                            Err(err) => {
                                                SqlEditorWidget::emit_script_message(
                                                    &sender,
                                                    &session,
                                                    "SHOW ERRORS",
                                                    &format!("Error: {}", err),
                                                );
                                                command_error = true;
                                            }
                                        }
                                    } else {
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "SHOW ERRORS",
                                            "Error: No compiled object found to show errors.",
                                        );
                                        command_error = true;
                                    }
                                }
                                ToolCommand::ShowUser => {
                                    // This command needs a connection
                                    let conn = match conn_opt.as_ref() {
                                        Some(c) => c,
                                        None => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "SHOW USER",
                                                "Error: Not connected to database",
                                            );
                                            continue;
                                        }
                                    };

                                    let sql = "SELECT USER FROM DUAL";
                                    let user_result: Result<String, OracleError> = (|| {
                                        let mut stmt = conn.statement(sql).build()?;
                                        let row = stmt.query_row(&[])?;
                                        let user: String = row.get(0)?;
                                        Ok(user)
                                    })(
                                    );
                                    match user_result {
                                        Ok(user) => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "SHOW USER",
                                                &format!("USER: {}", user),
                                            );
                                        }
                                        Err(err) => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "SHOW USER",
                                                &format!("Error: {}", err),
                                            );
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::ShowAll => {
                                    let (
                                        server_output,
                                        define_enabled,
                                        define_char,
                                        scan_enabled,
                                        verify_enabled,
                                        echo_enabled,
                                        timing_enabled,
                                        feedback_enabled,
                                        heading_enabled,
                                        pagesize,
                                        linesize,
                                        continue_on_error,
                                        spool_path,
                                    ) = match session.lock() {
                                        Ok(guard) => (
                                            guard.server_output.clone(),
                                            guard.define_enabled,
                                            guard.define_char,
                                            guard.scan_enabled,
                                            guard.verify_enabled,
                                            guard.echo_enabled,
                                            guard.timing_enabled,
                                            guard.feedback_enabled,
                                            guard.heading_enabled,
                                            guard.pagesize,
                                            guard.linesize,
                                            guard.continue_on_error,
                                            guard.spool_path.clone(),
                                        ),
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            let guard = poisoned.into_inner();
                                            (
                                                guard.server_output.clone(),
                                                guard.define_enabled,
                                                guard.define_char,
                                                guard.scan_enabled,
                                                guard.verify_enabled,
                                                guard.echo_enabled,
                                                guard.timing_enabled,
                                                guard.feedback_enabled,
                                                guard.heading_enabled,
                                                guard.pagesize,
                                                guard.linesize,
                                                guard.continue_on_error,
                                                guard.spool_path.clone(),
                                            )
                                        }
                                    };

                                    let autocommit_enabled = {
                                        let conn_guard = lock_connection(&shared_connection);
                                        conn_guard.auto_commit()
                                    };

                                    let serveroutput_line = if server_output.enabled {
                                        if server_output.size == 0 {
                                            "SERVEROUTPUT ON SIZE UNLIMITED".to_string()
                                        } else {
                                            format!("SERVEROUTPUT ON SIZE {}", server_output.size)
                                        }
                                    } else {
                                        "SERVEROUTPUT OFF".to_string()
                                    };

                                    let spool_line = match spool_path {
                                        Some(path) => format!("SPOOL {}", path.display()),
                                        None => "SPOOL OFF".to_string(),
                                    };

                                    let lines = vec![
                                        format!(
                                            "AUTOCOMMIT {}",
                                            if autocommit_enabled { "ON" } else { "OFF" }
                                        ),
                                        serveroutput_line,
                                        if define_enabled {
                                            format!("DEFINE '{}'", define_char)
                                        } else {
                                            "DEFINE OFF".to_string()
                                        },
                                        format!("SCAN {}", if scan_enabled { "ON" } else { "OFF" }),
                                        format!(
                                            "VERIFY {}",
                                            if verify_enabled { "ON" } else { "OFF" }
                                        ),
                                        format!("ECHO {}", if echo_enabled { "ON" } else { "OFF" }),
                                        format!(
                                            "TIMING {}",
                                            if timing_enabled { "ON" } else { "OFF" }
                                        ),
                                        format!(
                                            "FEEDBACK {}",
                                            if feedback_enabled { "ON" } else { "OFF" }
                                        ),
                                        format!(
                                            "HEADING {}",
                                            if heading_enabled { "ON" } else { "OFF" }
                                        ),
                                        format!("PAGESIZE {}", pagesize),
                                        format!("LINESIZE {}", linesize),
                                        format!(
                                            "ERRORCONTINUE {}",
                                            if continue_on_error { "ON" } else { "OFF" }
                                        ),
                                        spool_line,
                                    ];

                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SHOW ALL",
                                        &lines.join("\n"),
                                    );
                                }
                                ToolCommand::Describe { name } => {
                                    let conn = match conn_opt.as_ref() {
                                        Some(c) => c,
                                        None => {
                                            if script_mode {
                                                SqlEditorWidget::emit_script_message(
                                                    &sender,
                                                    &session,
                                                    "DESCRIBE",
                                                    "Error: Not connected to database",
                                                );
                                            } else {
                                                let emitted =
                                                    SqlEditorWidget::emit_non_select_result(
                                                        &sender,
                                                        &session,
                                                        &conn_name,
                                                        result_index,
                                                        &format!("DESCRIBE {}", name),
                                                        "Error: Not connected to database"
                                                            .to_string(),
                                                        false,
                                                        false,
                                                        false,
                                                    );
                                                if emitted {
                                                    result_index += 1;
                                                }
                                            }
                                            continue;
                                        }
                                    };
                                    let title = format!("DESCRIBE {}", name);
                                    match QueryExecutor::describe_object(conn.as_ref(), &name) {
                                        Ok(columns) => {
                                            if columns.is_empty() {
                                                if script_mode {
                                                    SqlEditorWidget::emit_script_message(
                                                        &sender,
                                                        &session,
                                                        &title,
                                                        "Error: Object not found.",
                                                    );
                                                } else {
                                                    let emitted =
                                                        SqlEditorWidget::emit_non_select_result(
                                                            &sender,
                                                            &session,
                                                            &conn_name,
                                                            result_index,
                                                            &title,
                                                            "Error: Object not found.".to_string(),
                                                            false,
                                                            false,
                                                            false,
                                                        );
                                                    if emitted {
                                                        result_index += 1;
                                                    }
                                                }
                                                command_error = true;
                                            } else {
                                                let rows = columns
                                                    .into_iter()
                                                    .map(|col| {
                                                        let type_display = col.get_type_display();
                                                        let TableColumnDetail {
                                                            name,
                                                            nullable,
                                                            is_primary_key,
                                                            ..
                                                        } = col;
                                                        vec![
                                                            name,
                                                            type_display,
                                                            if nullable {
                                                                "YES".to_string()
                                                            } else {
                                                                "NO".to_string()
                                                            },
                                                            if is_primary_key {
                                                                "PK".to_string()
                                                            } else {
                                                                String::new()
                                                            },
                                                        ]
                                                    })
                                                    .collect::<Vec<Vec<String>>>();
                                                if script_mode {
                                                    let (heading_enabled, _feedback_enabled) =
                                                        SqlEditorWidget::current_output_settings(
                                                            &session,
                                                        );
                                                    SqlEditorWidget::emit_script_table(
                                                        &sender,
                                                        &session,
                                                        &title,
                                                        vec![
                                                            "COLUMN".to_string(),
                                                            "TYPE".to_string(),
                                                            "NULLABLE".to_string(),
                                                            "PK".to_string(),
                                                        ],
                                                        rows,
                                                        heading_enabled,
                                                    );
                                                } else {
                                                    let (heading_enabled, feedback_enabled) =
                                                        SqlEditorWidget::current_output_settings(
                                                            &session,
                                                        );
                                                    let headers =
                                                        SqlEditorWidget::apply_heading_setting(
                                                            vec![
                                                                "COLUMN".to_string(),
                                                                "TYPE".to_string(),
                                                                "NULLABLE".to_string(),
                                                                "PK".to_string(),
                                                            ],
                                                            heading_enabled,
                                                        );
                                                    SqlEditorWidget::emit_select_result(
                                                        &sender,
                                                        &session,
                                                        &conn_name,
                                                        result_index,
                                                        &title,
                                                        headers,
                                                        rows,
                                                        true,
                                                        feedback_enabled,
                                                    );
                                                    result_index += 1;
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            if script_mode {
                                                SqlEditorWidget::emit_script_message(
                                                    &sender,
                                                    &session,
                                                    &title,
                                                    &format!("Error: {}", err),
                                                );
                                            } else {
                                                let emitted =
                                                    SqlEditorWidget::emit_non_select_result(
                                                        &sender,
                                                        &session,
                                                        &conn_name,
                                                        result_index,
                                                        &title,
                                                        format!("Error: {}", err),
                                                        false,
                                                        false,
                                                        false,
                                                    );
                                                if emitted {
                                                    result_index += 1;
                                                }
                                            }
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::Prompt { text } => {
                                    let mut output_text = text;
                                    let (define_enabled, scan_enabled) = match session.lock() {
                                        Ok(guard) => (guard.define_enabled, guard.scan_enabled),
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            let guard = poisoned.into_inner();
                                            (guard.define_enabled, guard.scan_enabled)
                                        }
                                    };
                                    if define_enabled && scan_enabled && !output_text.is_empty() {
                                        match SqlEditorWidget::apply_define_substitution(
                                            &output_text,
                                            &session,
                                            &sender,
                                        ) {
                                            Ok(updated) => {
                                                output_text = updated;
                                            }
                                            Err(message) => {
                                                SqlEditorWidget::emit_script_message(
                                                    &sender,
                                                    &session,
                                                    "PROMPT",
                                                    &format!("Error: {}", message),
                                                );
                                                command_error = true;
                                            }
                                        }
                                    }
                                    if !command_error {
                                        SqlEditorWidget::emit_script_output(
                                            &sender,
                                            &session,
                                            vec![output_text],
                                        );
                                    }
                                }
                                ToolCommand::Pause { message } => {
                                    let prompt_text = message
                                        .filter(|text| !text.trim().is_empty())
                                        .unwrap_or_else(|| "Press ENTER to continue.".to_string());
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "PAUSE",
                                        &prompt_text,
                                    );
                                    match SqlEditorWidget::prompt_for_input_with_sender(
                                        &sender,
                                        &prompt_text,
                                    ) {
                                        Ok(_) => {}
                                        Err(_) => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "PAUSE",
                                                "Error: PAUSE cancelled.",
                                            );
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::Accept { name, prompt } => {
                                    let prompt_text = prompt
                                        .unwrap_or_else(|| format!("Enter value for {}:", name));
                                    match SqlEditorWidget::prompt_for_input_with_sender(
                                        &sender,
                                        &prompt_text,
                                    ) {
                                        Ok(value) => {
                                            let key = SessionState::normalize_name(&name);
                                            match session.lock() {
                                                Ok(mut guard) => {
                                                    guard
                                                        .define_vars
                                                        .insert(key.clone(), value.clone());
                                                }
                                                Err(poisoned) => {
                                                    eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                    let mut guard = poisoned.into_inner();
                                                    guard
                                                        .define_vars
                                                        .insert(key.clone(), value.clone());
                                                }
                                            }
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                &format!("ACCEPT {}", key),
                                                &format!("Value assigned to {}", key),
                                            );
                                        }
                                        Err(message) => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                &format!("ACCEPT {}", name),
                                                &format!("Error: {}", message),
                                            );
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::Define { name, value } => {
                                    let (define_enabled, scan_enabled) = match session.lock() {
                                        Ok(guard) => (guard.define_enabled, guard.scan_enabled),
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            let guard = poisoned.into_inner();
                                            (guard.define_enabled, guard.scan_enabled)
                                        }
                                    };
                                    let mut resolved_value = value;
                                    if define_enabled && scan_enabled {
                                        match SqlEditorWidget::apply_define_substitution(
                                            &resolved_value,
                                            &session,
                                            &sender,
                                        ) {
                                            Ok(updated) => {
                                                resolved_value = updated;
                                            }
                                            Err(message) => {
                                                SqlEditorWidget::emit_script_message(
                                                    &sender,
                                                    &session,
                                                    &format!("DEFINE {}", name),
                                                    &format!("Error: {}", message),
                                                );
                                                command_error = true;
                                            }
                                        }
                                    }
                                    let key = SessionState::normalize_name(&name);
                                    if !command_error {
                                        match session.lock() {
                                            Ok(mut guard) => {
                                                guard
                                                    .define_vars
                                                    .insert(key.clone(), resolved_value.clone());
                                            }
                                            Err(poisoned) => {
                                                eprintln!(
                                                "Warning: session state lock was poisoned; recovering."
                                            );
                                                let mut guard = poisoned.into_inner();
                                                guard
                                                    .define_vars
                                                    .insert(key.clone(), resolved_value.clone());
                                            }
                                        }
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            &format!("DEFINE {}", key),
                                            &format!("Defined {} = {}", key, resolved_value),
                                        );
                                    }
                                }
                                ToolCommand::Undefine { name } => {
                                    let key = SessionState::normalize_name(&name);
                                    match session.lock() {
                                        Ok(mut guard) => {
                                            guard.define_vars.remove(&key);
                                        }
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            let mut guard = poisoned.into_inner();
                                            guard.define_vars.remove(&key);
                                        }
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        &format!("UNDEFINE {}", key),
                                        &format!("Undefined {}", key),
                                    );
                                }
                                ToolCommand::SetErrorContinue { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.continue_on_error = enabled;
                                    }
                                    continue_on_error = enabled;

                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET ERRORCONTINUE",
                                        &format!(
                                            "ERRORCONTINUE {}",
                                            if enabled { "ON" } else { "OFF" }
                                        ),
                                    );
                                }
                                ToolCommand::SetAutoCommit { enabled } => {
                                    {
                                        let mut conn_guard = lock_connection(&shared_connection);
                                        conn_guard.set_auto_commit(enabled);
                                    }
                                    auto_commit = enabled;
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET AUTOCOMMIT",
                                        if enabled {
                                            "Auto-commit enabled"
                                        } else {
                                            "Auto-commit disabled"
                                        },
                                    );
                                    let _ =
                                        sender.send(QueryProgress::AutoCommitChanged { enabled });
                                    app::awake();
                                }
                                ToolCommand::SetDefine {
                                    enabled,
                                    define_char,
                                } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.define_enabled = enabled;
                                        if let Some(ch) = define_char {
                                            guard.define_char = ch;
                                        }
                                    }
                                    let msg = if let Some(ch) = define_char {
                                        format!("DEFINE '{}'", ch)
                                    } else {
                                        format!("DEFINE {}", if enabled { "ON" } else { "OFF" })
                                    };
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET DEFINE",
                                        &msg,
                                    );
                                }
                                ToolCommand::SetScan { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.scan_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET SCAN",
                                        &format!("SCAN {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetVerify { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.verify_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET VERIFY",
                                        &format!("VERIFY {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetEcho { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.echo_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET ECHO",
                                        &format!("ECHO {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetTiming { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.timing_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET TIMING",
                                        &format!("TIMING {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetFeedback { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.feedback_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET FEEDBACK",
                                        &format!("FEEDBACK {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetHeading { enabled } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.heading_enabled = enabled;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET HEADING",
                                        &format!("HEADING {}", if enabled { "ON" } else { "OFF" }),
                                    );
                                }
                                ToolCommand::SetPageSize { size } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.pagesize = size;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET PAGESIZE",
                                        &format!("PAGESIZE {}", size),
                                    );
                                }
                                ToolCommand::SetLineSize { size } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.linesize = size;
                                    }
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "SET LINESIZE",
                                        &format!("LINESIZE {}", size),
                                    );
                                }
                                ToolCommand::Spool { path } => match path {
                                    Some(path) => {
                                        let target_path = if Path::new(&path).is_absolute() {
                                            PathBuf::from(&path)
                                        } else {
                                            frame.base_dir.join(&path)
                                        };
                                        match session.lock() {
                                            Ok(mut guard) => {
                                                guard.spool_path = Some(target_path.clone());
                                                guard.spool_truncate = true;
                                            }
                                            Err(poisoned) => {
                                                eprintln!(
                                                "Warning: session state lock was poisoned; recovering."
                                            );
                                                let mut guard = poisoned.into_inner();
                                                guard.spool_path = Some(target_path.clone());
                                                guard.spool_truncate = true;
                                            }
                                        }
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "SPOOL",
                                            &format!(
                                                "Spooling output to {}",
                                                target_path.display()
                                            ),
                                        );
                                    }
                                    None => {
                                        match session.lock() {
                                            Ok(mut guard) => {
                                                guard.spool_path = None;
                                                guard.spool_truncate = false;
                                            }
                                            Err(poisoned) => {
                                                eprintln!(
                                                "Warning: session state lock was poisoned; recovering."
                                            );
                                                let mut guard = poisoned.into_inner();
                                                guard.spool_path = None;
                                                guard.spool_truncate = false;
                                            }
                                        }
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "SPOOL",
                                            "Spooling disabled",
                                        );
                                    }
                                },
                                ToolCommand::WheneverSqlError { exit } => {
                                    {
                                        let mut guard = match session.lock() {
                                            Ok(guard) => guard,
                                            Err(poisoned) => {
                                                eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                poisoned.into_inner()
                                            }
                                        };
                                        guard.continue_on_error = !exit;
                                    }
                                    continue_on_error = !exit;
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "WHENEVER SQLERROR",
                                        if exit { "Mode EXIT" } else { "Mode CONTINUE" },
                                    );
                                }
                                ToolCommand::Exit => {
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "EXIT",
                                        "Execution stopped.",
                                    );
                                    stop_execution = true;
                                }
                                ToolCommand::Quit => {
                                    SqlEditorWidget::emit_script_message(
                                        &sender,
                                        &session,
                                        "QUIT",
                                        "Execution stopped.",
                                    );
                                    stop_execution = true;
                                }
                                ToolCommand::Connect {
                                    username,
                                    password,
                                    host,
                                    port,
                                    service_name,
                                } => {
                                    let conn_info = ConnectionInfo {
                                        name: format!("{}@{}", username, host),
                                        username,
                                        password,
                                        host,
                                        port,
                                        service_name,
                                    };

                                    let mut shared_conn_guard = lock_connection(&shared_connection);
                                    match shared_conn_guard.connect(conn_info.clone()) {
                                        Ok(_) => {
                                            conn_opt = shared_conn_guard.get_connection();
                                            if shared_conn_guard.is_connected() {
                                                conn_name =
                                                    shared_conn_guard.get_info().name.clone();
                                            } else {
                                                conn_name.clear();
                                            }
                                            drop(shared_conn_guard);
                                            match session.lock() {
                                                Ok(mut guard) => guard.reset(),
                                                Err(poisoned) => {
                                                    eprintln!(
                                                    "Warning: session state lock was poisoned; recovering."
                                                );
                                                    poisoned.into_inner().reset();
                                                }
                                            }
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                "CONNECT",
                                                &format!(
                                                    "Connected to {}",
                                                    conn_info.display_string()
                                                ),
                                            );
                                            previous_timeout = conn_opt
                                                .as_ref()
                                                .and_then(|c| c.call_timeout().ok())
                                                .flatten();
                                            if let Some(conn) = conn_opt.as_ref() {
                                                let _ = conn.set_call_timeout(query_timeout);
                                            }
                                            let _ = sender.send(QueryProgress::ConnectionChanged {
                                                info: Some(conn_info.clone()),
                                            });
                                            app::awake();
                                        }
                                        Err(err) => {
                                            let error_msg = format!("Connection failed: {}", err);
                                            SqlEditorWidget::emit_script_message(
                                                &sender, &session, "CONNECT", &error_msg,
                                            );
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::Disconnect => {
                                    let mut shared_conn_guard = lock_connection(&shared_connection);
                                    if shared_conn_guard.is_connected() {
                                        shared_conn_guard.disconnect();
                                        conn_opt = shared_conn_guard.get_connection();
                                        if shared_conn_guard.is_connected() {
                                            conn_name = shared_conn_guard.get_info().name.clone();
                                        } else {
                                            conn_name.clear();
                                        }
                                        drop(shared_conn_guard);
                                        match session.lock() {
                                            Ok(mut guard) => guard.reset(),
                                            Err(poisoned) => {
                                                eprintln!(
                                                "Warning: session state lock was poisoned; recovering."
                                            );
                                                poisoned.into_inner().reset();
                                            }
                                        }
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "DISCONNECT",
                                            "Disconnected from database",
                                        );
                                        previous_timeout = conn_opt
                                            .as_ref()
                                            .and_then(|c| c.call_timeout().ok())
                                            .flatten();
                                        let _ = sender
                                            .send(QueryProgress::ConnectionChanged { info: None });
                                        app::awake();
                                    } else {
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            "DISCONNECT",
                                            "Not connected to any database",
                                        );
                                    }
                                }
                                ToolCommand::RunScript {
                                    path,
                                    relative_to_caller,
                                } => {
                                    let base_dir = if relative_to_caller {
                                        frame.base_dir.clone()
                                    } else {
                                        working_dir.clone()
                                    };
                                    let target_path = if Path::new(&path).is_absolute() {
                                        PathBuf::from(&path)
                                    } else {
                                        base_dir.join(&path)
                                    };
                                    match fs::read_to_string(&target_path) {
                                        Ok(contents) => {
                                            let script_items =
                                                QueryExecutor::split_script_items(&contents);
                                            let script_dir = target_path
                                                .parent()
                                                .unwrap_or(&base_dir)
                                                .to_path_buf();
                                            frames.push(ScriptFrame {
                                                items: script_items,
                                                index: 0,
                                                base_dir: script_dir,
                                            });
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                if relative_to_caller { "@@" } else { "@" },
                                                &format!(
                                                    "Running script {}",
                                                    target_path.display()
                                                ),
                                            );
                                        }
                                        Err(err) => {
                                            SqlEditorWidget::emit_script_message(
                                                &sender,
                                                &session,
                                                if relative_to_caller { "@@" } else { "@" },
                                                &format!(
                                                    "Error: Failed to read script {}: {}",
                                                    target_path.display(),
                                                    err
                                                ),
                                            );
                                            command_error = true;
                                        }
                                    }
                                }
                                ToolCommand::Unsupported {
                                    raw,
                                    message,
                                    is_error,
                                } => {
                                    if is_error {
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            &raw,
                                            &format!("Error: {}", message),
                                        );
                                        command_error = true;
                                    } else {
                                        SqlEditorWidget::emit_script_message(
                                            &sender,
                                            &session,
                                            &raw,
                                            &format!("Warning: {}", message),
                                        );
                                    }
                                }
                            }

                            if command_error && !continue_on_error {
                                stop_execution = true;
                            }
                        }
                        ScriptItem::Statement(statement) => {
                            // For statements, we need a connection
                            let conn = match conn_opt.as_ref() {
                                Some(c) => c,
                                None => {
                                    // This shouldn't happen as we checked earlier
                                    eprintln!(
                                        "Error: No connection available for statement execution"
                                    );
                                    let emitted = SqlEditorWidget::emit_non_select_result(
                                        &sender,
                                        &session,
                                        &conn_name,
                                        result_index,
                                        &statement,
                                        "Error: Not connected to database".to_string(),
                                        false,
                                        false,
                                        script_mode,
                                    );
                                    if emitted {
                                        result_index += 1;
                                    }
                                    stop_execution = true;
                                    continue;
                                }
                            };

                            let trimmed = statement.trim_start_matches(';').trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            let mut sql_text = trimmed.to_string();
                            let (define_enabled, scan_enabled, verify_enabled) =
                                match session.lock() {
                                    Ok(guard) => (
                                        guard.define_enabled,
                                        guard.scan_enabled,
                                        guard.verify_enabled,
                                    ),
                                    Err(poisoned) => {
                                        eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                        let guard = poisoned.into_inner();
                                        (
                                            guard.define_enabled,
                                            guard.scan_enabled,
                                            guard.verify_enabled,
                                        )
                                    }
                                };
                            if define_enabled && scan_enabled {
                                let sql_before = sql_text.clone();
                                match SqlEditorWidget::apply_define_substitution(
                                    &sql_text, &session, &sender,
                                ) {
                                    Ok(updated) => {
                                        if verify_enabled && updated != sql_before {
                                            SqlEditorWidget::emit_script_output(
                                                &sender,
                                                &session,
                                                vec![
                                                    format!("old: {}", sql_before),
                                                    format!("new: {}", updated),
                                                ],
                                            );
                                        }
                                        sql_text = updated;
                                    }
                                    Err(message) => {
                                        let emitted = SqlEditorWidget::emit_non_select_result(
                                            &sender,
                                            &session,
                                            &conn_name,
                                            result_index,
                                            trimmed,
                                            format!("Error: {}", message),
                                            false,
                                            false,
                                            script_mode,
                                        );
                                        if emitted {
                                            result_index += 1;
                                        }
                                        if !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                }
                            }

                            let cleaned = SqlEditorWidget::strip_leading_comments(&sql_text);
                            let upper = cleaned.to_uppercase();

                            if upper.starts_with("COMMIT") {
                                let mut timed_out = false;
                                let statement_start = Instant::now();
                                let mut result = match conn.commit() {
                                    Ok(()) => QueryResult {
                                        sql: sql_text.to_string(),
                                        columns: vec![],
                                        rows: vec![],
                                        row_count: 0,
                                        execution_time: Duration::from_secs(0),
                                        message: "Commit complete".to_string(),
                                        is_select: false,
                                        success: true,
                                    },
                                    Err(err) => {
                                        timed_out = SqlEditorWidget::is_timeout_error(&err);
                                        QueryResult::new_error(&sql_text, &err.to_string())
                                    }
                                };
                                let timing_duration = statement_start.elapsed();
                                result.execution_time = timing_duration;
                                let result_success = result.success;
                                if script_mode {
                                    if result_success {
                                        SqlEditorWidget::emit_script_lines(
                                            &sender,
                                            &session,
                                            &result.message,
                                        );
                                    }
                                    SqlEditorWidget::emit_script_result(
                                        &sender,
                                        &conn_name,
                                        result_index,
                                        result,
                                        timed_out,
                                    );
                                } else {
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();
                                    if !result.message.trim().is_empty() {
                                        SqlEditorWidget::append_spool_output(
                                            &session,
                                            &[result.message.clone()],
                                        );
                                    }
                                    let _ = sender.send(QueryProgress::StatementFinished {
                                        index,
                                        result,
                                        connection_name: conn_name.clone(),
                                        timed_out,
                                    });
                                    app::awake();
                                    result_index += 1;
                                }
                                SqlEditorWidget::emit_timing_if_enabled(
                                    &sender,
                                    &session,
                                    timing_duration,
                                );
                                if timed_out {
                                    stop_execution = true;
                                } else if !result_success && !continue_on_error {
                                    stop_execution = true;
                                }
                                continue;
                            }

                            if upper.starts_with("ROLLBACK") {
                                let mut timed_out = false;
                                let statement_start = Instant::now();
                                let mut result = match conn.rollback() {
                                    Ok(()) => QueryResult {
                                        sql: sql_text.to_string(),
                                        columns: vec![],
                                        rows: vec![],
                                        row_count: 0,
                                        execution_time: Duration::from_secs(0),
                                        message: "Rollback complete".to_string(),
                                        is_select: false,
                                        success: true,
                                    },
                                    Err(err) => {
                                        timed_out = SqlEditorWidget::is_timeout_error(&err);
                                        QueryResult::new_error(&sql_text, &err.to_string())
                                    }
                                };
                                let timing_duration = statement_start.elapsed();
                                result.execution_time = timing_duration;
                                let result_success = result.success;
                                if script_mode {
                                    if result_success {
                                        SqlEditorWidget::emit_script_lines(
                                            &sender,
                                            &session,
                                            &result.message,
                                        );
                                    }
                                    SqlEditorWidget::emit_script_result(
                                        &sender,
                                        &conn_name,
                                        result_index,
                                        result,
                                        timed_out,
                                    );
                                } else {
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();
                                    if !result.message.trim().is_empty() {
                                        SqlEditorWidget::append_spool_output(
                                            &session,
                                            &[result.message.clone()],
                                        );
                                    }
                                    let _ = sender.send(QueryProgress::StatementFinished {
                                        index,
                                        result,
                                        connection_name: conn_name.clone(),
                                        timed_out,
                                    });
                                    app::awake();
                                    result_index += 1;
                                }
                                SqlEditorWidget::emit_timing_if_enabled(
                                    &sender,
                                    &session,
                                    timing_duration,
                                );
                                if timed_out {
                                    stop_execution = true;
                                } else if !result_success && !continue_on_error {
                                    stop_execution = true;
                                }
                                continue;
                            }

                            let compiled_object = QueryExecutor::parse_compiled_object(&sql_text);
                            let is_compiled_plsql = compiled_object.is_some();
                            if let Some(object) = compiled_object.clone() {
                                let mut guard = match session.lock() {
                                    Ok(guard) => guard,
                                    Err(poisoned) => {
                                        eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                        poisoned.into_inner()
                                    }
                                };
                                guard.last_compiled = Some(object);
                            }

                            let exec_call = QueryExecutor::normalize_exec_call(&sql_text);
                            if exec_call.is_some() {
                                if let Err(message) =
                                    QueryExecutor::check_named_positional_mix(&sql_text)
                                {
                                    let emitted = SqlEditorWidget::emit_non_select_result(
                                        &sender,
                                        &session,
                                        &conn_name,
                                        result_index,
                                        &sql_text,
                                        format!("Error: {}", message),
                                        false,
                                        false,
                                        script_mode,
                                    );
                                    if emitted {
                                        result_index += 1;
                                    }
                                    if !continue_on_error {
                                        stop_execution = true;
                                    }
                                    continue;
                                }
                            }

                            let is_plsql_block =
                                upper.starts_with("BEGIN") || upper.starts_with("DECLARE");
                            let is_select = QueryExecutor::is_select_statement(&sql_text);

                            if exec_call.is_some() || is_plsql_block {
                                let mut sql_to_execute =
                                    exec_call.unwrap_or_else(|| sql_text.to_string());
                                if is_plsql_block {
                                    sql_to_execute =
                                        SqlEditorWidget::ensure_plsql_terminator(&sql_to_execute);
                                }
                                let binds = match session.lock() {
                                    Ok(guard) => {
                                        QueryExecutor::resolve_binds(&sql_to_execute, &guard)
                                    }
                                    Err(poisoned) => {
                                        eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                        QueryExecutor::resolve_binds(
                                            &sql_to_execute,
                                            &poisoned.into_inner(),
                                        )
                                    }
                                };

                                let binds = match binds {
                                    Ok(binds) => binds,
                                    Err(message) => {
                                        let emitted = SqlEditorWidget::emit_non_select_result(
                                            &sender,
                                            &session,
                                            &conn_name,
                                            result_index,
                                            &sql_text,
                                            format!("Error: {}", message),
                                            false,
                                            false,
                                            script_mode,
                                        );
                                        if emitted {
                                            result_index += 1;
                                        }
                                        if !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                };

                                let statement_start = Instant::now();
                                let mut timed_out = false;
                                let stmt = match QueryExecutor::execute_with_binds(
                                    conn.as_ref(),
                                    &sql_to_execute,
                                    &binds,
                                ) {
                                    Ok(stmt) => stmt,
                                    Err(err) => {
                                        let cancelled = SqlEditorWidget::is_cancel_error(&err);
                                        timed_out = SqlEditorWidget::is_timeout_error(&err);
                                        let message = if cancelled {
                                            SqlEditorWidget::cancel_message()
                                        } else if timed_out {
                                            SqlEditorWidget::timeout_message(query_timeout)
                                        } else {
                                            err.to_string()
                                        };
                                        if script_mode {
                                            let result =
                                                QueryResult::new_error(&sql_text, &message);
                                            SqlEditorWidget::emit_script_result(
                                                &sender,
                                                &conn_name,
                                                result_index,
                                                result,
                                                timed_out,
                                            );
                                        } else {
                                            let index = result_index;
                                            let _ = sender
                                                .send(QueryProgress::StatementStart { index });
                                            app::awake();
                                            SqlEditorWidget::append_spool_output(
                                                &session,
                                                &[message.clone()],
                                            );
                                            let result =
                                                QueryResult::new_error(&sql_text, &message);
                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result,
                                                connection_name: conn_name.clone(),
                                                timed_out,
                                            });
                                            app::awake();
                                            result_index += 1;
                                        }
                                        SqlEditorWidget::emit_timing_if_enabled(
                                            &sender,
                                            &session,
                                            statement_start.elapsed(),
                                        );
                                        if timed_out || cancelled || !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                };

                                let timing_duration = statement_start.elapsed();
                                let mut result = QueryResult {
                                    sql: sql_text.to_string(),
                                    columns: vec![],
                                    rows: vec![],
                                    row_count: 0,
                                    execution_time: timing_duration,
                                    message: "PL/SQL procedure successfully completed".to_string(),
                                    is_select: false,
                                    success: true,
                                };

                                let mut out_messages: Vec<String> = Vec::new();
                                if let Ok(updates) =
                                    QueryExecutor::fetch_scalar_bind_updates(&stmt, &binds)
                                {
                                    let mut guard = match session.lock() {
                                        Ok(guard) => guard,
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            poisoned.into_inner()
                                        }
                                    };
                                    for (name, value) in updates {
                                        if let Some(bind) = guard.binds.get_mut(&name) {
                                            bind.value = value.clone();
                                        }
                                        if let BindValue::Scalar(val) = value {
                                            out_messages.push(format!(
                                                ":{} = {}",
                                                name,
                                                val.unwrap_or_else(|| "NULL".to_string())
                                            ));
                                        }
                                    }
                                }

                                if !out_messages.is_empty() {
                                    result.message = format!(
                                        "{} | OUT: {}",
                                        result.message,
                                        out_messages.join(", ")
                                    );
                                }

                                if auto_commit {
                                    if let Err(err) = conn.commit() {
                                        result = QueryResult::new_error(
                                            &sql_text,
                                            &format!("Auto-commit failed: {}", err),
                                        );
                                    } else {
                                        result.message =
                                            format!("{} | Auto-commit applied", result.message);
                                    }
                                }

                                if script_mode {
                                    if result.success {
                                        SqlEditorWidget::emit_script_lines(
                                            &sender,
                                            &session,
                                            &result.message,
                                        );
                                    }
                                    SqlEditorWidget::emit_script_result(
                                        &sender,
                                        &conn_name,
                                        result_index,
                                        result.clone(),
                                        timed_out,
                                    );
                                } else {
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();
                                    if !result.message.trim().is_empty() {
                                        SqlEditorWidget::append_spool_output(
                                            &session,
                                            &[result.message.clone()],
                                        );
                                    }
                                    let _ = sender.send(QueryProgress::StatementFinished {
                                        index,
                                        result: result.clone(),
                                        connection_name: conn_name.clone(),
                                        timed_out,
                                    });
                                    app::awake();
                                    result_index += 1;
                                }

                                let ref_cursors = QueryExecutor::extract_ref_cursors(&stmt, &binds)
                                    .unwrap_or_default();
                                let implicit_results =
                                    QueryExecutor::extract_implicit_results(&stmt)
                                        .unwrap_or_default();

                                for (cursor_name, mut cursor) in ref_cursors {
                                    if stop_execution {
                                        break;
                                    }
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();

                                    let mut buffered_rows: Vec<Vec<String>> = Vec::new();
                                    let mut cursor_rows: Vec<Vec<String>> = Vec::new();
                                    let mut last_flush = Instant::now();
                                    let cursor_start = Instant::now();
                                    let mut cursor_timed_out = false;
                                    let (heading_enabled, feedback_enabled) =
                                        SqlEditorWidget::current_output_settings(&session);

                                    let cursor_label = format!("REFCURSOR :{}", cursor_name);
                                    let cursor_result = QueryExecutor::execute_ref_cursor_streaming(
                                        &mut cursor,
                                        &cursor_label,
                                        &mut |columns| {
                                            let names = columns
                                                .iter()
                                                .map(|col| col.name.clone())
                                                .collect::<Vec<String>>();
                                            let display_columns =
                                                SqlEditorWidget::apply_heading_setting(
                                                    names,
                                                    heading_enabled,
                                                );
                                            let _ = sender.send(QueryProgress::SelectStart {
                                                index,
                                                columns: display_columns.clone(),
                                            });
                                            app::awake();
                                            if !display_columns.is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[display_columns.join(" | ")],
                                                );
                                            }
                                        },
                                        &mut |row| {
                                            if let Some(timeout_duration) = query_timeout {
                                                if cursor_start.elapsed() >= timeout_duration {
                                                    cursor_timed_out = true;
                                                    return false;
                                                }
                                            }
                                            cursor_rows.push(row.clone());
                                            buffered_rows.push(row);
                                            if last_flush.elapsed() >= Duration::from_secs(1) {
                                                let rows = std::mem::take(&mut buffered_rows);
                                                SqlEditorWidget::append_spool_rows(&session, &rows);
                                                let _ = sender
                                                    .send(QueryProgress::Rows { index, rows });
                                                app::awake();
                                                last_flush = Instant::now();
                                            }
                                            true
                                        },
                                    );

                                    match cursor_result {
                                        Ok((mut query_result, was_cancelled)) => {
                                            if !buffered_rows.is_empty() {
                                                let rows = std::mem::take(&mut buffered_rows);
                                                SqlEditorWidget::append_spool_rows(&session, &rows);
                                                let _ = sender
                                                    .send(QueryProgress::Rows { index, rows });
                                                app::awake();
                                            }

                                            if cursor_timed_out {
                                                query_result.message =
                                                    SqlEditorWidget::timeout_message(query_timeout);
                                                query_result.success = false;
                                                cursor_timed_out = true;
                                            } else if was_cancelled {
                                                query_result.message =
                                                    SqlEditorWidget::cancel_message();
                                                query_result.success = false;
                                            }
                                            SqlEditorWidget::apply_heading_to_result(
                                                &mut query_result,
                                                heading_enabled,
                                            );
                                            if !feedback_enabled {
                                                query_result.message.clear();
                                            }

                                            let column_names: Vec<String> = query_result
                                                .columns
                                                .iter()
                                                .map(|c| c.name.clone())
                                                .collect();

                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result: query_result.clone(),
                                                connection_name: conn_name.clone(),
                                                timed_out: cursor_timed_out,
                                            });
                                            app::awake();
                                            if !query_result.message.trim().is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[query_result.message.clone()],
                                                );
                                            }
                                            result_index += 1;

                                            let mut guard = match session.lock() {
                                                Ok(guard) => guard,
                                                Err(poisoned) => {
                                                    eprintln!("Warning: session state lock was poisoned; recovering.");
                                                    poisoned.into_inner()
                                                }
                                            };
                                            if let Some(bind) = guard.binds.get_mut(&cursor_name) {
                                                bind.value =
                                                    BindValue::Cursor(Some(CursorResult {
                                                        columns: column_names,
                                                        rows: cursor_rows,
                                                    }));
                                            }

                                            if cursor_timed_out {
                                                stop_execution = true;
                                                break;
                                            }
                                            if !query_result.success && !continue_on_error {
                                                stop_execution = true;
                                                break;
                                            }
                                        }
                                        Err(err) => {
                                            let cancelled = SqlEditorWidget::is_cancel_error(&err);
                                            cursor_timed_out =
                                                SqlEditorWidget::is_timeout_error(&err);
                                            let message = if cancelled {
                                                SqlEditorWidget::cancel_message()
                                            } else if cursor_timed_out {
                                                SqlEditorWidget::timeout_message(query_timeout)
                                            } else {
                                                err.to_string()
                                            };
                                            SqlEditorWidget::append_spool_output(
                                                &session,
                                                &[message.clone()],
                                            );
                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result: QueryResult::new_error(
                                                    &cursor_label,
                                                    &message,
                                                ),
                                                connection_name: conn_name.clone(),
                                                timed_out: cursor_timed_out,
                                            });
                                            app::awake();
                                            result_index += 1;

                                            if cursor_timed_out || cancelled || !continue_on_error {
                                                stop_execution = true;
                                                break;
                                            }
                                        }
                                    }
                                }

                                for (idx, mut cursor) in implicit_results.into_iter().enumerate() {
                                    if stop_execution {
                                        break;
                                    }
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();

                                    let mut buffered_rows: Vec<Vec<String>> = Vec::new();
                                    let mut last_flush = Instant::now();
                                    let cursor_start = Instant::now();
                                    let mut cursor_timed_out = false;
                                    let (heading_enabled, feedback_enabled) =
                                        SqlEditorWidget::current_output_settings(&session);
                                    let cursor_label = format!("IMPLICIT RESULT {}", idx + 1);

                                    let cursor_result = QueryExecutor::execute_ref_cursor_streaming(
                                        &mut cursor,
                                        &cursor_label,
                                        &mut |columns| {
                                            let names = columns
                                                .iter()
                                                .map(|col| col.name.clone())
                                                .collect::<Vec<String>>();
                                            let display_columns =
                                                SqlEditorWidget::apply_heading_setting(
                                                    names,
                                                    heading_enabled,
                                                );
                                            let _ = sender.send(QueryProgress::SelectStart {
                                                index,
                                                columns: display_columns.clone(),
                                            });
                                            app::awake();
                                            if !display_columns.is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[display_columns.join(" | ")],
                                                );
                                            }
                                        },
                                        &mut |row| {
                                            if let Some(timeout_duration) = query_timeout {
                                                if cursor_start.elapsed() >= timeout_duration {
                                                    cursor_timed_out = true;
                                                    return false;
                                                }
                                            }
                                            buffered_rows.push(row);
                                            if last_flush.elapsed() >= Duration::from_secs(1) {
                                                let rows = std::mem::take(&mut buffered_rows);
                                                SqlEditorWidget::append_spool_rows(&session, &rows);
                                                let _ = sender
                                                    .send(QueryProgress::Rows { index, rows });
                                                app::awake();
                                                last_flush = Instant::now();
                                            }
                                            true
                                        },
                                    );

                                    match cursor_result {
                                        Ok((mut query_result, was_cancelled)) => {
                                            if !buffered_rows.is_empty() {
                                                let rows = std::mem::take(&mut buffered_rows);
                                                SqlEditorWidget::append_spool_rows(&session, &rows);
                                                let _ = sender
                                                    .send(QueryProgress::Rows { index, rows });
                                                app::awake();
                                            }

                                            if cursor_timed_out {
                                                query_result.message =
                                                    SqlEditorWidget::timeout_message(query_timeout);
                                                query_result.success = false;
                                                cursor_timed_out = true;
                                            } else if was_cancelled {
                                                query_result.message =
                                                    SqlEditorWidget::cancel_message();
                                                query_result.success = false;
                                            }
                                            SqlEditorWidget::apply_heading_to_result(
                                                &mut query_result,
                                                heading_enabled,
                                            );
                                            if !feedback_enabled {
                                                query_result.message.clear();
                                            }

                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result: query_result.clone(),
                                                connection_name: conn_name.clone(),
                                                timed_out: cursor_timed_out,
                                            });
                                            app::awake();
                                            if !query_result.message.trim().is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[query_result.message.clone()],
                                                );
                                            }
                                            result_index += 1;

                                            if cursor_timed_out {
                                                stop_execution = true;
                                                break;
                                            }
                                            if !query_result.success && !continue_on_error {
                                                stop_execution = true;
                                                break;
                                            }
                                        }
                                        Err(err) => {
                                            let cancelled = SqlEditorWidget::is_cancel_error(&err);
                                            cursor_timed_out =
                                                SqlEditorWidget::is_timeout_error(&err);
                                            let message = if cancelled {
                                                SqlEditorWidget::cancel_message()
                                            } else if cursor_timed_out {
                                                SqlEditorWidget::timeout_message(query_timeout)
                                            } else {
                                                err.to_string()
                                            };
                                            SqlEditorWidget::append_spool_output(
                                                &session,
                                                &[message.clone()],
                                            );
                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result: QueryResult::new_error(
                                                    &cursor_label,
                                                    &message,
                                                ),
                                                connection_name: conn_name.clone(),
                                                timed_out: cursor_timed_out,
                                            });
                                            app::awake();
                                            result_index += 1;

                                            if cursor_timed_out || cancelled || !continue_on_error {
                                                stop_execution = true;
                                                break;
                                            }
                                        }
                                    }
                                }

                                let _ = SqlEditorWidget::emit_dbms_output(
                                    &sender,
                                    &conn_name,
                                    conn.as_ref(),
                                    &session,
                                    &mut result_index,
                                );
                                SqlEditorWidget::emit_timing_if_enabled(
                                    &sender,
                                    &session,
                                    timing_duration,
                                );

                                if timed_out {
                                    stop_execution = true;
                                } else if !result.success && !continue_on_error {
                                    stop_execution = true;
                                }
                            } else if is_select {
                                let sql_to_execute =
                                    sql_text.trim_end_matches(';').trim().to_string();
                                let binds = match session.lock() {
                                    Ok(guard) => {
                                        QueryExecutor::resolve_binds(&sql_to_execute, &guard)
                                    }
                                    Err(poisoned) => {
                                        eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                        QueryExecutor::resolve_binds(
                                            &sql_to_execute,
                                            &poisoned.into_inner(),
                                        )
                                    }
                                };

                                let binds = match binds {
                                    Ok(binds) => binds,
                                    Err(message) => {
                                        let emitted = SqlEditorWidget::emit_non_select_result(
                                            &sender,
                                            &session,
                                            &conn_name,
                                            result_index,
                                            &sql_text,
                                            format!("Error: {}", message),
                                            false,
                                            false,
                                            script_mode,
                                        );
                                        if emitted {
                                            result_index += 1;
                                        }
                                        if !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                };

                                let index = result_index;
                                let _ = sender.send(QueryProgress::StatementStart { index });
                                app::awake();

                                let (heading_enabled, feedback_enabled) =
                                    SqlEditorWidget::current_output_settings(&session);
                                let mut buffered_rows: Vec<Vec<String>> = Vec::new();
                                let mut last_flush = Instant::now();
                                let statement_start = Instant::now();
                                let mut timed_out = false;

                                let result =
                                    match QueryExecutor::execute_select_streaming_with_binds(
                                        conn.as_ref(),
                                        &sql_to_execute,
                                        &binds,
                                        &mut |columns| {
                                            let names = columns
                                                .iter()
                                                .map(|col| col.name.clone())
                                                .collect::<Vec<String>>();
                                            let display_columns =
                                                SqlEditorWidget::apply_heading_setting(
                                                    names,
                                                    heading_enabled,
                                                );
                                            let _ = sender.send(QueryProgress::SelectStart {
                                                index,
                                                columns: display_columns.clone(),
                                            });
                                            app::awake();
                                            if !display_columns.is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[display_columns.join(" | ")],
                                                );
                                            }
                                        },
                                        &mut |row| {
                                            if let Some(timeout_duration) = query_timeout {
                                                if statement_start.elapsed() >= timeout_duration {
                                                    timed_out = true;
                                                    return false;
                                                }
                                            }

                                            buffered_rows.push(row);
                                            if last_flush.elapsed() >= Duration::from_secs(1) {
                                                let rows = std::mem::take(&mut buffered_rows);
                                                SqlEditorWidget::append_spool_rows(&session, &rows);
                                                let _ = sender
                                                    .send(QueryProgress::Rows { index, rows });
                                                app::awake();
                                                last_flush = Instant::now();
                                            }
                                            true
                                        },
                                    ) {
                                        Ok((mut query_result, was_cancelled)) => {
                                            SqlEditorWidget::apply_heading_to_result(
                                                &mut query_result,
                                                heading_enabled,
                                            );
                                            if timed_out {
                                                query_result.message =
                                                    SqlEditorWidget::timeout_message(query_timeout);
                                                query_result.success = false;
                                                timed_out = true;
                                            } else if was_cancelled {
                                                query_result.message =
                                                    SqlEditorWidget::cancel_message();
                                                query_result.success = false;
                                            }
                                            if !feedback_enabled {
                                                query_result.message.clear();
                                            }
                                            if !query_result.message.trim().is_empty() {
                                                SqlEditorWidget::append_spool_output(
                                                    &session,
                                                    &[query_result.message.clone()],
                                                );
                                            }
                                            query_result
                                        }
                                        Err(err) => {
                                            let cancelled = SqlEditorWidget::is_cancel_error(&err);
                                            timed_out = SqlEditorWidget::is_timeout_error(&err);
                                            let message = if cancelled {
                                                SqlEditorWidget::cancel_message()
                                            } else if timed_out {
                                                SqlEditorWidget::timeout_message(query_timeout)
                                            } else {
                                                err.to_string()
                                            };
                                            let mut error_result =
                                                QueryResult::new_error(&sql_text, &message);
                                            // Preserve is_select flag so existing streamed data is kept
                                            error_result.is_select = true;
                                            error_result
                                        }
                                    };

                                if !buffered_rows.is_empty() {
                                    let rows = std::mem::take(&mut buffered_rows);
                                    SqlEditorWidget::append_spool_rows(&session, &rows);
                                    let _ = sender.send(QueryProgress::Rows { index, rows });
                                    app::awake();
                                }

                                if !result.message.trim().is_empty() {
                                    SqlEditorWidget::append_spool_output(
                                        &session,
                                        &[result.message.clone()],
                                    );
                                }
                                let timing_duration = if result.execution_time.is_zero() {
                                    statement_start.elapsed()
                                } else {
                                    result.execution_time
                                };
                                let _ = sender.send(QueryProgress::StatementFinished {
                                    index,
                                    result: result.clone(),
                                    connection_name: conn_name.clone(),
                                    timed_out,
                                });
                                app::awake();
                                result_index += 1;

                                let _ = SqlEditorWidget::emit_dbms_output(
                                    &sender,
                                    &conn_name,
                                    conn.as_ref(),
                                    &session,
                                    &mut result_index,
                                );
                                SqlEditorWidget::emit_timing_if_enabled(
                                    &sender,
                                    &session,
                                    timing_duration,
                                );

                                if timed_out {
                                    stop_execution = true;
                                } else if !result.success && !continue_on_error {
                                    stop_execution = true;
                                }
                            } else {
                                let sql_to_execute = if is_compiled_plsql {
                                    SqlEditorWidget::ensure_plsql_terminator(&sql_text)
                                } else {
                                    sql_text.trim_end_matches(';').trim().to_string()
                                };
                                let binds = match session.lock() {
                                    Ok(guard) => {
                                        QueryExecutor::resolve_binds(&sql_to_execute, &guard)
                                    }
                                    Err(poisoned) => {
                                        eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                        QueryExecutor::resolve_binds(
                                            &sql_to_execute,
                                            &poisoned.into_inner(),
                                        )
                                    }
                                };

                                let binds = match binds {
                                    Ok(binds) => binds,
                                    Err(message) => {
                                        let emitted = SqlEditorWidget::emit_non_select_result(
                                            &sender,
                                            &session,
                                            &conn_name,
                                            result_index,
                                            &sql_text,
                                            format!("Error: {}", message),
                                            false,
                                            false,
                                            script_mode,
                                        );
                                        if emitted {
                                            result_index += 1;
                                        }
                                        if !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                };

                                let statement_start = Instant::now();
                                let mut timed_out = false;
                                let stmt = match QueryExecutor::execute_with_binds(
                                    conn.as_ref(),
                                    &sql_to_execute,
                                    &binds,
                                ) {
                                    Ok(stmt) => stmt,
                                    Err(err) => {
                                        let cancelled = SqlEditorWidget::is_cancel_error(&err);
                                        timed_out = SqlEditorWidget::is_timeout_error(&err);
                                        let message = if cancelled {
                                            SqlEditorWidget::cancel_message()
                                        } else if timed_out {
                                            SqlEditorWidget::timeout_message(query_timeout)
                                        } else {
                                            err.to_string()
                                        };
                                        if script_mode {
                                            let result =
                                                QueryResult::new_error(&sql_text, &message);
                                            SqlEditorWidget::emit_script_result(
                                                &sender,
                                                &conn_name,
                                                result_index,
                                                result,
                                                timed_out,
                                            );
                                        } else {
                                            let index = result_index;
                                            let _ = sender
                                                .send(QueryProgress::StatementStart { index });
                                            app::awake();
                                            let result =
                                                QueryResult::new_error(&sql_text, &message);
                                            let _ = sender.send(QueryProgress::StatementFinished {
                                                index,
                                                result,
                                                connection_name: conn_name.clone(),
                                                timed_out,
                                            });
                                            app::awake();
                                            result_index += 1;
                                        }
                                        SqlEditorWidget::emit_timing_if_enabled(
                                            &sender,
                                            &session,
                                            statement_start.elapsed(),
                                        );
                                        if timed_out || cancelled || !continue_on_error {
                                            stop_execution = true;
                                        }
                                        continue;
                                    }
                                };

                                let execution_time = statement_start.elapsed();
                                let timing_duration = execution_time;
                                let dml_type = if upper.starts_with("INSERT") {
                                    Some("INSERT")
                                } else if upper.starts_with("UPDATE") {
                                    Some("UPDATE")
                                } else if upper.starts_with("DELETE") {
                                    Some("DELETE")
                                } else if upper.starts_with("MERGE") {
                                    Some("MERGE")
                                } else {
                                    None
                                };

                                let mut result = if let Some(statement_type) = dml_type {
                                    let affected_rows = stmt.row_count().unwrap_or(0);
                                    QueryResult::new_dml(
                                        &sql_text,
                                        affected_rows,
                                        execution_time,
                                        statement_type,
                                    )
                                } else {
                                    QueryResult {
                                        sql: sql_text.to_string(),
                                        columns: vec![],
                                        rows: vec![],
                                        row_count: 0,
                                        execution_time,
                                        message: if upper.starts_with("CREATE")
                                            || upper.starts_with("ALTER")
                                            || upper.starts_with("DROP")
                                            || upper.starts_with("TRUNCATE")
                                            || upper.starts_with("RENAME")
                                            || upper.starts_with("GRANT")
                                            || upper.starts_with("REVOKE")
                                            || upper.starts_with("COMMENT")
                                        {
                                            SqlEditorWidget::ddl_message(&upper)
                                        } else {
                                            "Statement executed successfully".to_string()
                                        },
                                        is_select: false,
                                        success: true,
                                    }
                                };

                                let mut out_messages: Vec<String> = Vec::new();
                                if let Ok(updates) =
                                    QueryExecutor::fetch_scalar_bind_updates(&stmt, &binds)
                                {
                                    let mut guard = match session.lock() {
                                        Ok(guard) => guard,
                                        Err(poisoned) => {
                                            eprintln!(
                                            "Warning: session state lock was poisoned; recovering."
                                        );
                                            poisoned.into_inner()
                                        }
                                    };
                                    for (name, value) in updates {
                                        if let Some(bind) = guard.binds.get_mut(&name) {
                                            bind.value = value.clone();
                                        }
                                        if let BindValue::Scalar(val) = value {
                                            out_messages.push(format!(
                                                ":{} = {}",
                                                name,
                                                val.unwrap_or_else(|| "NULL".to_string())
                                            ));
                                        }
                                    }
                                }

                                if !out_messages.is_empty() {
                                    result.message = format!(
                                        "{} | OUT: {}",
                                        result.message,
                                        out_messages.join(", ")
                                    );
                                }

                                let mut compile_errors: Option<Vec<Vec<String>>> = None;
                                if let Some(object) = compiled_object.clone() {
                                    match QueryExecutor::fetch_compilation_errors(
                                        conn.as_ref(),
                                        &object,
                                    ) {
                                        Ok(rows) => {
                                            if !rows.is_empty() {
                                                result.message = format!(
                                                    "{} | Compiled with errors",
                                                    result.message
                                                );
                                                result.success = false;
                                                compile_errors = Some(rows);
                                            }
                                        }
                                        Err(err) => {
                                            result.message = format!(
                                                "{} | Failed to fetch compilation errors: {}",
                                                result.message, err
                                            );
                                            result.success = false;
                                        }
                                    }
                                }

                                if dml_type.is_some() && !auto_commit && result.success {
                                    result.message =
                                        format!("{} | Commit required", result.message);
                                }

                                if auto_commit && result.success {
                                    if let Err(err) = conn.commit() {
                                        result = QueryResult::new_error(
                                            &sql_text,
                                            &format!("Auto-commit failed: {}", err),
                                        );
                                    } else {
                                        result.message =
                                            format!("{} | Auto-commit applied", result.message);
                                    }
                                }

                                if script_mode {
                                    if result.success {
                                        SqlEditorWidget::emit_script_lines(
                                            &sender,
                                            &session,
                                            &result.message,
                                        );
                                    }
                                    SqlEditorWidget::emit_script_result(
                                        &sender,
                                        &conn_name,
                                        result_index,
                                        result.clone(),
                                        timed_out,
                                    );
                                } else {
                                    let index = result_index;
                                    let _ = sender.send(QueryProgress::StatementStart { index });
                                    app::awake();
                                    if !result.message.trim().is_empty() {
                                        SqlEditorWidget::append_spool_output(
                                            &session,
                                            &[result.message.clone()],
                                        );
                                    }
                                    let _ = sender.send(QueryProgress::StatementFinished {
                                        index,
                                        result: result.clone(),
                                        connection_name: conn_name.clone(),
                                        timed_out,
                                    });
                                    app::awake();
                                    result_index += 1;
                                }

                                if let Some(rows) = compile_errors {
                                    let (heading_enabled, feedback_enabled) =
                                        SqlEditorWidget::current_output_settings(&session);
                                    SqlEditorWidget::emit_select_result(
                                        &sender,
                                        &session,
                                        &conn_name,
                                        result_index,
                                        "COMPILE ERRORS",
                                        SqlEditorWidget::apply_heading_setting(
                                            vec![
                                                "LINE".to_string(),
                                                "POSITION".to_string(),
                                                "TEXT".to_string(),
                                            ],
                                            heading_enabled,
                                        ),
                                        rows,
                                        false,
                                        feedback_enabled,
                                    );
                                    result_index += 1;
                                }

                                let _ = SqlEditorWidget::emit_dbms_output(
                                    &sender,
                                    &conn_name,
                                    conn.as_ref(),
                                    &session,
                                    &mut result_index,
                                );
                                SqlEditorWidget::emit_timing_if_enabled(
                                    &sender,
                                    &session,
                                    timing_duration,
                                );

                                if timed_out {
                                    stop_execution = true;
                                } else if !result.success && !continue_on_error {
                                    stop_execution = true;
                                }
                            }
                        }
                    }
                }

                // Restore previous timeout if we have a connection
                if let Some(conn) = conn_opt.as_ref() {
                    let _ = conn.set_call_timeout(previous_timeout);
                }
                let _ = sender.send(QueryProgress::BatchFinished);
                app::awake();
            })); // end catch_unwind

            if let Err(e) = result {
                eprintln!("Query thread panicked: {:?}", e);
                let _ = sender.send(QueryProgress::BatchFinished);
                app::awake();
            }
        });
    }

    fn emit_non_select_result(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        conn_name: &str,
        index: usize,
        sql: &str,
        message: String,
        success: bool,
        timed_out: bool,
        script_mode: bool,
    ) -> bool {
        if script_mode {
            if success {
                SqlEditorWidget::emit_script_lines(sender, session, &message);
            }
            let result = QueryResult {
                sql: sql.to_string(),
                columns: vec![],
                rows: vec![],
                row_count: 0,
                execution_time: Duration::from_secs(0),
                message,
                is_select: false,
                success,
            };
            SqlEditorWidget::emit_script_result(sender, conn_name, index, result, timed_out);
            return false;
        }

        let _ = sender.send(QueryProgress::StatementStart { index });
        app::awake();
        if !message.trim().is_empty() {
            SqlEditorWidget::append_spool_output(session, &[message.clone()]);
        }
        let result = QueryResult {
            sql: sql.to_string(),
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time: Duration::from_secs(0),
            message,
            is_select: false,
            success,
        };
        let _ = sender.send(QueryProgress::StatementFinished {
            index,
            result,
            connection_name: conn_name.to_string(),
            timed_out,
        });
        app::awake();
        true
    }

    fn emit_script_result(
        sender: &mpsc::Sender<QueryProgress>,
        conn_name: &str,
        index: usize,
        result: QueryResult,
        timed_out: bool,
    ) {
        let _ = sender.send(QueryProgress::StatementFinished {
            index,
            result,
            connection_name: conn_name.to_string(),
            timed_out,
        });
        app::awake();
    }

    fn current_output_settings(session: &Arc<Mutex<SessionState>>) -> (bool, bool) {
        match session.lock() {
            Ok(guard) => (guard.heading_enabled, guard.feedback_enabled),
            Err(poisoned) => {
                eprintln!("Warning: session state lock was poisoned; recovering.");
                let guard = poisoned.into_inner();
                (guard.heading_enabled, guard.feedback_enabled)
            }
        }
    }

    fn apply_heading_setting(column_names: Vec<String>, heading_enabled: bool) -> Vec<String> {
        if heading_enabled {
            column_names
        } else {
            column_names.into_iter().map(|_| String::new()).collect()
        }
    }

    fn apply_heading_to_result(result: &mut QueryResult, heading_enabled: bool) {
        if heading_enabled {
            return;
        }
        for column in &mut result.columns {
            column.name.clear();
        }
    }

    fn emit_select_result(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        conn_name: &str,
        index: usize,
        sql: &str,
        column_names: Vec<String>,
        rows: Vec<Vec<String>>,
        success: bool,
        feedback_enabled: bool,
    ) {
        let _ = sender.send(QueryProgress::StatementStart { index });
        app::awake();
        let _ = sender.send(QueryProgress::SelectStart {
            index,
            columns: column_names.clone(),
        });
        app::awake();
        if !column_names.is_empty() {
            SqlEditorWidget::append_spool_output(session, &[column_names.join(" | ")]);
        }
        if !rows.is_empty() {
            let _ = sender.send(QueryProgress::Rows {
                index,
                rows: rows.clone(),
            });
            app::awake();
            SqlEditorWidget::append_spool_rows(session, &rows);
        }
        let column_info: Vec<ColumnInfo> = column_names
            .iter()
            .map(|name| ColumnInfo {
                name: name.clone(),
                data_type: "VARCHAR2".to_string(),
            })
            .collect();
        let mut result = QueryResult::new_select(sql, column_info, rows, Duration::from_secs(0));
        result.success = success;
        if !feedback_enabled {
            result.message.clear();
        }
        if !result.message.trim().is_empty() {
            SqlEditorWidget::append_spool_output(session, &[result.message.clone()]);
        }
        let _ = sender.send(QueryProgress::StatementFinished {
            index,
            result,
            connection_name: conn_name.to_string(),
            timed_out: false,
        });
        app::awake();
    }

    fn emit_script_output(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        lines: Vec<String>,
    ) {
        if lines.is_empty() {
            return;
        }
        SqlEditorWidget::append_spool_output(session, &lines);
        let _ = sender.send(QueryProgress::ScriptOutput { lines });
        app::awake();
    }

    fn emit_timing_if_enabled(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        duration: Duration,
    ) {
        let enabled = match session.lock() {
            Ok(guard) => guard.timing_enabled,
            Err(poisoned) => {
                eprintln!("Warning: session state lock was poisoned; recovering.");
                poisoned.into_inner().timing_enabled
            }
        };
        if !enabled {
            return;
        }
        let line = format!("Elapsed: {:.3}s", duration.as_secs_f64());
        SqlEditorWidget::emit_script_output(sender, session, vec![line]);
    }

    fn emit_script_lines(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        message: &str,
    ) {
        let lines: Vec<String> = message.lines().map(|line| line.to_string()).collect();
        if lines.is_empty() {
            return;
        }
        SqlEditorWidget::emit_script_output(sender, session, lines);
    }

    fn emit_script_message(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        title: &str,
        message: &str,
    ) {
        let mut lines = Vec::new();
        lines.push(format!("[{}]", title));
        for line in message.lines() {
            lines.push(line.to_string());
        }
        SqlEditorWidget::emit_script_output(sender, session, lines);
    }

    fn emit_script_table(
        sender: &mpsc::Sender<QueryProgress>,
        session: &Arc<Mutex<SessionState>>,
        title: &str,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        heading_enabled: bool,
    ) {
        let mut lines = Vec::new();
        lines.push(format!("[{}]", title));
        if heading_enabled && !columns.is_empty() {
            lines.push(columns.join(" | "));
        }
        for row in rows {
            lines.push(row.join(" | "));
        }
        SqlEditorWidget::emit_script_output(sender, session, lines);
    }

    fn append_spool_output(session: &Arc<Mutex<SessionState>>, lines: &[String]) {
        if lines.is_empty() {
            return;
        }

        let (path, truncate) = match session.lock() {
            Ok(mut guard) => {
                let path = guard.spool_path.clone();
                let truncate = guard.spool_truncate;
                if truncate {
                    guard.spool_truncate = false;
                }
                (path, truncate)
            }
            Err(poisoned) => {
                eprintln!("Warning: session state lock was poisoned; recovering.");
                let mut guard = poisoned.into_inner();
                let path = guard.spool_path.clone();
                let truncate = guard.spool_truncate;
                if truncate {
                    guard.spool_truncate = false;
                }
                (path, truncate)
            }
        };

        let Some(path) = path else {
            return;
        };

        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if truncate {
            options.truncate(true);
        } else {
            options.append(true);
        }

        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("Failed to open spool file {}: {}", path.display(), err);
                return;
            }
        };

        for line in lines {
            if let Err(err) = writeln!(file, "{}", line) {
                eprintln!("Failed to write to spool file {}: {}", path.display(), err);
                break;
            }
        }
    }

    fn append_spool_rows(session: &Arc<Mutex<SessionState>>, rows: &[Vec<String>]) {
        if rows.is_empty() {
            return;
        }
        let lines: Vec<String> = rows.iter().map(|row| row.join(" | ")).collect();
        SqlEditorWidget::append_spool_output(session, &lines);
    }

    fn apply_define_substitution(
        sql: &str,
        session: &Arc<Mutex<SessionState>>,
        sender: &mpsc::Sender<QueryProgress>,
    ) -> Result<String, String> {
        let define_char = match session.lock() {
            Ok(guard) => guard.define_char,
            Err(poisoned) => {
                eprintln!("Warning: session state lock was poisoned; recovering.");
                poisoned.into_inner().define_char
            }
        };

        let mut result = String::with_capacity(sql.len());
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
                result.push(c);
                if c == '\n' {
                    in_line_comment = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                result.push(c);
                if c == '*' && next == Some('/') {
                    result.push('/');
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if c == '-' && next == Some('-') {
                result.push('-');
                result.push('-');
                in_line_comment = true;
                i += 2;
                continue;
            }

            if c == '/' && next == Some('*') {
                result.push('/');
                result.push('*');
                in_block_comment = true;
                i += 2;
                continue;
            }

            if c == define_char {
                let is_double = next == Some(define_char);
                let start = if is_double { i + 2 } else { i + 1 };
                let mut j = start;
                while j < len {
                    let ch = chars[j];
                    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' || ch == '#' {
                        j += 1;
                    } else {
                        break;
                    }
                }

                if j == start {
                    result.push(c);
                    if is_double {
                        result.push(define_char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }

                let name: String = chars[start..j].iter().collect();
                let key = SessionState::normalize_name(&name);
                let (define_value, bind_value) = match session.lock() {
                    Ok(guard) => (
                        guard.define_vars.get(&key).cloned(),
                        guard.binds.get(&key).cloned(),
                    ),
                    Err(poisoned) => {
                        eprintln!("Warning: session state lock was poisoned; recovering.");
                        let guard = poisoned.into_inner();
                        (
                            guard.define_vars.get(&key).cloned(),
                            guard.binds.get(&key).cloned(),
                        )
                    }
                };

                let mut replacement = if let Some(value) = define_value {
                    value
                } else if let Some(bind) = bind_value {
                    SqlEditorWidget::format_define_value(&key, &bind)?
                } else {
                    let prompt = format!("Enter value for {}:", name);
                    let input = SqlEditorWidget::prompt_for_input_with_sender(sender, &prompt)?;
                    if is_double {
                        match session.lock() {
                            Ok(mut guard) => {
                                guard.define_vars.insert(key.clone(), input.clone());
                            }
                            Err(poisoned) => {
                                eprintln!("Warning: session state lock was poisoned; recovering.");
                                let mut guard = poisoned.into_inner();
                                guard.define_vars.insert(key.clone(), input.clone());
                            }
                        }
                    }
                    input
                }
                .to_string();

                if in_single_quote || in_q_quote {
                    if let Some(stripped) =
                        SqlEditorWidget::strip_wrapping_single_quotes(&replacement)
                    {
                        replacement = stripped;
                    }
                }

                result.push_str(&replacement);
                i = j;
                continue;
            }

            if in_q_quote {
                result.push(c);
                if Some(c) == q_quote_end && next == Some('\'') {
                    result.push('\'');
                    in_q_quote = false;
                    q_quote_end = None;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                result.push(c);
                if c == '\'' {
                    if next == Some('\'') {
                        result.push('\'');
                        i += 2;
                        continue;
                    }
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                result.push(c);
                if c == '"' {
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
                && i + 3 < len
            {
                let delimiter = chars[i + 3];
                let closing = match delimiter {
                    '(' => Some(')'),
                    '[' => Some(']'),
                    '{' => Some('}'),
                    '<' => Some('>'),
                    _ => Some(delimiter),
                };
                result.push(c);
                result.push(chars[i + 1]);
                result.push('\'');
                result.push(delimiter);
                in_q_quote = true;
                q_quote_end = closing;
                i += 4;
                continue;
            }

            // Handle q'[...]' (q-quoted strings)
            if (c == 'q' || c == 'Q') && next == Some('\'') && next2.is_some() {
                let delimiter = chars[i + 2];
                let closing = match delimiter {
                    '(' => Some(')'),
                    '[' => Some(']'),
                    '{' => Some('}'),
                    '<' => Some('>'),
                    _ => Some(delimiter),
                };
                result.push(c);
                result.push('\'');
                result.push(delimiter);
                in_q_quote = true;
                q_quote_end = closing;
                i += 3;
                continue;
            }

            if c == '\'' {
                result.push(c);
                in_single_quote = true;
                i += 1;
                continue;
            }

            if c == '"' {
                result.push(c);
                in_double_quote = true;
                i += 1;
                continue;
            }

            result.push(c);
            i += 1;
        }

        Ok(result)
    }

    fn prompt_for_input_with_sender(
        sender: &mpsc::Sender<QueryProgress>,
        prompt: &str,
    ) -> Result<String, String> {
        let (response_tx, response_rx) = mpsc::channel();
        if sender
            .send(QueryProgress::PromptInput {
                prompt: prompt.to_string(),
                response: response_tx,
            })
            .is_err()
        {
            return Err("Substitution prompt failed: UI disconnected.".to_string());
        }

        match response_rx.recv_timeout(Duration::from_secs(300)) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err("Substitution prompt cancelled.".to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("Substitution prompt timed out.".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Substitution prompt failed: UI disconnected.".to_string())
            }
        }
    }

    pub fn prompt_input_dialog(prompt: &str) -> Option<String> {
        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut dialog = fltk::window::Window::default()
            .with_size(420, 150)
            .with_label("Input")
            .center_screen();
        dialog.set_color(theme::panel_raised());
        dialog.make_modal(true);

        let mut main_flex = Flex::default().with_pos(10, 10).with_size(400, 130);
        main_flex.set_type(FlexType::Column);
        main_flex.set_spacing(8);

        let mut prompt_frame = Frame::default().with_label(prompt);
        prompt_frame.set_label_color(theme::text_primary());
        prompt_frame.set_align(Align::Left | Align::Inside | Align::Wrap);
        main_flex.fixed(&prompt_frame, 48);

        let mut input = Input::default();
        input.set_color(theme::input_bg());
        input.set_text_color(theme::text_primary());
        input.set_trigger(CallbackTrigger::EnterKeyAlways);
        main_flex.fixed(&input, 30);

        let mut button_flex = Flex::default();
        button_flex.set_type(FlexType::Row);
        button_flex.set_spacing(8);

        let _spacer = Frame::default();

        let mut ok_btn = Button::default().with_size(80, 24).with_label("OK");
        ok_btn.set_color(theme::button_primary());
        ok_btn.set_label_color(theme::text_primary());
        ok_btn.set_frame(FrameType::RFlatBox);

        let mut cancel_btn = Button::default().with_size(80, 24).with_label("Cancel");
        cancel_btn.set_color(theme::button_subtle());
        cancel_btn.set_label_color(theme::text_primary());
        cancel_btn.set_frame(FrameType::RFlatBox);

        button_flex.fixed(&ok_btn, 80);
        button_flex.fixed(&cancel_btn, 80);
        button_flex.end();
        main_flex.fixed(&button_flex, 28);
        main_flex.end();
        dialog.end();

        let result: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let cancelled = Rc::new(RefCell::new(false));

        {
            let result = result.clone();
            let mut dialog = dialog.clone();
            let input = input.clone();
            ok_btn.set_callback(move |_| {
                *result.borrow_mut() = Some(input.value());
                dialog.hide();
            });
        }

        {
            let cancelled = cancelled.clone();
            let mut dialog = dialog.clone();
            cancel_btn.set_callback(move |_| {
                *cancelled.borrow_mut() = true;
                dialog.hide();
            });
        }

        {
            let result = result.clone();
            let mut input_cb = input.clone();
            let input_value = input.clone();
            let mut dialog_cb = dialog.clone();
            input_cb.set_callback(move |_| {
                *result.borrow_mut() = Some(input_value.value());
                dialog_cb.hide();
            });
        }

        {
            let cancelled = cancelled.clone();
            let mut dialog_cb = dialog.clone();
            let mut dialog_handle = dialog.clone();
            dialog_cb.set_callback(move |_| {
                *cancelled.borrow_mut() = true;
                dialog_handle.hide();
            });
        }

        dialog.show();
        input.take_focus().ok();

        while dialog.shown() {
            app::wait();
        }

        if *cancelled.borrow() {
            None
        } else {
            result.borrow().clone()
        }
    }

    fn format_define_value(name: &str, bind: &BindVar) -> Result<String, String> {
        let BindValue::Scalar(value) = &bind.value else {
            return Err(format!(
                "Substitution variable &{} must be a scalar value.",
                name
            ));
        };

        let value = value
            .as_ref()
            .ok_or_else(|| format!("Substitution variable &{} has no value.", name))?;

        if value.eq_ignore_ascii_case("NULL") {
            return Ok("NULL".to_string());
        }

        match bind.data_type {
            crate::db::session::BindDataType::Number => Ok(value.clone()),
            crate::db::session::BindDataType::Date
            | crate::db::session::BindDataType::Timestamp(_)
            | crate::db::session::BindDataType::Varchar2(_)
            | crate::db::session::BindDataType::Clob => {
                Ok(format!("'{}'", SqlEditorWidget::escape_sql_literal(value)))
            }
            crate::db::session::BindDataType::RefCursor => Err(format!(
                "Substitution variable &{} cannot be a REFCURSOR.",
                name
            )),
        }
    }

    fn escape_sql_literal(value: &str) -> String {
        value.replace('\'', "''")
    }

    fn strip_wrapping_single_quotes(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.len() < 2 {
            return None;
        }
        if trimmed.starts_with('\'') && trimmed.ends_with('\'') {
            Some(trimmed[1..trimmed.len() - 1].to_string())
        } else {
            None
        }
    }

    fn emit_dbms_output(
        sender: &mpsc::Sender<QueryProgress>,
        _conn_name: &str,
        conn: &Connection,
        session: &Arc<Mutex<SessionState>>,
        _result_index: &mut usize,
    ) -> Result<(), OracleError> {
        let (enabled, size) = match session.lock() {
            Ok(guard) => (guard.server_output.enabled, guard.server_output.size),
            Err(poisoned) => {
                eprintln!("Warning: session state lock was poisoned; recovering.");
                let guard = poisoned.into_inner();
                (guard.server_output.enabled, guard.server_output.size)
            }
        };

        if !enabled {
            return Ok(());
        }

        let max_lines = if size == 0 {
            10_000
        } else {
            (size / 80).max(1).min(10_000)
        };
        let lines = QueryExecutor::get_dbms_output(conn, max_lines)?;
        if lines.is_empty() {
            return Ok(());
        }

        let mut output_lines = Vec::with_capacity(lines.len() + 1);
        output_lines.push("DBMS_OUTPUT".to_string());
        output_lines.extend(lines);
        SqlEditorWidget::emit_script_output(sender, session, output_lines);
        Ok(())
    }

    fn ensure_plsql_terminator(sql: &str) -> String {
        let trimmed = sql.trim_end();
        if trimmed.ends_with(';') {
            trimmed.to_string()
        } else {
            format!("{};", trimmed)
        }
    }

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

    fn normalize_object_name(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            trimmed.trim_matches('"').to_string()
        } else {
            trimmed.to_uppercase()
        }
    }

    fn ddl_message(sql_upper: &str) -> String {
        if sql_upper.starts_with("CREATE") {
            if sql_upper.contains(" TABLE ") {
                "Table created".to_string()
            } else if sql_upper.contains(" VIEW ") {
                "View created".to_string()
            } else if sql_upper.contains(" INDEX ") {
                "Index created".to_string()
            } else if sql_upper.contains(" PROCEDURE ") {
                "Procedure created".to_string()
            } else if sql_upper.contains(" FUNCTION ") {
                "Function created".to_string()
            } else if sql_upper.contains(" PACKAGE ") {
                "Package created".to_string()
            } else if sql_upper.contains(" TRIGGER ") {
                "Trigger created".to_string()
            } else if sql_upper.contains(" SEQUENCE ") {
                "Sequence created".to_string()
            } else if sql_upper.contains(" SYNONYM ") {
                "Synonym created".to_string()
            } else if sql_upper.contains(" TYPE ") {
                "Type created".to_string()
            } else {
                "Object created".to_string()
            }
        } else if sql_upper.starts_with("ALTER") {
            "Object altered".to_string()
        } else if sql_upper.starts_with("DROP") {
            "Object dropped".to_string()
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
        }
    }

    fn is_timeout_error(err: &OracleError) -> bool {
        let message = err.to_string();
        message.contains("DPI-1067")
    }

    fn is_cancel_error(err: &OracleError) -> bool {
        let message = err.to_string();
        message.contains("ORA-01013")
    }

    fn timeout_message(timeout: Option<Duration>) -> String {
        match timeout {
            Some(duration) => format!("Query timed out after {} seconds", duration.as_secs()),
            None => "Query timed out".to_string(),
        }
    }

    fn cancel_message() -> String {
        "Query cancelled".to_string()
    }

    fn parse_timeout(value: &str) -> Option<Duration> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        let secs = match trimmed.parse::<u64>() {
            Ok(secs) => secs,
            Err(err) => {
                eprintln!("Invalid timeout value '{trimmed}': {err}");
                return None;
            }
        };
        if secs == 0 {
            None
        } else {
            Some(Duration::from_secs(secs))
        }
    }

    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: FnMut(QueryProgress) + 'static,
    {
        *self.progress_callback.borrow_mut() = Some(Box::new(callback));
    }
}
