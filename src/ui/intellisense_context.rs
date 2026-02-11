use std::collections::HashSet;

use crate::ui::sql_editor::SqlToken;

/// SQL clause phase within a query at a specific depth level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlPhase {
    Initial,
    WithClause,
    SelectList,
    IntoClause,
    FromClause,
    JoinCondition,
    WhereClause,
    GroupByClause,
    HavingClause,
    OrderByClause,
    SetClause,
    ConnectByClause,
    StartWithClause,
    ValuesClause,
    UpdateTarget,
    DeleteTarget,
    MergeTarget,
    PivotClause,
    ModelClause,
}

impl SqlPhase {
    pub fn is_column_context(&self) -> bool {
        matches!(
            self,
            SqlPhase::SelectList
                | SqlPhase::WhereClause
                | SqlPhase::JoinCondition
                | SqlPhase::GroupByClause
                | SqlPhase::HavingClause
                | SqlPhase::OrderByClause
                | SqlPhase::SetClause
                | SqlPhase::ConnectByClause
                | SqlPhase::StartWithClause
        )
    }

    pub fn is_table_context(&self) -> bool {
        matches!(
            self,
            SqlPhase::FromClause
                | SqlPhase::IntoClause
                | SqlPhase::UpdateTarget
                | SqlPhase::DeleteTarget
                | SqlPhase::MergeTarget
        )
    }
}

/// A table/view reference with optional alias, collected from a query scope.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScopedTableRef {
    pub name: String,
    pub alias: Option<String>,
    pub depth: usize,
    pub is_cte: bool,
}

/// CTE definition parsed from WITH clause.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CteDefinition {
    pub name: String,
    pub explicit_columns: Vec<String>,
    /// Tokens inside the CTE body parentheses (the SELECT statement).
    pub body_tokens: Vec<SqlToken>,
}

/// A subquery alias with its body tokens, for column inference.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubqueryDefinition {
    pub alias: String,
    pub body_tokens: Vec<SqlToken>,
    pub depth: usize,
}

/// Result of deep context analysis at cursor position.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CursorContext {
    /// Current SQL phase at cursor position
    pub phase: SqlPhase,
    /// Current parenthesis nesting depth (0 = top level)
    pub depth: usize,
    /// All tables visible at cursor position (current scope + parent scopes + CTEs)
    pub tables_in_scope: Vec<ScopedTableRef>,
    /// CTEs defined in WITH clause
    pub ctes: Vec<CteDefinition>,
    /// Subquery aliases with their body tokens for column inference
    pub subqueries: Vec<SubqueryDefinition>,
    /// The qualifier before cursor (e.g., "t" in "t.col")
    pub qualifier: Option<String>,
    /// Resolved table names for the qualifier
    pub qualifier_tables: Vec<String>,
}

/// CTE parsing state machine
#[derive(Debug, Clone, Copy, PartialEq)]
enum CteState {
    None,
    ExpectName,
    AfterName,
    ExpectAs,
    ExpectBody,
    InBody,
}

/// Analyze the SQL text from statement start to cursor position.
/// Returns a `CursorContext` describing the phase, depth, and available tables.
///
/// `before_cursor` is the SQL text from statement start up to cursor.
/// `full_statement` is the complete statement text (for collecting all table references).
/// `tokenize` is used to tokenize the SQL.
pub fn analyze_cursor_context(before_cursor: &[SqlToken], full_statement: &[SqlToken]) -> CursorContext {
    let phase_analysis = analyze_phase(before_cursor);
    let table_analysis = collect_tables_deep(full_statement, phase_analysis.depth);
    let ctes = parse_ctes(full_statement);

    let mut tables_in_scope = table_analysis.tables;
    for cte in &ctes {
        let already = tables_in_scope
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(&cte.name));
        if !already {
            tables_in_scope.push(ScopedTableRef {
                name: cte.name.clone(),
                alias: None,
                depth: 0,
                is_cte: true,
            });
        }
    }

    CursorContext {
        phase: phase_analysis.phase,
        depth: phase_analysis.depth,
        tables_in_scope,
        ctes,
        subqueries: table_analysis.subqueries,
        qualifier: None,
        qualifier_tables: Vec::new(),
    }
}

struct PhaseAnalysis {
    phase: SqlPhase,
    depth: usize,
}

