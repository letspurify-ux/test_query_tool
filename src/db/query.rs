use oracle::{Connection, Error as OracleError, Row};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub execution_time: Duration,
    pub message: String,
    pub is_select: bool,
}

impl QueryResult {
    pub fn new_select(
        columns: Vec<ColumnInfo>,
        rows: Vec<Vec<String>>,
        execution_time: Duration,
    ) -> Self {
        let row_count = rows.len();
        Self {
            columns,
            rows,
            row_count,
            execution_time,
            message: format!("{} rows fetched", row_count),
            is_select: true,
        }
    }

    pub fn new_dml(affected_rows: u64, execution_time: Duration, statement_type: &str) -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            row_count: affected_rows as usize,
            execution_time,
            message: format!("{} {} row(s) affected", statement_type, affected_rows),
            is_select: false,
        }
    }

    pub fn new_error(error: &str) -> Self {
        Self {
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
    pub fn execute(conn: &Connection, sql: &str) -> Result<QueryResult, OracleError> {
        let sql_trimmed = sql.trim();
        let sql_upper = sql_trimmed.to_uppercase();

        let start = Instant::now();

        if sql_upper.starts_with("SELECT") || sql_upper.starts_with("WITH") {
            Self::execute_select(conn, sql_trimmed, start)
        } else if sql_upper.starts_with("INSERT") {
            Self::execute_dml(conn, sql_trimmed, start, "INSERT")
        } else if sql_upper.starts_with("UPDATE") {
            Self::execute_dml(conn, sql_trimmed, start, "UPDATE")
        } else if sql_upper.starts_with("DELETE") {
            Self::execute_dml(conn, sql_trimmed, start, "DELETE")
        } else {
            Self::execute_ddl(conn, sql_trimmed, start)
        }
    }

    fn execute_select(
        conn: &Connection,
        sql: &str,
        start: Instant,
    ) -> Result<QueryResult, OracleError> {
        let mut stmt = conn.statement(sql).build()?;
        let result_set = stmt.query(&[])?;

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
            let row: Row = row_result?;
            let mut row_data: Vec<String> = Vec::new();

            for i in 0..column_info.len() {
                let value: Option<String> = row.get(i).unwrap_or(None);
                row_data.push(value.unwrap_or_else(|| "NULL".to_string()));
            }

            rows.push(row_data);
        }

        let execution_time = start.elapsed();
        Ok(QueryResult::new_select(column_info, rows, execution_time))
    }

    fn execute_dml(
        conn: &Connection,
        sql: &str,
        start: Instant,
        statement_type: &str,
    ) -> Result<QueryResult, OracleError> {
        let stmt = conn.execute(sql, &[])?;
        let affected_rows = stmt.row_count()?;
        let execution_time = start.elapsed();
        Ok(QueryResult::new_dml(
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
        conn.execute(sql, &[])?;
        let execution_time = start.elapsed();
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            row_count: 0,
            execution_time,
            message: "Statement executed successfully".to_string(),
            is_select: false,
        })
    }

    pub fn get_explain_plan(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let explain_sql = format!("EXPLAIN PLAN FOR {}", sql);
        conn.execute(&explain_sql, &[])?;

        let plan_sql =
            "SELECT plan_table_output FROM TABLE(DBMS_XPLAN.DISPLAY('PLAN_TABLE', NULL, 'ALL'))";
        let mut stmt = conn.statement(plan_sql).build()?;
        let rows = stmt.query(&[])?;

        let mut plan_lines: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = row_result?;
            let line: Option<String> = row.get(0)?;
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

    pub fn get_table_columns(
        conn: &Connection,
        table_name: &str,
    ) -> Result<Vec<ColumnInfo>, OracleError> {
        let sql = "SELECT column_name, data_type FROM user_tab_columns WHERE table_name = :1 ORDER BY column_id";
        let mut stmt = conn.statement(sql).build()?;
        let rows = stmt.query(&[&table_name.to_uppercase()])?;

        let mut columns: Vec<ColumnInfo> = Vec::new();
        for row_result in rows {
            let row: Row = row_result?;
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            columns.push(ColumnInfo { name, data_type });
        }

        Ok(columns)
    }

    fn get_object_list(conn: &Connection, sql: &str) -> Result<Vec<String>, OracleError> {
        let mut stmt = conn.statement(sql).build()?;
        let rows = stmt.query(&[])?;

        let mut objects: Vec<String> = Vec::new();
        for row_result in rows {
            let row: Row = row_result?;
            let name: String = row.get(0)?;
            objects.push(name);
        }

        Ok(objects)
    }
}
