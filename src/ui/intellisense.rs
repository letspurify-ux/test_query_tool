use fltk::{
    browser::HoldBrowser,
    prelude::*,
    window::Window,
};
use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use std::rc::Rc;
use crate::ui::theme;

// SQL Keywords for autocomplete
pub const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "BETWEEN", "LIKE", "IS", "NULL",
    "ORDER", "BY", "ASC", "DESC", "GROUP", "HAVING", "JOIN", "INNER", "LEFT", "RIGHT",
    "OUTER", "FULL", "CROSS", "ON", "AS", "DISTINCT", "ALL", "TOP", "LIMIT", "OFFSET",
    "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "INDEX",
    "VIEW", "DROP", "ALTER", "ADD", "COLUMN", "CONSTRAINT", "PRIMARY", "KEY", "FOREIGN",
    "REFERENCES", "UNIQUE", "CHECK", "DEFAULT", "CASCADE", "TRUNCATE", "GRANT", "REVOKE",
    "COMMIT", "ROLLBACK", "SAVEPOINT", "BEGIN", "END", "DECLARE", "CURSOR", "FETCH",
    "CASE", "WHEN", "THEN", "ELSE", "UNION", "INTERSECT", "EXCEPT", "EXISTS", "ANY",
    "SOME", "WITH", "RECURSIVE", "OVER", "PARTITION", "ROW_NUMBER", "RANK", "DENSE_RANK",
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NVL", "DECODE", "TO_CHAR", "TO_DATE",
    "TO_NUMBER", "SYSDATE", "SYSTIMESTAMP", "ROWNUM", "ROWID", "DUAL", "SEQUENCE", "NEXTVAL",
    "CURRVAL", "TRIGGER", "PROCEDURE", "FUNCTION", "PACKAGE", "BODY", "RETURN", "RETURNS",
    "VARCHAR2", "NUMBER", "INTEGER", "DATE", "TIMESTAMP", "CLOB", "BLOB", "BOOLEAN",
];

// Oracle built-in functions
pub const ORACLE_FUNCTIONS: &[&str] = &[
    "ABS", "ACOS", "ADD_MONTHS", "ASCII", "ASIN", "ATAN", "ATAN2",
    "AVG", "BFILENAME", "BIN_TO_NUM", "BITAND", "CARDINALITY", "CAST",
    "CEIL", "CHARTOROWID", "CHR", "COALESCE", "CONCAT", "CONVERT",
    "CORR", "COS", "COSH", "COUNT", "COVAR_POP", "COVAR_SAMP",
    "CUME_DIST", "CURRENT_DATE", "CURRENT_TIMESTAMP", "DBTIMEZONE",
    "DECODE", "DENSE_RANK", "DUMP", "EMPTY_BLOB", "EMPTY_CLOB",
    "EXP", "EXTRACT", "FIRST", "FIRST_VALUE", "FLOOR", "FROM_TZ",
    "GREATEST", "GROUP_ID", "HEXTORAW", "INITCAP", "INSTR", "LAG",
    "LAST", "LAST_DAY", "LAST_VALUE", "LEAD", "LEAST", "LENGTH",
    "LISTAGG", "LN", "LNNVL", "LOCALTIMESTAMP", "LOG", "LOWER",
    "LPAD", "LTRIM", "MAX", "MEDIAN", "MIN", "MOD", "MONTHS_BETWEEN",
    "NANVL", "NEW_TIME", "NEXT_DAY", "NLS_CHARSET_ID", "NLS_INITCAP",
    "NLS_LOWER", "NLS_UPPER", "NLSSORT", "NTILE", "NULLIF", "NVL",
    "NVL2", "PERCENT_RANK", "PERCENTILE_CONT", "PERCENTILE_DISC",
    "POWER", "RANK", "RAWTOHEX", "REGEXP_COUNT", "REGEXP_INSTR",
    "REGEXP_REPLACE", "REGEXP_SUBSTR", "REMAINDER", "REPLACE",
    "ROUND", "ROW_NUMBER", "ROWIDTOCHAR", "RPAD", "RTRIM",
    "SESSIONTIMEZONE", "SIGN", "SIN", "SINH", "SOUNDEX", "SQRT",
    "STDDEV", "STDDEV_POP", "STDDEV_SAMP", "SUBSTR", "SUM",
    "SYS_CONTEXT", "SYSDATE", "SYSTIMESTAMP", "TAN", "TANH",
    "TO_CHAR", "TO_CLOB", "TO_DATE", "TO_DSINTERVAL", "TO_LOB",
    "TO_MULTI_BYTE", "TO_NUMBER", "TO_SINGLE_BYTE", "TO_TIMESTAMP",
    "TO_TIMESTAMP_TZ", "TO_YMINTERVAL", "TRANSLATE", "TRIM", "TRUNC",
    "TZ_OFFSET", "UID", "UPPER", "USER", "USERENV", "VAR_POP",
    "VAR_SAMP", "VARIANCE", "VSIZE", "WIDTH_BUCKET", "XMLAGG",
];