/// Walk tokens up to cursor to determine the current SQL phase and depth.
fn analyze_phase(tokens: &[SqlToken]) -> PhaseAnalysis {
    let mut depth: usize = 0;
    // Track phase at each depth level
    let mut phase_stack: Vec<SqlPhase> = vec![SqlPhase::Initial];
    let mut cte_state = CteState::None;
    let mut cte_paren_depth: usize = 0;
    let mut idx = 0;

    while idx < tokens.len() {
        let token = &tokens[idx];

        match token {
            SqlToken::Symbol(sym) if sym == "(" => {
                depth += 1;
                if phase_stack.len() <= depth {
                    phase_stack.push(SqlPhase::Initial);
                } else {
                    phase_stack[depth] = SqlPhase::Initial;
                }
                if matches!(cte_state, CteState::ExpectBody) {
                    cte_state = CteState::InBody;
                    cte_paren_depth = depth;
                }
                if matches!(cte_state, CteState::AfterName) {
                    // CTE explicit columns: WITH cte(col1, col2) AS (...)
                    // Skip until matching ')'
                    cte_state = CteState::ExpectAs;
                }
                idx += 1;
                continue;
            }
            SqlToken::Symbol(sym) if sym == ")" => {
                if matches!(cte_state, CteState::InBody) && depth == cte_paren_depth {
                    cte_state = CteState::None;
                }
                if depth > 0 {
                    depth -= 1;
                }
                idx += 1;
                continue;
            }
            SqlToken::Comment(_) | SqlToken::String(_) => {
                idx += 1;
                continue;
            }
            SqlToken::Word(word) => {
                let upper = word.to_uppercase();

                // CTE state machine
                match cte_state {
                    CteState::ExpectName if upper != "RECURSIVE" => {
                        cte_state = CteState::AfterName;
                        idx += 1;
                        continue;
                    }
                    CteState::AfterName => {
                        if upper == "AS" {
                            cte_state = CteState::ExpectBody;
                        }
                        idx += 1;
                        continue;
                    }
                    CteState::ExpectAs => {
                        if upper == "AS" {
                            cte_state = CteState::ExpectBody;
                        }
                        idx += 1;
                        continue;
                    }
                    CteState::InBody => {
                        // Inside CTE body, process normally for phase tracking at this depth
                        // but don't break out of CTE state
                    }
                    CteState::None => {}
                    _ => {
                        idx += 1;
                        continue;
                    }
                }

                // Ensure phase_stack has entry for current depth
                while phase_stack.len() <= depth {
                    phase_stack.push(SqlPhase::Initial);
                }

                let current_phase = phase_stack[depth];

                match upper.as_str() {
                    "WITH" if matches!(current_phase, SqlPhase::Initial) => {
                        phase_stack[depth] = SqlPhase::WithClause;
                        cte_state = CteState::ExpectName;
                    }
                    "SELECT" => {
                        phase_stack[depth] = SqlPhase::SelectList;
                    }
                    "FROM" => {
                        // Avoid transition for EXTRACT(... FROM ...)
                        if !matches!(current_phase, SqlPhase::Initial) || depth > 0 {
                            phase_stack[depth] = SqlPhase::FromClause;
                        } else {
                            phase_stack[depth] = SqlPhase::FromClause;
                        }
                    }
                    "INTO" => {
                        if matches!(
                            current_phase,
                            SqlPhase::SelectList | SqlPhase::Initial
                        ) {
                            phase_stack[depth] = SqlPhase::IntoClause;
                        }
                    }
                    "JOIN" => {
                        // JOIN resets to FROM context for next table
                        phase_stack[depth] = SqlPhase::FromClause;
                    }
                    "ON" => {
                        if matches!(current_phase, SqlPhase::FromClause) {
                            phase_stack[depth] = SqlPhase::JoinCondition;
                        }
                    }
                    "WHERE" => {
                        phase_stack[depth] = SqlPhase::WhereClause;
                    }
                    "GROUP" => {
                        if peek_word_upper(tokens, idx + 1) == Some("BY") {
                            phase_stack[depth] = SqlPhase::GroupByClause;
                            idx += 1; // skip BY
                        }
                    }
                    "HAVING" => {
                        phase_stack[depth] = SqlPhase::HavingClause;
                    }
                    "ORDER" => {
                        if peek_word_upper(tokens, idx + 1) == Some("BY") {
                            phase_stack[depth] = SqlPhase::OrderByClause;
                            idx += 1; // skip BY
                        }
                    }
                    "SET" => {
                        phase_stack[depth] = SqlPhase::SetClause;
                    }
                    "UPDATE" => {
                        phase_stack[depth] = SqlPhase::UpdateTarget;
                    }
                    "DELETE" => {
                        phase_stack[depth] = SqlPhase::DeleteTarget;
                    }
                    "MERGE" => {
                        phase_stack[depth] = SqlPhase::MergeTarget;
                    }
                    "CONNECT" => {
                        if peek_word_upper(tokens, idx + 1) == Some("BY") {
                            phase_stack[depth] = SqlPhase::ConnectByClause;
                            idx += 1;
                        }
                    }
                    "START" => {
                        if peek_word_upper(tokens, idx + 1) == Some("WITH") {
                            phase_stack[depth] = SqlPhase::StartWithClause;
                            idx += 1;
                        }
                    }
                    "VALUES" => {
                        phase_stack[depth] = SqlPhase::ValuesClause;
                    }
                    "PIVOT" | "UNPIVOT" => {
                        phase_stack[depth] = SqlPhase::PivotClause;
                    }
                    "MODEL" => {
                        phase_stack[depth] = SqlPhase::ModelClause;
                    }
                    // Set operations reset to Initial for next SELECT
                    "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => {
                        phase_stack[depth] = SqlPhase::Initial;
                    }
                    // After comma in WITH clause, expect next CTE name
                    _ => {
                        if matches!(cte_state, CteState::None)
                            && matches!(phase_stack.get(0), Some(SqlPhase::WithClause))
                            && depth == 0
                        {
                            // We might be between CTE definitions
                        }
                    }
                }
            }
            SqlToken::Symbol(sym) if sym == "," => {
                // After comma in WITH clause at depth 0, expect next CTE name
                if matches!(cte_state, CteState::None)
                    && depth == 0
                    && matches!(phase_stack.first(), Some(SqlPhase::WithClause))
                {
                    cte_state = CteState::ExpectName;
                }
            }
            _ => {}
        }
        idx += 1;
    }

    let phase = phase_stack.get(depth).copied().unwrap_or(SqlPhase::Initial);

    PhaseAnalysis { phase, depth }
}

