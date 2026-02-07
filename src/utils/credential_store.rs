use keyring::Entry;

const SERVICE_NAME: &str = "oracle_query_tool";

fn entry_for(connection_name: &str) -> Result<Entry, keyring::Error> {
    Entry::new(SERVICE_NAME, connection_name)
}

/// Store a password in the OS keyring for the given connection name.
pub fn store_password(connection_name: &str, password: &str) -> Result<(), String> {
    let entry = entry_for(connection_name).map_err(|e| format!("Keyring error: {}", e))?;
    entry
        .set_password(password)
        .map_err(|e| format!("Failed to store password in keyring: {}", e))
}

/// Retrieve a password from the OS keyring for the given connection name.
/// Returns Ok(None) if no credential is found (not an error).
pub fn get_password(connection_name: &str) -> Result<Option<String>, String> {
    let entry = entry_for(connection_name).map_err(|e| format!("Keyring error: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to retrieve password from keyring: {}", e)),
    }
}

/// Delete a password from the OS keyring for the given connection name.
/// Silently succeeds if no credential exists.
pub fn delete_password(connection_name: &str) -> Result<(), String> {
    let entry = entry_for(connection_name).map_err(|e| format!("Keyring error: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete password from keyring: {}", e)),
    }
}