const MAX_SUGGESTIONS: usize = 50;

#[derive(Clone)]
struct NameEntry {
    name: String,
    upper: String,
}

impl NameEntry {
    fn new(name: String) -> Self {
        let upper = name.to_uppercase();
        Self { name, upper }
    }
}

#[derive(Clone)]
pub struct IntellisenseData {
    pub tables: Vec<String>,
    pub columns: HashMap<String, Vec<String>>, // table_name -> column_names
    pub columns_loading: HashSet<String>,
    pub views: Vec<String>,
    pub procedures: Vec<String>,
    pub functions: Vec<String>,
    table_entries: Vec<NameEntry>,
    view_entries: Vec<NameEntry>,
    procedure_entries: Vec<NameEntry>,
    function_entries: Vec<NameEntry>,
    column_entries_by_table: HashMap<String, Vec<NameEntry>>,
    all_columns_entries: Vec<NameEntry>,
    all_columns_dirty: bool,
    relations_upper: HashSet<String>,
}

impl IntellisenseData {
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            columns: HashMap::new(),
            columns_loading: HashSet::new(),
            views: Vec::new(),
            procedures: Vec::new(),
            functions: Vec::new(),
            table_entries: Vec::new(),
            view_entries: Vec::new(),
            procedure_entries: Vec::new(),
            function_entries: Vec::new(),
            column_entries_by_table: HashMap::new(),
            all_columns_entries: Vec::new(),
            all_columns_dirty: false,
            relations_upper: HashSet::new(),
        }
    }

    pub fn get_suggestions(
        &mut self,
        prefix: &str,
        include_columns: bool,
        column_tables: Option<&[String]>,
    ) -> Vec<String> {
        self.ensure_base_indices();

        let prefix_upper = prefix.to_uppercase();
        let mut suggestions = Vec::new();
        let mut seen = HashSet::new();

        let push_suggestion = |value: String,
                               suggestions: &mut Vec<String>,
                               seen: &mut HashSet<String>| {
            if suggestions.len() >= MAX_SUGGESTIONS {
                return true;
            }
            if seen.insert(value.clone()) {
                suggestions.push(value);
            }
            suggestions.len() >= MAX_SUGGESTIONS
        };

        // Add SQL keywords
        for keyword in SQL_KEYWORDS {
            if keyword.starts_with(&prefix_upper) {
                if push_suggestion(keyword.to_string(), &mut suggestions, &mut seen) {
                    break;
                }
            }
        }

        // Add Oracle functions
        for func in ORACLE_FUNCTIONS {
            if func.starts_with(&prefix_upper) {
                if push_suggestion(format!("{}()", func), &mut suggestions, &mut seen) {
                    break;
                }
            }
        }

        // Add tables
        if Self::push_entries(
            &self.table_entries,
            &prefix_upper,
            &mut suggestions,
            &mut seen,
        ) {
            suggestions.sort_unstable();
            suggestions.dedup();
            return suggestions;
        }

        // Add views
        if Self::push_entries(
            &self.view_entries,
            &prefix_upper,
            &mut suggestions,
            &mut seen,
        ) {
            suggestions.sort_unstable();
            suggestions.dedup();
            return suggestions;
        }

        // Add procedures
        if Self::push_entries(
            &self.procedure_entries,
            &prefix_upper,
            &mut suggestions,
            &mut seen,
        ) {
            suggestions.sort_unstable();
            suggestions.dedup();
            return suggestions;
        }

        // Add functions
        if Self::push_entries(
            &self.function_entries,
            &prefix_upper,
            &mut suggestions,
            &mut seen,
        ) {
            suggestions.sort_unstable();
            suggestions.dedup();
            return suggestions;
        }

        if include_columns {
            match column_tables {
                Some(tables) if !tables.is_empty() => {
                    for table in tables {
                        let key = table.to_uppercase();
                        if let Some(cols) = self.column_entries_by_table.get(&key) {
                            if Self::push_entries(
                                cols,
                                &prefix_upper,
                                &mut suggestions,
                                &mut seen,
                            ) {
                                break;
                            }
                        }
                    }
                }
                _ => {
                    if !prefix_upper.is_empty() {
                        self.ensure_all_columns_entries();
                        let _ = Self::push_entries(
                            &self.all_columns_entries,
                            &prefix_upper,
                            &mut suggestions,
                            &mut seen,
                        );
                    }
                }
            }
        }

        suggestions.sort_unstable();
        suggestions.dedup();
        suggestions.truncate(MAX_SUGGESTIONS);
        suggestions
    }

    pub fn get_column_suggestions(
        &mut self,
        prefix: &str,
        column_tables: Option<&[String]>,
    ) -> Vec<String> {
        self.ensure_base_indices();

        let prefix_upper = prefix.to_uppercase();
        let mut suggestions = Vec::new();
        let mut seen = HashSet::new();

        match column_tables {
            Some(tables) if !tables.is_empty() => {
                for table in tables {
                    let key = table.to_uppercase();
                    if let Some(cols) = self.column_entries_by_table.get(&key) {
                        if Self::push_entries(
                            cols,
                            &prefix_upper,
                            &mut suggestions,
                            &mut seen,
                        ) {
                            break;
                        }
                    }
                }
            }
            _ => {
                self.ensure_all_columns_entries();
                let _ = Self::push_entries(
                    &self.all_columns_entries,
                    &prefix_upper,
                    &mut suggestions,
                    &mut seen,
                );
            }
        }

        suggestions.sort_unstable();
        suggestions.dedup();
        suggestions.truncate(MAX_SUGGESTIONS);
        suggestions
    }

    #[allow(dead_code)]
    pub fn get_columns_for_table(&self, table_name: &str) -> Vec<String> {
        let key = table_name.to_uppercase();
        self.columns.get(&key).cloned().unwrap_or_default()
    }

    pub fn set_columns_for_table(&mut self, table_name: &str, columns: Vec<String>) {
        let key = table_name.to_uppercase();
        self.columns_loading.remove(&key);
        self.columns.insert(key.clone(), columns.clone());
        self.column_entries_by_table
            .insert(key, Self::build_entries(&columns));
        self.all_columns_dirty = true;
    }

    pub fn mark_columns_loading(&mut self, table_name: &str) -> bool {
        let key = table_name.to_uppercase();
        if self.columns.contains_key(&key) || self.columns_loading.contains(&key) {
            return false;
        }
        self.columns_loading.insert(key);
        true
    }

    pub fn is_known_relation(&self, name: &str) -> bool {
        let upper = name.to_uppercase();
        if !self.relations_upper.is_empty() {
            return self.relations_upper.contains(&upper);
        }
        self.tables.iter().any(|t| t.to_uppercase() == upper)
            || self.views.iter().any(|v| v.to_uppercase() == upper)
    }

    pub fn rebuild_indices(&mut self) {
        self.table_entries = Self::build_entries(&self.tables);
        self.view_entries = Self::build_entries(&self.views);
        self.procedure_entries = Self::build_entries(&self.procedures);
        self.function_entries = Self::build_entries(&self.functions);
        self.relations_upper = self
            .tables
            .iter()
            .chain(self.views.iter())
            .map(|name| name.to_uppercase())
            .collect();
        self.column_entries_by_table.clear();
        for (table, columns) in &self.columns {
            self.column_entries_by_table
                .insert(table.clone(), Self::build_entries(columns));
        }
        self.all_columns_entries.clear();
        self.all_columns_dirty = true;
    }

    fn ensure_base_indices(&mut self) {
        if self.table_entries.len() != self.tables.len()
            || self.view_entries.len() != self.views.len()
            || self.procedure_entries.len() != self.procedures.len()
            || self.function_entries.len() != self.functions.len()
        {
            self.rebuild_indices();
        }
    }

    fn ensure_all_columns_entries(&mut self) {
        if !self.all_columns_dirty {
            return;
        }
        let mut all = Vec::new();
        for entries in self.column_entries_by_table.values() {
            all.extend(entries.iter().cloned());
        }
        all.sort_by(|a, b| a.upper.cmp(&b.upper).then_with(|| a.name.cmp(&b.name)));
        all.dedup_by(|a, b| a.upper == b.upper && a.name == b.name);
        self.all_columns_entries = all;
        self.all_columns_dirty = false;
    }

    fn build_entries(names: &[String]) -> Vec<NameEntry> {
        let mut entries: Vec<NameEntry> = names
            .iter()
            .cloned()
            .map(NameEntry::new)
            .collect();
        entries.sort_by(|a, b| a.upper.cmp(&b.upper).then_with(|| a.name.cmp(&b.name)));
        entries
    }

    fn push_entries(
        entries: &[NameEntry],
        prefix_upper: &str,
        suggestions: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) -> bool {
        if suggestions.len() >= MAX_SUGGESTIONS || entries.is_empty() {
            return suggestions.len() >= MAX_SUGGESTIONS;
        }
        let start = entries.partition_point(|entry| entry.upper.as_str() < prefix_upper);
        for entry in entries.iter().skip(start) {
            if !entry.upper.starts_with(prefix_upper) {
                break;
            }
            if seen.insert(entry.name.clone()) {
                suggestions.push(entry.name.clone());
                if suggestions.len() >= MAX_SUGGESTIONS {
                    return true;
                }
            }
        }
        suggestions.len() >= MAX_SUGGESTIONS
    }
}