struct TableAnalysis {
    tables: Vec<ScopedTableRef>,
    subqueries: Vec<SubqueryDefinition>,
}

/// Collect all table references from the full statement, tracking depth.
/// Returns tables visible at the given `target_depth`.
fn collect_tables_deep(tokens: &[SqlToken], target_depth: usize) -> TableAnalysis {
    let mut all_tables: Vec<ScopedTableRef> = Vec::new();
    let mut all_subqueries: Vec<SubqueryDefinition> = Vec::new();
    let mut depth: usize = 0;
    let mut phase_stack: Vec<SqlPhase> = vec![SqlPhase::Initial];
    let mut expect_table = false;
    let mut cte_state = CteState::None;
    let mut cte_paren_depth: usize = 0;
    // Track subquery aliases: when we close a paren at a certain depth in FROM context,
    // store (depth, start_token_idx) so we can capture body tokens
    let mut subquery_tracks: Vec<(usize, usize)> = Vec::new(); // (depth, start_idx)
    let mut idx = 0;

    while idx < tokens.len() {
        let token = &tokens[idx];

        match token {
            SqlToken::Symbol(sym) if sym == "(" => {
                let parent_phase = phase_stack.get(depth).copied().unwrap_or(SqlPhase::Initial);
                depth += 1;
                while phase_stack.len() <= depth {
                    phase_stack.push(SqlPhase::Initial);
                }
                phase_stack[depth] = SqlPhase::Initial;
                expect_table = false;

                if matches!(parent_phase, SqlPhase::FromClause) {
                    subquery_tracks.push((depth, idx + 1)); // depth after increment, token after '('
                }
                if matches!(cte_state, CteState::ExpectBody) {
                    cte_state = CteState::InBody;
                    cte_paren_depth = depth;
                }
                idx += 1;
                continue;
            }
            SqlToken::Symbol(sym) if sym == ")" => {
                if matches!(cte_state, CteState::InBody) && depth == cte_paren_depth {
                    cte_state = CteState::None;
                }

                let was_subquery = subquery_tracks.last().map(|t| t.0) == Some(depth);
                if was_subquery {
                    let (_, start_idx) = subquery_tracks.pop().unwrap();
                    // Look for alias after the closing paren
                    if let Some((alias, next_idx)) = parse_subquery_alias(tokens, idx + 1) {
                        // Capture body tokens for column inference
                        let body_tokens: Vec<SqlToken> = tokens[start_idx..idx]
                            .iter()
                            .cloned()
                            .collect();
                        all_subqueries.push(SubqueryDefinition {
                            alias: alias.clone(),
                            body_tokens,
                            depth: depth.saturating_sub(1),
                        });
                        all_tables.push(ScopedTableRef {
                            name: alias.clone(),
                            alias: Some(alias),
                            depth: depth.saturating_sub(1),
                            is_cte: false,
                        });
                        idx = next_idx;
                        if depth > 0 {
                            depth -= 1;
                        }
                        continue;
                    }
                }

                if depth > 0 {
                    depth -= 1;
                }
                idx += 1;
                continue;
            }
            SqlToken::Comment(_) | SqlToken::String(_) => {
                idx += 1;
                continue;
            }
            SqlToken::Symbol(sym) if sym == "," => {
                // After comma in FROM clause, expect another table
                let current_phase = phase_stack.get(depth).copied().unwrap_or(SqlPhase::Initial);
                if matches!(current_phase, SqlPhase::FromClause) {
                    expect_table = true;
                }
                // After comma in WITH clause, expect next CTE
                if matches!(cte_state, CteState::None)
                    && depth == 0
                    && matches!(phase_stack.first(), Some(SqlPhase::WithClause))
                {
                    cte_state = CteState::ExpectName;
                }
                idx += 1;
                continue;
            }
            SqlToken::Symbol(sym) if sym == ";" => {
                // Statement boundary - reset everything
                all_tables.clear();
                all_subqueries.clear();
                depth = 0;
                phase_stack = vec![SqlPhase::Initial];
                expect_table = false;
                cte_state = CteState::None;
                subquery_tracks.clear();
                idx += 1;
                continue;
            }
            SqlToken::Word(word) => {
                let upper = word.to_uppercase();

                // CTE state machine for table collection
                match cte_state {
                    CteState::ExpectName if upper != "RECURSIVE" => {
                        cte_state = CteState::AfterName;
                        idx += 1;
                        continue;
                    }
                    CteState::AfterName => {
                        if upper == "AS" {
                            cte_state = CteState::ExpectBody;
                        }
                        idx += 1;
                        continue;
                    }
                    CteState::ExpectAs => {
                        if upper == "AS" {
                            cte_state = CteState::ExpectBody;
                        }
                        idx += 1;
                        continue;
                    }
                    CteState::InBody => {
                        // Process normally inside CTE body
                    }
                    CteState::None => {}
                    _ => {
                        idx += 1;
                        continue;
                    }
                }

                while phase_stack.len() <= depth {
                    phase_stack.push(SqlPhase::Initial);
                }

                // Phase transitions
                match upper.as_str() {
                    "WITH" if matches!(phase_stack[depth], SqlPhase::Initial) => {
                        phase_stack[depth] = SqlPhase::WithClause;
                        cte_state = CteState::ExpectName;
                        expect_table = false;
                    }
                    "SELECT" => {
                        phase_stack[depth] = SqlPhase::SelectList;
                        expect_table = false;
                    }
                    "FROM" => {
                        phase_stack[depth] = SqlPhase::FromClause;
                        expect_table = true;
                    }
                    "JOIN" => {
                        phase_stack[depth] = SqlPhase::FromClause;
                        expect_table = true;
                    }
                    "INTO" if matches!(
                        phase_stack[depth],
                        SqlPhase::SelectList | SqlPhase::Initial
                    ) =>
                    {
                        phase_stack[depth] = SqlPhase::IntoClause;
                        expect_table = true;
                    }
                    "UPDATE" => {
                        phase_stack[depth] = SqlPhase::UpdateTarget;
                        expect_table = true;
                    }
                    "ON" if matches!(phase_stack[depth], SqlPhase::FromClause) => {
                        phase_stack[depth] = SqlPhase::JoinCondition;
                        expect_table = false;
                    }
                    "WHERE" | "HAVING" => {
                        phase_stack[depth] = if upper == "WHERE" {
                            SqlPhase::WhereClause
                        } else {
                            SqlPhase::HavingClause
                        };
                        expect_table = false;
                    }
                    "GROUP" if peek_word_upper(tokens, idx + 1) == Some("BY") => {
                        phase_stack[depth] = SqlPhase::GroupByClause;
                        expect_table = false;
                        idx += 1;
                    }
                    "ORDER" if peek_word_upper(tokens, idx + 1) == Some("BY") => {
                        phase_stack[depth] = SqlPhase::OrderByClause;
                        expect_table = false;
                        idx += 1;
                    }
                    "SET" => {
                        phase_stack[depth] = SqlPhase::SetClause;
                        expect_table = false;
                    }
                    "CONNECT" if peek_word_upper(tokens, idx + 1) == Some("BY") => {
                        phase_stack[depth] = SqlPhase::ConnectByClause;
                        expect_table = false;
                        idx += 1;
                    }
                    "START" if peek_word_upper(tokens, idx + 1) == Some("WITH") => {
                        phase_stack[depth] = SqlPhase::StartWithClause;
                        expect_table = false;
                        idx += 1;
                    }
                    "VALUES" => {
                        phase_stack[depth] = SqlPhase::ValuesClause;
                        expect_table = false;
                    }
                    "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => {
                        phase_stack[depth] = SqlPhase::Initial;
                        expect_table = false;
                    }
                    // Keywords that signal end of FROM clause table collection
                    kw if is_table_stop_keyword(kw) && expect_table => {
                        expect_table = false;
                    }
                    _ => {
                        if expect_table {
                            // Try to parse a table name
                            if let Some((table_name, next_idx)) = parse_table_name_deep(tokens, idx)
                            {
                                let (alias, after_alias) = parse_alias_deep(tokens, next_idx);
                                all_tables.push(ScopedTableRef {
                                    name: table_name,
                                    alias,
                                    depth,
                                    is_cte: false,
                                });
                                // Check if next is comma (continue expecting tables)
                                if let Some(SqlToken::Symbol(sym)) = tokens.get(after_alias) {
                                    if sym == "," {
                                        expect_table = true;
                                        idx = after_alias + 1;
                                        continue;
                                    }
                                }
                                expect_table = false;
                                idx = after_alias;
                                continue;
                            }
                            expect_table = false;
                        }
                    }
                }
            }
            _ => {}
        }
        idx += 1;
    }

    // Filter tables visible at target_depth: tables at depth <= target_depth are visible
    let visible: Vec<ScopedTableRef> = all_tables
        .into_iter()
        .filter(|t| t.depth <= target_depth)
        .collect();

    // Filter subqueries visible at target_depth
    let visible_subqueries: Vec<SubqueryDefinition> = all_subqueries
        .into_iter()
        .filter(|s| s.depth <= target_depth)
        .collect();

    TableAnalysis {
        tables: visible,
        subqueries: visible_subqueries,
    }
}

