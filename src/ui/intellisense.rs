use fltk::{
    browser::HoldBrowser,
    enums::{Color, Event, Key},
    prelude::*,
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;

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

#[derive(Clone)]
pub struct IntellisenseData {
    pub tables: Vec<String>,
    pub columns: Vec<(String, Vec<String>)>, // (table_name, [column_names])
    pub views: Vec<String>,
    pub procedures: Vec<String>,
    pub functions: Vec<String>,
}

impl IntellisenseData {
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            columns: Vec::new(),
            views: Vec::new(),
            procedures: Vec::new(),
            functions: Vec::new(),
        }
    }

    pub fn get_all_suggestions(&self, prefix: &str) -> Vec<String> {
        let prefix_upper = prefix.to_uppercase();
        let mut suggestions = Vec::new();

        // Add SQL keywords
        for keyword in SQL_KEYWORDS {
            if keyword.starts_with(&prefix_upper) {
                suggestions.push(keyword.to_string());
            }
        }

        // Add Oracle functions
        for func in ORACLE_FUNCTIONS {
            if func.starts_with(&prefix_upper) {
                suggestions.push(format!("{}()", func));
            }
        }

        // Add tables
        for table in &self.tables {
            if table.to_uppercase().starts_with(&prefix_upper) {
                suggestions.push(table.clone());
            }
        }

        // Add views
        for view in &self.views {
            if view.to_uppercase().starts_with(&prefix_upper) {
                suggestions.push(view.clone());
            }
        }

        // Add procedures
        for proc in &self.procedures {
            if proc.to_uppercase().starts_with(&prefix_upper) {
                suggestions.push(proc.clone());
            }
        }

        // Add functions
        for func in &self.functions {
            if func.to_uppercase().starts_with(&prefix_upper) {
                suggestions.push(func.clone());
            }
        }

        // Add all columns
        for (_, cols) in &self.columns {
            for col in cols {
                if col.to_uppercase().starts_with(&prefix_upper) {
                    suggestions.push(col.clone());
                }
            }
        }

        suggestions.sort();
        suggestions.dedup();
        suggestions.truncate(50); // Limit to 50 suggestions
        suggestions
    }

    #[allow(dead_code)]
    pub fn get_columns_for_table(&self, table_name: &str) -> Vec<String> {
        let table_upper = table_name.to_uppercase();
        for (name, cols) in &self.columns {
            if name.to_uppercase() == table_upper {
                return cols.clone();
            }
        }
        Vec::new()
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
        let mut window = Window::default()
            .with_size(320, 200);
        window.set_border(false);
        window.set_color(Color::from_rgb(45, 45, 48)); // Modern popup background

        let mut browser = HoldBrowser::default()
            .with_size(320, 200)
            .with_pos(0, 0);
        browser.set_color(Color::from_rgb(45, 45, 48)); // Match popup
        browser.set_selection_color(Color::from_rgb(0, 120, 212)); // Modern accent

        window.end();

        let suggestions = Rc::new(RefCell::new(Vec::new()));
        let selected_callback: Rc<RefCell<Option<Box<dyn FnMut(String)>>>> =
            Rc::new(RefCell::new(None));
        let visible = Rc::new(RefCell::new(false));

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
        let suggestions = self.suggestions.clone();
        let callback = self.selected_callback.clone();
        let mut window = self.window.clone();
        let visible = self.visible.clone();

        self.browser.set_callback(move |b| {
            let selected = b.value();
            if selected > 0 {
                let suggestions = suggestions.borrow();
                if let Some(text) = suggestions.get((selected - 1) as usize) {
                    if let Some(ref mut cb) = *callback.borrow_mut() {
                        cb(text.clone());
                    }
                    window.hide();
                    *visible.borrow_mut() = false;
                }
            }
        });

        // Handle keyboard events
        let suggestions = self.suggestions.clone();
        let callback = self.selected_callback.clone();
        let mut window = self.window.clone();
        let mut browser = self.browser.clone();
        let visible = self.visible.clone();

        self.window.handle(move |_, ev| {
            match ev {
                Event::KeyDown => {
                    let key = fltk::app::event_key();
                    match key {
                        Key::Escape => {
                            window.hide();
                            *visible.borrow_mut() = false;
                            true
                        }
                        Key::Enter => {
                            let selected = browser.value();
                            if selected > 0 {
                                let suggestions = suggestions.borrow();
                                if let Some(text) = suggestions.get((selected - 1) as usize) {
                                    if let Some(ref mut cb) = *callback.borrow_mut() {
                                        cb(text.clone());
                                    }
                                }
                            }
                            window.hide();
                            *visible.borrow_mut() = false;
                            true
                        }
                        Key::Up => {
                            let current = browser.value();
                            if current > 1 {
                                browser.select(current - 1);
                            }
                            true
                        }
                        Key::Down => {
                            let current = browser.value();
                            let count = browser.size();
                            if current < count {
                                browser.select(current + 1);
                            }
                            true
                        }
                        _ => false,
                    }
                }
                Event::Unfocus => {
                    // Hide when losing focus
                    window.hide();
                    *visible.borrow_mut() = false;
                    true
                }
                _ => false,
            }
        });
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
        *self.visible.borrow_mut() = true;
    }

    pub fn hide(&mut self) {
        self.window.hide();
        *self.visible.borrow_mut() = false;
    }

    pub fn is_visible(&self) -> bool {
        *self.visible.borrow()
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

// Helper function to extract the current word at cursor position
pub fn get_word_at_cursor(text: &str, cursor_pos: usize) -> (String, usize, usize) {
    if text.is_empty() || cursor_pos == 0 {
        return (String::new(), 0, 0);
    }

    let chars: Vec<char> = text.chars().collect();
    let pos = cursor_pos.min(chars.len());

    // Find word start
    let mut start = pos;
    while start > 0 {
        let ch = chars[start - 1];
        if !ch.is_alphanumeric() && ch != '_' {
            break;
        }
        start -= 1;
    }

    // Find word end
    let mut end = pos;
    while end < chars.len() {
        let ch = chars[end];
        if !ch.is_alphanumeric() && ch != '_' {
            break;
        }
        end += 1;
    }

    let word: String = chars[start..pos].iter().collect();
    (word, start, end)
}

// Detect context for smarter suggestions (after FROM, after SELECT, etc.)
#[allow(dead_code)]
pub fn detect_sql_context(text: &str, cursor_pos: usize) -> SqlContext {
    let text_before_cursor: String = text.chars().take(cursor_pos).collect();
    let upper = text_before_cursor.to_uppercase();

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

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum SqlContext {
    General,
    TableName,
    ColumnName,
    ColumnOrAll,
}