impl Default for IntellisenseData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct IntellisensePopup {
    window: Window,
    browser: HoldBrowser,
    suggestions: Rc<RefCell<Vec<String>>>,
    selected_callback: Rc<RefCell<Option<Box<dyn FnMut(String)>>>>,
    visible: Rc<RefCell<bool>>,
}

impl IntellisensePopup {
    pub fn new() -> Self {
        // Temporarily suspend current group to prevent popup window from being
        // added to the parent container (which causes layout issues)
        let current_group = fltk::group::Group::try_current();
        
        fltk::group::Group::set_current(None::<&fltk::group::Group>);

        let mut window = Window::default()
            .with_size(320, 200);
        window.set_border(false);
        window.set_color(theme::panel_raised());
        window.make_modal(false);

        let mut browser = HoldBrowser::default()
            .with_size(320, 200)
            .with_pos(0, 0);
        browser.set_color(theme::panel_alt());
        browser.set_selection_color(theme::selection_strong());

        window.end();

        // Restore current group
        if let Some(ref group) = current_group {
            fltk::group::Group::set_current(Some(group));
        }

        let suggestions = Rc::new(RefCell::new(Vec::new()));
        let selected_callback: Rc<RefCell<Option<Box<dyn FnMut(String)>>>> =
            Rc::new(RefCell::new(None));
        let visible = Rc::new(RefCell::new(false));

        window.hide();

        let mut popup = Self {
            window,
            browser,
            suggestions,
            selected_callback,
            visible,
        };

        popup.setup_callbacks();
        popup
    }