/// Parse CTE definitions from WITH clause.
fn parse_ctes(tokens: &[SqlToken]) -> Vec<CteDefinition> {
    let mut ctes = Vec::new();
    let mut idx = 0;

    // Find WITH keyword
    while idx < tokens.len() {
        if let SqlToken::Word(w) = &tokens[idx] {
            if w.to_uppercase() == "WITH" {
                idx += 1;
                break;
            }
        }
        // If we hit SELECT/INSERT/UPDATE/DELETE before WITH, no CTEs
        if let SqlToken::Word(w) = &tokens[idx] {
            let u = w.to_uppercase();
            if matches!(u.as_str(), "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE") {
                return ctes;
            }
        }
        idx += 1;
    }

    // Skip RECURSIVE if present
    if let Some(SqlToken::Word(w)) = tokens.get(idx) {
        if w.to_uppercase() == "RECURSIVE" {
            idx += 1;
        }
    }

    // Parse CTE definitions
    loop {
        if idx >= tokens.len() {
            break;
        }

        // Expect CTE name
        let cte_name = match tokens.get(idx) {
            Some(SqlToken::Word(w)) => {
                let u = w.to_uppercase();
                if matches!(u.as_str(), "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE") {
                    break;
                }
                w.clone()
            }
            _ => break,
        };
        idx += 1;

        let mut explicit_columns = Vec::new();

        // Check for explicit column list: cte_name(col1, col2)
        if let Some(SqlToken::Symbol(s)) = tokens.get(idx) {
            if s == "(" {
                idx += 1;
                let mut paren_depth = 1;
                while idx < tokens.len() && paren_depth > 0 {
                    match &tokens[idx] {
                        SqlToken::Symbol(s) if s == "(" => paren_depth += 1,
                        SqlToken::Symbol(s) if s == ")" => {
                            paren_depth -= 1;
                            if paren_depth == 0 {
                                idx += 1;
                                break;
                            }
                        }
                        SqlToken::Word(w) if paren_depth == 1 => {
                            explicit_columns.push(w.clone());
                        }
                        _ => {}
                    }
                    idx += 1;
                }
            }
        }

        // Expect AS
        if let Some(SqlToken::Word(w)) = tokens.get(idx) {
            if w.to_uppercase() == "AS" {
                idx += 1;
            }
        }

        // Capture CTE body tokens (balanced parens)
        let mut body_tokens = Vec::new();
        if let Some(SqlToken::Symbol(s)) = tokens.get(idx) {
            if s == "(" {
                idx += 1;
                let mut paren_depth = 1;
                while idx < tokens.len() && paren_depth > 0 {
                    match &tokens[idx] {
                        SqlToken::Symbol(s) if s == "(" => {
                            paren_depth += 1;
                            body_tokens.push(tokens[idx].clone());
                        }
                        SqlToken::Symbol(s) if s == ")" => {
                            paren_depth -= 1;
                            if paren_depth > 0 {
                                body_tokens.push(tokens[idx].clone());
                            }
                        }
                        _ => {
                            body_tokens.push(tokens[idx].clone());
                        }
                    }
                    idx += 1;
                }
            }
        }

        ctes.push(CteDefinition {
            name: cte_name,
            explicit_columns,
            body_tokens,
        });

        // Check for comma (another CTE) or end
        match tokens.get(idx) {
            Some(SqlToken::Symbol(s)) if s == "," => {
                idx += 1;
                continue;
            }
            _ => break,
        }
    }

    ctes
}

