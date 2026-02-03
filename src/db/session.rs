use oracle::sql_type::OracleType;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum BindDataType {
    Number,
    Varchar2(u32),
    Date,
    Timestamp(u8),
    RefCursor,
    Clob,
}

#[derive(Debug, Clone)]
pub enum BindValue {
    Scalar(Option<String>),
    Cursor(Option<CursorResult>),
}

#[derive(Debug, Clone)]
pub struct BindVar {
    pub data_type: BindDataType,
    pub value: BindValue,
}

#[derive(Debug, Clone)]
pub struct CursorResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ServerOutputConfig {
    pub enabled: bool,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct CompiledObject {
    pub owner: Option<String>,
    pub object_type: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub binds: HashMap<String, BindVar>,
    pub define_vars: HashMap<String, String>,
    pub server_output: ServerOutputConfig,
    pub last_compiled: Option<CompiledObject>,
    pub continue_on_error: bool,
    pub define_enabled: bool,
    pub scan_enabled: bool,
    pub verify_enabled: bool,
    pub echo_enabled: bool,
    pub feedback_enabled: bool,
    pub heading_enabled: bool,
    pub pagesize: u32,
    pub linesize: u32,
    pub spool_path: Option<PathBuf>,
    pub spool_truncate: bool,
}

impl Default for ServerOutputConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 1_000_000,
        }
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            binds: HashMap::new(),
            define_vars: HashMap::new(),
            server_output: ServerOutputConfig::default(),
            last_compiled: None,
            continue_on_error: false,
            define_enabled: true,
            scan_enabled: true,
            verify_enabled: false,
            echo_enabled: false,
            feedback_enabled: true,
            heading_enabled: true,
            pagesize: 14,
            linesize: 80,
            spool_path: None,
            spool_truncate: false,
        }
    }
}

impl BindDataType {
    pub fn oracle_type(&self) -> OracleType {
        match self {
            BindDataType::Number => OracleType::Number(0, 0),
            BindDataType::Varchar2(size) => OracleType::Varchar2(*size),
            BindDataType::Date => OracleType::Date,
            BindDataType::Timestamp(precision) => OracleType::Timestamp(*precision),
            BindDataType::RefCursor => OracleType::RefCursor,
            BindDataType::Clob => OracleType::CLOB,
        }
    }

    pub fn display(&self) -> String {
        match self {
            BindDataType::Number => "NUMBER".to_string(),
            BindDataType::Varchar2(size) => format!("VARCHAR2({})", size),
            BindDataType::Date => "DATE".to_string(),
            BindDataType::Timestamp(precision) => format!("TIMESTAMP({})", precision),
            BindDataType::RefCursor => "REFCURSOR".to_string(),
            BindDataType::Clob => "CLOB".to_string(),
        }
    }
}

impl BindVar {
    pub fn new(data_type: BindDataType) -> Self {
        let value = match data_type {
            BindDataType::RefCursor => BindValue::Cursor(None),
            _ => BindValue::Scalar(None),
        };
        Self { data_type, value }
    }
}

impl SessionState {
    pub fn normalize_name(name: &str) -> String {
        name.trim().trim_start_matches(':').to_uppercase()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