    fn setup_callbacks(&mut self) {
        // Browser click callback - handle mouse selection
        let suggestions = self.suggestions.clone();
        let callback = self.selected_callback.clone();
        let mut window = self.window.clone();
        let visible = self.visible.clone();

        self.browser.set_callback(move |b| {
            let selected = b.value();
            if selected > 0 {
                // First, get the text with suggestions borrow, then release it
                let text = {
                    let suggestions = suggestions.borrow();
                    suggestions.get((selected - 1) as usize).cloned()
                };
                if let Some(text) = text {
                    // Take the callback out, call it, then put it back
                    // This ensures the RefCell is not borrowed during callback execution
                    let cb_opt = callback.borrow_mut().take();
                    if let Some(mut cb) = cb_opt {
                        cb(text);
                        *callback.borrow_mut() = Some(cb);
                    }
                    window.hide();
                    *visible.borrow_mut() = false;
                }
            }
        });

        // Note: Keyboard events are handled by the editor, not by this popup window.
        // This is because the editor retains focus while the popup is visible,
        // so key events go to the editor's handle(), not the popup's handle().
    }

    pub fn show_suggestions(&mut self, suggestions: Vec<String>, x: i32, y: i32) {
        if suggestions.is_empty() {
            self.hide();
            return;
        }

        self.browser.clear();
        *self.suggestions.borrow_mut() = suggestions.clone();

        for suggestion in &suggestions {
            // Add with color formatting for dark theme
            self.browser.add(&format!("@C255 {}", suggestion));
        }

        // Select first item
        if !suggestions.is_empty() {
            self.browser.select(1);
        }

        // Calculate popup size
        let height = (suggestions.len().min(10) * 20 + 10) as i32;
        self.window.set_size(320, height);
        self.browser.set_size(320, height);

        self.window.set_pos(x, y);
        self.window.show();
        self.window.set_on_top();
        *self.visible.borrow_mut() = true;
    }