/// Peek at the next word token (skipping comments) and return its uppercase form.
fn peek_word_upper(tokens: &[SqlToken], idx: usize) -> Option<&'static str> {
    let mut i = idx;
    while i < tokens.len() {
        match &tokens[i] {
            SqlToken::Comment(_) => {
                i += 1;
                continue;
            }
            SqlToken::Word(w) => {
                let upper = w.to_uppercase();
                // Return a static str by matching known keywords
                return match upper.as_str() {
                    "BY" => Some("BY"),
                    "WITH" => Some("WITH"),
                    "AS" => Some("AS"),
                    _ => None,
                };
            }
            _ => return None,
        }
    }
    None
}

fn strip_identifier_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].replace("\"\"", "\"")
    } else {
        trimmed.to_string()
    }
}

fn normalize_identifier_for_lookup(value: &str) -> String {
    strip_identifier_quotes(value).to_uppercase()
}

/// Parse a table name at the given position (handling schema.table format).
fn parse_table_name_deep(tokens: &[SqlToken], start: usize) -> Option<(String, usize)> {
    match tokens.get(start) {
        Some(SqlToken::Symbol(sym)) if sym == "(" => None,
        Some(SqlToken::Word(word)) => {
            let is_quoted = word.trim().starts_with('"') && word.trim().ends_with('"');
            let upper = word.to_uppercase();
            // Skip if this is a keyword rather than a table name
            if !is_quoted && (is_join_keyword(&upper) || is_table_stop_keyword(&upper)) {
                return None;
            }
            let mut table = strip_identifier_quotes(word);
            let mut idx = start + 1;
            // Handle schema.table
            if matches!(tokens.get(idx), Some(SqlToken::Symbol(sym)) if sym == ".") {
                if let Some(SqlToken::Word(name)) = tokens.get(idx + 1) {
                    table = strip_identifier_quotes(name);
                    idx += 2;
                }
            }
            Some((table, idx))
        }
        _ => None,
    }
}

