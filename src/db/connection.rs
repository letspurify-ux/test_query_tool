use oracle::{Connection, Error as OracleError};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::db::session::SessionState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub username: String,
    #[serde(skip_serializing, default)]
    pub password: String,
    pub host: String,
    pub port: u16,
    pub service_name: String,
}

impl ConnectionInfo {
    pub fn new(
        name: &str,
        username: &str,
        password: &str,
        host: &str,
        port: u16,
        service_name: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            host: host.to_string(),
            port,
            service_name: service_name.to_string(),
        }
    }

    pub fn connection_string(&self) -> String {
        format!("//{}:{}/{}", self.host, self.port, self.service_name)
    }

    pub fn display_string(&self) -> String {
        format!(
            "{} ({}@@{}:{}/{})",
            self.name, self.username, self.host, self.port, self.service_name
        )
    }

    /// Securely clear the password from memory by overwriting with zeros
    /// then releasing the allocation.
    pub fn clear_password(&mut self) {
        // Overwrite the existing bytes with zeros before dropping
        // SAFETY: we write zeros over the valid UTF-8 bytes (zeros are valid UTF-8)
        let bytes = unsafe { self.password.as_bytes_mut() };
        for b in bytes.iter_mut() {
            // Use write_volatile to prevent the compiler from optimizing away the zeroing
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        self.password.clear();
        self.password.shrink_to_fit();
    }
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            username: String::new(),
            password: String::new(),
            host: "localhost".to_string(),
            port: 1521,
            service_name: "ORCL".to_string(),
        }
    }
}

pub struct DatabaseConnection {
    connection: Option<Arc<Connection>>,
    info: ConnectionInfo,
    connected: bool,
    auto_commit: bool,
    session: Arc<Mutex<SessionState>>,
}

impl DatabaseConnection {
    pub fn new() -> Self {
        Self {
            connection: None,
            info: ConnectionInfo::default(),
            connected: false,
            auto_commit: false,
            session: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    pub fn connect(&mut self, info: ConnectionInfo) -> Result<(), OracleError> {
        let conn_str = info.connection_string();
        let connection = Arc::new(
            match Connection::connect(&info.username, &info.password, &conn_str) {
                Ok(connection) => connection,
                Err(err) => {
                    eprintln!("Connection error: {err}");
                    return Err(err);
                }
            },
        );

        Self::apply_default_session_settings(connection.as_ref());

        self.connection = Some(connection);
        self.info = info;
        // Clear password from memory now that the connection is established
        self.info.clear_password();
        self.connected = true;

        Ok(())
    }

    fn apply_default_session_settings(conn: &Connection) {
        let statements = [
            "ALTER SESSION SET NLS_TIMESTAMP_FORMAT = 'yyyy-mm-dd hh24:mi:ss'",
            "ALTER SESSION SET NLS_DATE_FORMAT = 'yyyy-mm-dd hh24:mi:ss'",
        ];

        for statement in statements {
            if let Err(err) = conn.execute(statement, &[]) {
                eprintln!("Warning: failed to apply default session setting `{statement}`: {err}");
            }
        }
    }

    pub fn disconnect(&mut self) {
        self.connection = None;
        self.connected = false;
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn get_connection(&self) -> Option<Arc<Connection>> {
        self.connection.clone()
    }

    pub fn get_info(&self) -> &ConnectionInfo {
        &self.info
    }

    pub fn set_auto_commit(&mut self, enabled: bool) {
        self.auto_commit = enabled;
    }

    pub fn auto_commit(&self) -> bool {
        self.auto_commit
    }

    pub fn session_state(&self) -> Arc<Mutex<SessionState>> {
        Arc::clone(&self.session)
    }

    pub fn test_connection(info: &ConnectionInfo) -> Result<(), OracleError> {
        let conn_str = info.connection_string();
        match Connection::connect(&info.username, &info.password, &conn_str) {
            Ok(_connection) => {}
            Err(err) => {
                eprintln!("Connection error: {err}");
                return Err(err);
            }
        }
        Ok(())
    }
}

impl Default for DatabaseConnection {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedConnection = Arc<Mutex<DatabaseConnection>>;

pub fn create_shared_connection() -> SharedConnection {
    Arc::new(Mutex::new(DatabaseConnection::new()))
}

pub fn lock_connection(connection: &SharedConnection) -> MutexGuard<'_, DatabaseConnection> {
    match connection.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Warning: database connection lock was poisoned; recovering.");
            poisoned.into_inner()
        }
    }
}

/// Try to acquire the connection lock without blocking.
/// Returns None if the lock is already held (query is running).
pub fn try_lock_connection(
    connection: &SharedConnection,
) -> Option<MutexGuard<'_, DatabaseConnection>> {
    match connection.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::WouldBlock) => None,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => {
            eprintln!("Warning: database connection lock was poisoned; recovering.");
            Some(poisoned.into_inner())
        }
    }
}
