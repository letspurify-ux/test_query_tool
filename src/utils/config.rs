use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::db::ConnectionInfo;
use crate::utils::credential_store;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub recent_connections: Vec<ConnectionInfo>,
    pub last_connection: Option<String>,
    pub editor_font: String,
    pub editor_font_size: u32,
    pub result_font: String,
    pub result_font_size: u32,
    pub max_rows: u32,
    pub auto_commit: bool,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            recent_connections: Vec::new(),
            last_connection: None,
            editor_font: "Courier".to_string(),
            editor_font_size: 14,
            result_font: "Helvetica".to_string(),
            result_font_size: 14,
            max_rows: 1000,
            auto_commit: false,
        }
    }

    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut path| {
            path.push("oracle_query_tool");
            path.push("config.json");
            path
        })
    }

    pub fn load() -> Self {
        let mut config = if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    serde_json::from_str(&content).unwrap_or_else(|_| Self::new())
                } else {
                    Self::new()
                }
            } else {
                Self::new()
            }
        } else {
            Self::new()
        };

        // Migrate plain-text passwords from old config to keyring,
        // then load passwords from keyring for all connections.
        let mut needs_resave = false;
        for conn in &mut config.recent_connections {
            // Migration: if password was deserialized from old config, store it in keyring
            if !conn.password.is_empty() {
                if let Err(e) = credential_store::store_password(&conn.name, &conn.password) {
                    eprintln!("Keyring migration warning: {}", e);
                }
                // Clear from memory; will be reloaded from keyring below
                conn.password.clear();
                needs_resave = true;
            }

            // Load password from keyring
            match credential_store::get_password(&conn.name) {
                Ok(Some(password)) => conn.password = password,
                Ok(None) => {} // No stored password
                Err(e) => eprintln!("Keyring load warning: {}", e),
            }
        }

        // Re-save to strip plain-text passwords from config.json
        if needs_resave {
            if let Err(e) = config.save() {
                eprintln!("Failed to re-save config after keyring migration: {}", e);
            }
        }

        config
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                match fs::create_dir_all(parent) {
                    Ok(()) => {}
                    Err(err) => {
                        eprintln!("Config persistence error: {err}");
                        return Err(Box::new(err));
                    }
                }
            }
            let content = match serde_json::to_string_pretty(self) {
                Ok(content) => content,
                Err(err) => {
                    eprintln!("Config persistence error: {err}");
                    return Err(Box::new(err));
                }
            };
            match fs::write(&path, content) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("Config persistence error: {err}");
                    return Err(Box::new(err));
                }
            }

            // Restrict file permissions to owner-only (0600) on Unix
            #[cfg(unix)]
            {
                let permissions = fs::Permissions::from_mode(0o600);
                if let Err(e) = fs::set_permissions(&path, permissions) {
                    eprintln!("Warning: could not set config file permissions: {}", e);
                }
            }
        }
        Ok(())
    }

    pub fn add_recent_connection(&mut self, info: ConnectionInfo) {
        // Store password securely in OS keyring
        if !info.password.is_empty() {
            if let Err(e) = credential_store::store_password(&info.name, &info.password) {
                eprintln!("Keyring store warning: {}", e);
            }
        }

        // Remove existing connection with same name
        self.recent_connections.retain(|c| c.name != info.name);

        // Add to front
        self.recent_connections.insert(0, info);

        // Keep only last 10 connections
        self.recent_connections.truncate(10);
    }

    pub fn get_connection_by_name(&self, name: &str) -> Option<&ConnectionInfo> {
        self.recent_connections.iter().find(|c| c.name == name)
    }

    pub fn remove_connection(&mut self, name: &str) {
        // Remove password from OS keyring
        if let Err(e) = credential_store::delete_password(name) {
            eprintln!("Keyring delete warning: {}", e);
        }
        self.recent_connections.retain(|c| c.name != name);
    }

    pub fn get_all_connections(&self) -> &Vec<ConnectionInfo> {
        &self.recent_connections
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryHistory {
    pub queries: Vec<QueryHistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub sql: String,
    pub timestamp: String,
    pub execution_time_ms: u64,
    pub row_count: usize,
    pub connection_name: String,
}

impl QueryHistory {
    pub fn new() -> Self {
        Self {
            queries: Vec::new(),
        }
    }

    pub fn history_path() -> Option<PathBuf> {
        dirs::data_dir().map(|mut path| {
            path.push("oracle_query_tool");
            path.push("history.json");
            path
        })
    }

    pub fn load() -> Self {
        if let Some(path) = Self::history_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(history) = serde_json::from_str(&content) {
                        return history;
                    }
                }
            }
        }
        Self::new()
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::history_path() {
            if let Some(parent) = path.parent() {
                match fs::create_dir_all(parent) {
                    Ok(()) => {}
                    Err(err) => {
                        eprintln!("Config persistence error: {err}");
                        return Err(Box::new(err));
                    }
                }
            }
            let content = match serde_json::to_string_pretty(self) {
                Ok(content) => content,
                Err(err) => {
                    eprintln!("Config persistence error: {err}");
                    return Err(Box::new(err));
                }
            };
            match fs::write(path, content) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("Config persistence error: {err}");
                    return Err(Box::new(err));
                }
            }
        }
        Ok(())
    }

    pub fn add_entry(&mut self, entry: QueryHistoryEntry) {
        self.queries.insert(0, entry);
        // Keep only last 1000 queries
        self.queries.truncate(1000);
    }
}

impl Default for QueryHistory {
    fn default() -> Self {
        Self::new()
    }
}