/// Parse an optional alias after a table name.
fn parse_alias_deep(tokens: &[SqlToken], start: usize) -> (Option<String>, usize) {
    match tokens.get(start) {
        Some(SqlToken::Word(word)) => {
            let is_quoted = word.trim().starts_with('"') && word.trim().ends_with('"');
            let upper = word.to_uppercase();
            if upper == "AS" {
                if let Some(SqlToken::Word(alias)) = tokens.get(start + 1) {
                    return (Some(strip_identifier_quotes(alias)), start + 2);
                }
                return (None, start + 1);
            }
            if is_quoted || !is_alias_breaker(&upper) {
                return (Some(strip_identifier_quotes(word)), start + 1);
            }
        }
        _ => {}
    }
    (None, start)
}

/// Parse an alias after a subquery closing ')'.
fn parse_subquery_alias(tokens: &[SqlToken], start: usize) -> Option<(String, usize)> {
    let mut idx = start;
    // Skip comments
    while idx < tokens.len() {
        if let SqlToken::Comment(_) = &tokens[idx] {
            idx += 1;
            continue;
        }
        break;
    }

    match tokens.get(idx) {
        Some(SqlToken::Word(word)) => {
            let is_quoted = word.trim().starts_with('"') && word.trim().ends_with('"');
            let upper = word.to_uppercase();
            if upper == "AS" {
                idx += 1;
                // Skip comments after AS
                while idx < tokens.len() {
                    if let SqlToken::Comment(_) = &tokens[idx] {
                        idx += 1;
                        continue;
                    }
                    break;
                }
                if let Some(SqlToken::Word(alias)) = tokens.get(idx) {
                    return Some((strip_identifier_quotes(alias), idx + 1));
                }
                return None;
            }
            if is_quoted || (!is_alias_breaker(&upper) && !is_join_keyword(&upper)) {
                return Some((strip_identifier_quotes(word), idx + 1));
            }
            None
        }
        _ => None,
    }
}