    pub fn hide(&mut self) {
        self.window.hide();
        *self.visible.borrow_mut() = false;
    }

    pub fn is_visible(&self) -> bool {
        *self.visible.borrow()
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let left = self.window.x();
        let top = self.window.y();
        let right = left + self.window.w();
        let bottom = top + self.window.h();
        x >= left && x < right && y >= top && y < bottom
    }

    pub fn set_selected_callback<F>(&mut self, callback: F)
    where
        F: FnMut(String) + 'static,
    {
        *self.selected_callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn select_next(&mut self) {
        let current = self.browser.value();
        let count = self.browser.size();
        if current < count {
            self.browser.select(current + 1);
        }
    }

    pub fn select_prev(&mut self) {
        let current = self.browser.value();
        if current > 1 {
            self.browser.select(current - 1);
        }
    }

    pub fn get_selected(&self) -> Option<String> {
        let selected = self.browser.value();
        if selected > 0 {
            self.suggestions.borrow().get((selected - 1) as usize).cloned()
        } else {
            None
        }
    }
}

impl Default for IntellisensePopup {
    fn default() -> Self {
        Self::new()
    }
}

// Helper function to extract the current word at cursor position (ASCII-based).
// cursor_pos is a byte offset from FLTK TextBuffer.
pub fn get_word_at_cursor(text: &str, cursor_pos: usize) -> (String, usize, usize) {
    if text.is_empty() || cursor_pos == 0 {
        return (String::new(), 0, 0);
    }

    let bytes = text.as_bytes();
    let pos = cursor_pos.min(bytes.len());

    // Find word start by scanning backwards over ASCII identifier bytes
    let mut start = pos;
    while start > 0 {
        if let Some(&byte) = bytes.get(start - 1) {
            if is_identifier_byte(byte) {
                start -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Find word end by scanning forwards over ASCII identifier bytes
    let mut end = pos;
    while let Some(&byte) = bytes.get(end) {
        if is_identifier_byte(byte) {
            end += 1;
        } else {
            break;
        }
    }

    // Since we only matched ASCII bytes, start..end is always valid UTF-8
    let word = bytes
        .get(start..pos)
        .map(|slice| String::from_utf8_lossy(slice).into_owned())
        .unwrap_or_default();
    (word, start, end)
}

// Detect context for smarter suggestions (after FROM, after SELECT, etc.)
#[allow(dead_code)]
pub fn detect_sql_context(text: &str, cursor_pos: usize) -> SqlContext {
    let end = cursor_pos.min(text.len());
    let upper = String::from_utf8_lossy(&text.as_bytes()[..end]).to_ascii_uppercase();

    // Simple context detection
    let words: Vec<&str> = upper.split_whitespace().collect();

    if let Some(last_keyword) = words.iter().rev().find(|w| {
        matches!(
            w.as_ref(),
            "FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "SELECT" | "WHERE" | "AND" | "OR" | "SET"
        )
    }) {
        match *last_keyword {
            "FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" => SqlContext::TableName,
            "SELECT" => SqlContext::ColumnOrAll,
            "WHERE" | "AND" | "OR" | "SET" => SqlContext::ColumnName,
            _ => SqlContext::General,
        }
    } else {
        SqlContext::General
    }
}

// ASCII-based identifier check.
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum SqlContext {
    General,
    TableName,
    ColumnName,
    ColumnOrAll,
}