fn is_join_keyword(word: &str) -> bool {
    matches!(
        word,
        "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "CROSS"
            | "OUTER"
            | "NATURAL"
            | "LATERAL"
    )
}

fn is_table_stop_keyword(word: &str) -> bool {
    matches!(
        word,
        "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "CONNECT"
            | "START"
            | "UNION"
            | "INTERSECT"
            | "EXCEPT"
            | "MINUS"
            | "FETCH"
            | "FOR"
            | "WINDOW"
            | "QUALIFY"
            | "LIMIT"
            | "OFFSET"
            | "RETURNING"
            | "VALUES"
            | "SET"
            | "ON"
            | "PIVOT"
            | "UNPIVOT"
            | "MODEL"
            | "USING"
    )
}

fn is_alias_breaker(word: &str) -> bool {
    matches!(
        word,
        "ON" | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "CROSS"
            | "OUTER"
            | "NATURAL"
            | "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "CONNECT"
            | "START"
            | "UNION"
            | "INTERSECT"
            | "EXCEPT"
            | "MINUS"
            | "FETCH"
            | "FOR"
            | "WINDOW"
            | "QUALIFY"
            | "LIMIT"
            | "OFFSET"
            | "RETURNING"
            | "VALUES"
            | "SET"
            | "USING"
            | "PIVOT"
            | "UNPIVOT"
            | "MODEL"
            | "SELECT"
            | "FROM"
            | "INTO"
    )
}

/// Resolve which tables are relevant for a given qualifier (alias or table name).
pub fn resolve_qualifier_tables(
    qualifier: &str,
    tables_in_scope: &[ScopedTableRef],
) -> Vec<String> {
    let qualifier_upper = normalize_identifier_for_lookup(qualifier);
    let mut alias_match: Option<(usize, String)> = None;
    let mut name_match: Option<(usize, String)> = None;
    let mut seen = HashSet::new();

    for table_ref in tables_in_scope {
        let name_upper = normalize_identifier_for_lookup(&table_ref.name);
        let alias_upper = table_ref
            .alias
            .as_ref()
            .map(|a| normalize_identifier_for_lookup(a));

        if alias_upper.as_deref() == Some(qualifier_upper.as_str()) {
            if alias_match
                .as_ref()
                .map_or(true, |(depth, _)| table_ref.depth >= *depth)
            {
                alias_match = Some((table_ref.depth, table_ref.name.clone()));
            }
            continue;
        }

        if name_upper == qualifier_upper
            && name_match
                .as_ref()
                .map_or(true, |(depth, _)| table_ref.depth >= *depth)
        {
            name_match = Some((table_ref.depth, table_ref.name.clone()));
        }
    }

    if let Some((_, name)) = alias_match {
        if seen.insert(name.to_uppercase()) {
            return vec![name];
        }
    }

    if let Some((_, name)) = name_match {
        if seen.insert(name.to_uppercase()) {
            return vec![name];
        }
    }

    // If no match found, try the qualifier as a direct table name
    let normalized = strip_identifier_quotes(qualifier);
    if seen.insert(normalized.to_uppercase()) {
        return vec![normalized];
    }

    Vec::new()
}

/// Resolve all table names from scope (for unqualified column suggestions).
pub fn resolve_all_scope_tables(tables_in_scope: &[ScopedTableRef]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for table_ref in tables_in_scope {
        let upper = table_ref.name.to_uppercase();
        if seen.insert(upper) {
            result.push(table_ref.name.clone());
        }
    }

    result
}

/// Extract projected column names from a SELECT statement's token stream.
/// Returns column names/aliases in the order they appear in the SELECT list.
/// Items that cannot be resolved (e.g., `*`, expressions without aliases) are omitted.
pub fn extract_select_list_columns(tokens: &[SqlToken]) -> Vec<String> {
    let mut columns = Vec::new();
    let mut idx = 0;

    // Find SELECT keyword
    while idx < tokens.len() {
        match &tokens[idx] {
            SqlToken::Word(w) if w.eq_ignore_ascii_case("SELECT") => {
                idx += 1;
                break;
            }
            SqlToken::Comment(_) => {
                idx += 1;
                continue;
            }
            _ => {
                idx += 1;
                continue;
            }
        }
    }

    // Skip DISTINCT / ALL / UNIQUE
    while idx < tokens.len() {
        match &tokens[idx] {
            SqlToken::Word(w) => {
                let u = w.to_uppercase();
                if matches!(u.as_str(), "DISTINCT" | "ALL" | "UNIQUE") {
                    idx += 1;
                    continue;
                }
                break;
            }
            SqlToken::Comment(_) => {
                idx += 1;
                continue;
            }
            _ => break,
        }
    }

    // Collect tokens for each SELECT item (delimited by comma at depth 0)
    let mut depth = 0usize;
    let mut item_tokens: Vec<&SqlToken> = Vec::new();

    while idx < tokens.len() {
        let token = &tokens[idx];

        match token {
            SqlToken::Symbol(s) if s == "(" => {
                depth += 1;
                item_tokens.push(token);
            }
            SqlToken::Symbol(s) if s == ")" => {
                depth = depth.saturating_sub(1);
                item_tokens.push(token);
            }
            SqlToken::Symbol(s) if s == "," && depth == 0 => {
                if let Some(col) = resolve_item_column_name(&item_tokens) {
                    columns.push(col);
                }
                item_tokens.clear();
            }
            SqlToken::Word(w) if depth == 0 => {
                let u = w.to_uppercase();
                if matches!(u.as_str(), "FROM" | "INTO" | "BULK") {
                    if let Some(col) = resolve_item_column_name(&item_tokens) {
                        columns.push(col);
                    }
                    item_tokens.clear();
                    break;
                }
                item_tokens.push(token);
            }
            SqlToken::Comment(_) => { /* skip */ }
            _ => {
                item_tokens.push(token);
            }
        }
        idx += 1;
    }

    // Handle last item if we ran out of tokens (incomplete SQL)
    if !item_tokens.is_empty() {
        if let Some(col) = resolve_item_column_name(&item_tokens) {
            columns.push(col);
        }
    }

    columns
}

/// Given the tokens of a single SELECT item, determine the output column name.
fn resolve_item_column_name(item_tokens: &[&SqlToken]) -> Option<String> {
    let meaningful: Vec<&SqlToken> = item_tokens
        .iter()
        .copied()
        .filter(|t| !matches!(t, SqlToken::Comment(_)))
        .collect();

    if meaningful.is_empty() {
        return None;
    }

    // Check for lone `*`
    if meaningful.len() == 1 {
        if let SqlToken::Symbol(s) = meaningful[0] {
            if s == "*" {
                return None;
            }
        }
    }

    // Check for `qualifier.*` pattern
    if meaningful.len() >= 2 {
        if let SqlToken::Symbol(s) = meaningful[meaningful.len() - 1] {
            if s == "*" {
                return None;
            }
        }
    }

    let last = meaningful.last()?;
    let second_last = if meaningful.len() >= 2 {
        Some(meaningful[meaningful.len() - 2])
    } else {
        None
    };

    // Case 1: Explicit alias `... AS alias_name`
    if let SqlToken::Word(alias) = last {
        if let Some(SqlToken::Word(kw)) = second_last {
            if kw.eq_ignore_ascii_case("AS") {
                return Some(alias.clone());
            }
        }
    }

    // Case 2: Implicit alias — last token is a Word following `)` or another Word
    if let SqlToken::Word(alias) = last {
        let alias_upper = alias.to_uppercase();
        if !is_select_item_trailing_keyword(&alias_upper) {
            if let Some(prev) = second_last {
                let is_implicit = match prev {
                    SqlToken::Symbol(s) if s == ")" => true,
                    SqlToken::Word(_) => {
                        // Two consecutive words: the second is an implicit alias
                        // unless the first is AS (already handled above)
                        meaningful.len() > 1
                    }
                    SqlToken::Symbol(s) if s == "." => false, // qualifier.column, not alias
                    _ => false,
                };
                if is_implicit {
                    return Some(alias.clone());
                }
            }
        }
    }

    // Case 3: Simple column reference (single word)
    if meaningful.len() == 1 {
        if let SqlToken::Word(name) = meaningful[0] {
            return Some(name.clone());
        }
    }

    // Case 4: Qualified column `qualifier.column`
    if meaningful.len() == 3 {
        if let (SqlToken::Word(_), SqlToken::Symbol(dot), SqlToken::Word(col)) =
            (meaningful[0], meaningful[1], meaningful[2])
        {
            if dot == "." {
                return Some(col.clone());
            }
        }
    }

    // Expression without alias — cannot determine column name
    None
}

fn is_select_item_trailing_keyword(word: &str) -> bool {
    matches!(
        word,
        "FROM"
            | "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "INTO"
            | "UNION"
            | "INTERSECT"
            | "EXCEPT"
            | "MINUS"
            | "FETCH"
            | "FOR"
            | "LIMIT"
            | "OFFSET"
            | "CONNECT"
            | "START"
            | "BULK"
    )
}

#[cfg(test)]
mod tests;
