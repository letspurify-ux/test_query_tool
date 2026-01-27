use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct FeatureDefinition {
    pub name: Cow<'static, str>,
    pub category: Cow<'static, str>,
    pub implemented: bool,
    pub description: Cow<'static, str>,
}

#[derive(Debug, Clone)]
pub struct FeatureCatalog {
    pub features: Vec<FeatureDefinition>,
    pub total_count: usize,
    pub implemented_count: usize,
    pub external_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalFeatureDefinition {
    name: String,
    category: String,
    implemented: bool,
    description: String,
}

pub fn load_feature_catalog() -> FeatureCatalog {
    let mut features = static_features();
    let external_features = load_external_features();
    let external_count = external_features.len();
    features.extend(external_features);

    let total_count = features.len();
    let implemented_count = features
        .iter()
        .filter(|feature| feature.implemented)
        .count();

    FeatureCatalog {
        features,
        total_count,
        implemented_count,
        external_count,
    }
}

#[allow(dead_code)]
pub fn list_features() -> Vec<FeatureDefinition> {
    load_feature_catalog().features
}

#[allow(dead_code)]
pub fn total_count() -> usize {
    load_feature_catalog().total_count
}

#[allow(dead_code)]
pub fn implemented_count() -> usize {
    load_feature_catalog().implemented_count
}

#[allow(dead_code)]
pub fn build_catalog_text() -> String {
    let catalog = load_feature_catalog();
    build_catalog_text_filtered(&catalog, "", true, true)
}

pub fn build_catalog_text_filtered(
    catalog: &FeatureCatalog,
    filter: &str,
    show_implemented: bool,
    show_planned: bool,
) -> String {
    let mut output = String::new();
    let mut features = catalog.features.clone();
    let total = catalog.total_count;
    let implemented = catalog.implemented_count;
    let filter_lower = filter.trim().to_ascii_lowercase();

    features.retain(|feature| {
        let matches_filter = if filter_lower.is_empty() {
            true
        } else {
            let haystack = format!(
                "{} {} {}",
                feature.category, feature.name, feature.description
            )
            .to_ascii_lowercase();
            haystack.contains(&filter_lower)
        };

        if feature.implemented {
            matches_filter && show_implemented
        } else {
            matches_filter && show_planned
        }
    });

    let matching_total = features.len();
    let matching_implemented = features
        .iter()
        .filter(|feature| feature.implemented)
        .count();

    output.push_str(&format!(
        "Toad Core Feature Catalog (Implemented: {implemented}/{total}, External: {external_count})\n",
        external_count = catalog.external_count
    ));
    output.push_str(&format!(
        "Filtered: {matching_implemented}/{matching_total} | Status: [✓ Implemented] [• Planned]\n\n"
    ));

    if features.is_empty() {
        output.push_str("No matching features.\n");
        return output;
    }

    let mut grouped: BTreeMap<Cow<'static, str>, Vec<FeatureDefinition>> = BTreeMap::new();
    for feature in features {
        grouped.entry(feature.category.clone()).or_default().push(feature);
    }

    for (category, items) in grouped {
        output.push_str(&format!("{category}\n"));
        output.push_str(&"-".repeat(category.len()));
        output.push('\n');
        for item in items {
            let status = if item.implemented { "✓" } else { "•" };
            output.push_str(&format!(
                "[{status}] {} — {}\n",
                item.name, item.description
            ));
        }
        output.push('\n');
    }

    output
}

fn load_external_features() -> Vec<FeatureDefinition> {
    let Some(path) = external_features_path() else {
        return Vec::new();
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };

    let parsed: Vec<ExternalFeatureDefinition> = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(_) => return Vec::new(),
    };

    parsed
        .into_iter()
        .map(|feature| FeatureDefinition {
            name: Cow::Owned(feature.name),
            category: Cow::Owned(feature.category),
            implemented: feature.implemented,
            description: Cow::Owned(feature.description),
        })
        .collect()
}

fn external_features_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("TOAD_FEATURE_CATALOG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    let default_path = PathBuf::from("toad_manual_features.json");
    if default_path.exists() {
        return Some(default_path);
    }

    None
}

fn static_features() -> Vec<FeatureDefinition> {
    vec![
    FeatureDefinition {
        name: Cow::Borrowed("Saved connection profiles"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Store connection profiles with names and metadata."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connect to Oracle instance"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: true,
        description: Cow::Borrowed("Open a database session from the connection dialog."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Disconnect session"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: true,
        description: Cow::Borrowed("Explicitly close the active database session."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection status indicator"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: true,
        description: Cow::Borrowed("Show current connection details in the status bar."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Multiple simultaneous connections"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Manage more than one active database session."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection timeout handling"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Gracefully handle timeouts and reconnect prompts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection keep-alive"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Ping the server to keep sessions alive."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Read-only session mode"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Open a session that blocks DML/DDL changes."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session browser"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("List current sessions and their state."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session kill"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Terminate selected sessions by SID/Serial."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session variables editor"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Edit NLS settings and optimizer parameters."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Proxy user connections"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Connect using proxy authentication."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection by TNS alias"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Use a local TNS alias for connections."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Service name vs SID toggle"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Choose between service name and SID connection."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection statistics view"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("View round-trip latency and session metrics."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Recent connections list"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Show recently used connections for quick access."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Connection notes"),
        category: Cow::Borrowed("Connection & Session"),
        implemented: false,
        description: Cow::Borrowed("Attach notes and tags to connection profiles."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Password vault integration"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Retrieve credentials from a secure vault."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("OS authentication"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Authenticate using OS credentials."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SSL/TLS configuration"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Configure encrypted transport settings."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Wallet-based authentication"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Use Oracle wallet for connection credentials."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Role switching"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Enable or disable roles in a session."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Privilege inspector"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Inspect object and system privileges."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Audit trail viewer"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("View audited actions and login events."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data masking presets"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Apply masking templates to sensitive data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Redaction preview"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Preview data redaction rules."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Credential expiration alerts"),
        category: Cow::Borrowed("Security & Authentication"),
        implemented: false,
        description: Cow::Borrowed("Notify when passwords or wallets expire."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Multi-tab SQL editor"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Work with multiple query tabs in one workspace."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL editor syntax highlighting"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Highlight SQL keywords and object names."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Auto-complete (intellisense)"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Show schema-driven completion suggestions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Keyword auto-format"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Format SQL keywords and indentation."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Code folding"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Collapse blocks for easier navigation."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Line numbers"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Show line numbers in the editor margin."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Bracket matching"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Highlight matching parentheses and blocks."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL snippets library"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Insert reusable SQL templates."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Editor themes"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Switch between light/dark editor themes."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Multiple caret editing"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Edit multiple lines with multiple cursors."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Find in editor"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Search for text in the SQL editor."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Replace in editor"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Replace text in the SQL editor."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Find next"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Jump to the next search match."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Open SQL files"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Open SQL scripts from disk."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Save SQL files"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Save SQL scripts to disk."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Auto-save drafts"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Automatically save unsaved query drafts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL linting"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Surface syntax or style warnings."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Quick describe (F4)"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: true,
        description: Cow::Borrowed("Describe the object under the cursor."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Clipboard history"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Access previously copied snippets."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session bind variables"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Define and reuse bind variables."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL templates browser"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Browse built-in SQL starter templates."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Code comment toggling"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Toggle comment markers for selected lines."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Upper/lowercase conversion"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Convert selected text case."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Editor split view"),
        category: Cow::Borrowed("SQL Editor"),
        implemented: false,
        description: Cow::Borrowed("Split the editor for side-by-side edits."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Query execution (batch)"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("Run a batch of SQL statements."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Execute selected statement"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Run only the highlighted SQL."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Explain plan"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("Display execution plan for SQL."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Commit transaction"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("Commit the current transaction."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Rollback transaction"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("Rollback the current transaction."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Auto-commit mode"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Automatically commit after statements."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Cancel running query"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Stop a long-running query."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Query timeout control"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Set maximum execution time."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Run with tracing"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Enable SQL trace for the session."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Fetch size configuration"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Tune rows fetched per round trip."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Parallel query hint manager"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Apply and manage PQ hints."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL history logging"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("Persist query history entries."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Query history browser"),
        category: Cow::Borrowed("Query Execution"),
        implemented: true,
        description: Cow::Borrowed("View and reuse executed queries."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Execution statistics"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Display logical reads, buffer gets, etc."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL monitor"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Monitor active SQL execution progress."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL output window"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Display DBMS_OUTPUT results."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Query execution profiles"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Save execution settings per query."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Bind variable prompts"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Prompt for bind values at runtime."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session NLS options"),
        category: Cow::Borrowed("Query Execution"),
        implemented: false,
        description: Cow::Borrowed("Adjust NLS settings for executions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Result grid display"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: true,
        description: Cow::Borrowed("Display query results in a grid."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid sorting"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Sort result columns by clicking headers."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid filtering"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Filter rows by column values."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid column resizing"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Resize columns in the results grid."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid copy/export"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Copy selected rows to clipboard."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid row highlighting"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Highlight matching rows."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Pinned column support"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Freeze columns while scrolling."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Result set pagination"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Load large result sets in pages."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Result export to CSV"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: true,
        description: Cow::Borrowed("Export result rows to CSV."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to Excel"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Export results to XLSX."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to JSON"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Export results to JSON format."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to XML"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Export results to XML format."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid cell editing"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Edit data directly in the grid."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Row insert/update/delete"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Apply row-level DML from the grid."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data validation rules"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Enforce column validation on edits."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("LOB viewer"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Open CLOB/BLOB in a viewer."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Grid preferences"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Configure grid fonts and colors."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Result summary footer"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Show row count and execution stats."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Multiple result tabs"),
        category: Cow::Borrowed("Results & Data Grid"),
        implemented: false,
        description: Cow::Borrowed("Show each query result in a tab."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: tables"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: true,
        description: Cow::Borrowed("List tables in the schema browser."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: views"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: true,
        description: Cow::Borrowed("List views in the schema browser."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: procedures"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: true,
        description: Cow::Borrowed("List stored procedures in the browser."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: functions"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: true,
        description: Cow::Borrowed("List stored functions in the browser."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: packages"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse packages and package bodies."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: triggers"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse triggers per table."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: indexes"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse index definitions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: sequences"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse sequences and metadata."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: synonyms"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse synonyms."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: materialized views"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse materialized views and logs."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: types"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse user-defined types."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: queues"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse AQ queues."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: database links"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Browse database links."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object browser: grants"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("View grants and privileges."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object DDL viewer"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("View generated CREATE DDL."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object search"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Search objects by name pattern."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object dependencies"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("View object dependencies graph."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Schema compare"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Compare schema objects across databases."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Schema export"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Export schema DDL scripts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Object refresh"),
        category: Cow::Borrowed("Schema & Object Browser"),
        implemented: false,
        description: Cow::Borrowed("Refresh object browser lists."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL debugger"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Debug stored procedures with breakpoints."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL compiler warnings"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Surface compiler warnings."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL unit testing"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Run unit tests for packages."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL profiler"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Profile PL/SQL performance."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL code templates"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Insert PL/SQL template snippets."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL execution engine"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Run anonymous PL/SQL blocks."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Package specification editor"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Edit package specs with syntax support."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Package body editor"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Edit package bodies with syntax support."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Code coverage"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Measure coverage of PL/SQL tests."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Refactor rename"),
        category: Cow::Borrowed("PL/SQL & Code Tools"),
        implemented: false,
        description: Cow::Borrowed("Rename symbols across PL/SQL."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data import wizard"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Import CSV/Excel data into tables."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data export wizard"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Export table data with mapping."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data compare"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Compare data between schemas."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data sync"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Sync table data between environments."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data subset extractor"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Extract a subset of rows based on rules."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Copy table"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Create a copy of a table structure/data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Truncate table"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Truncate table data with confirmation."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Generate insert statements"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Generate INSERT scripts from data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data viewer for tables"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Browse table data without writing SQL."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data filter builder"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Build filters for viewing table data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Table editor"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Edit table data in a grid."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data generator"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Generate test data for tables."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("PL/SQL output logging"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Store DBMS_OUTPUT logs for sessions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Bulk DML loader"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Load large data sets efficiently."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Constraint manager"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Enable/disable constraints quickly."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Index rebuild wizard"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Rebuild indexes for performance."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Statistics gather"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Gather optimizer statistics."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Partition manager"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Manage partitioned tables and indexes."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Flashback query"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Query historical data with flashback."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Data dictionary browser"),
        category: Cow::Borrowed("Data Management"),
        implemented: false,
        description: Cow::Borrowed("Browse dictionary views with templates."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL optimizer hints catalog"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Browse available optimizer hints."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Explain plan visualization"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Visual plan viewer with tree layout."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL tuning advisor"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Recommend SQL tuning actions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Index usage monitor"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Track index usage statistics."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Top SQL report"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Report top SQL by CPU/IO."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session waits"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Inspect waits and events."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("ASH report"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Generate Active Session History report."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("AWR report"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Generate Automatic Workload Repository report."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL trace viewer"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Open and analyze trace files."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Plan baseline manager"),
        category: Cow::Borrowed("Performance & Tuning"),
        implemented: false,
        description: Cow::Borrowed("Manage SQL plan baselines."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Scheduler jobs browser"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Browse and manage scheduler jobs."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Job creation wizard"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Create new scheduler jobs."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Job history viewer"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Review job run history."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Alerts and notifications"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Configure job completion alerts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Script execution scheduler"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Schedule SQL scripts to run automatically."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Macro recorder"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Record and replay user actions."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Batch execution queues"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Queue scripts for batch execution."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Task templates"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Reuse automation task templates."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Job dependency graphs"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Visualize dependencies between jobs."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Execution notifications"),
        category: Cow::Borrowed("Automation & Scheduling"),
        implemented: false,
        description: Cow::Borrowed("Notify on success or failure."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Report designer"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Design formatted reports from data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Report scheduling"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Schedule reports for delivery."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Report templates"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Use predefined report layouts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to PDF"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Export results and reports to PDF."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to HTML"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Export results to HTML."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Export to text"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Export results to delimited text."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Charting"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Create charts from query results."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Pivot table"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Pivot data for analysis."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Email report delivery"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Send reports by email."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Report archive"),
        category: Cow::Borrowed("Reporting & Export"),
        implemented: false,
        description: Cow::Borrowed("Store generated reports."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Workspace layouts"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Save and switch workspace layouts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Dockable panels"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Dock and float tool panels."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Tabbed results"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Show multiple result tabs."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Custom keyboard shortcuts"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Customize shortcuts for commands."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Theme switcher"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Switch application themes."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session workspace restore"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Restore open tabs on launch."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Toolbar customization"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Configure toolbar buttons."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Status bar widgets"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Add custom status indicators."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Notification center"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("View non-modal alerts and messages."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Session activity indicator"),
        category: Cow::Borrowed("UI & Workspace"),
        implemented: false,
        description: Cow::Borrowed("Display running query activity."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Team shared snippets"),
        category: Cow::Borrowed("Collaboration & Versioning"),
        implemented: false,
        description: Cow::Borrowed("Share snippets with a team library."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Git integration"),
        category: Cow::Borrowed("Collaboration & Versioning"),
        implemented: false,
        description: Cow::Borrowed("Version SQL scripts with Git."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Schema compare reports"),
        category: Cow::Borrowed("Collaboration & Versioning"),
        implemented: false,
        description: Cow::Borrowed("Generate diff reports for schemas."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Team dashboards"),
        category: Cow::Borrowed("Collaboration & Versioning"),
        implemented: false,
        description: Cow::Borrowed("Share project dashboards."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Change request workflow"),
        category: Cow::Borrowed("Collaboration & Versioning"),
        implemented: false,
        description: Cow::Borrowed("Track change requests and approvals."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("User management"),
        category: Cow::Borrowed("Administration & Monitoring"),
        implemented: false,
        description: Cow::Borrowed("Create and manage database users."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Role management"),
        category: Cow::Borrowed("Administration & Monitoring"),
        implemented: false,
        description: Cow::Borrowed("Create and manage roles."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Storage manager"),
        category: Cow::Borrowed("Administration & Monitoring"),
        implemented: false,
        description: Cow::Borrowed("Monitor tablespace usage."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Database health dashboard"),
        category: Cow::Borrowed("Administration & Monitoring"),
        implemented: false,
        description: Cow::Borrowed("Summarize health and alerts."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Alert log viewer"),
        category: Cow::Borrowed("Administration & Monitoring"),
        implemented: false,
        description: Cow::Borrowed("View Oracle alert log entries."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Plugin SDK"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Expose APIs for plugin developers."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Scripting API"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Automate actions with a scripting API."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("REST API integration"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Connect to REST endpoints for data."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("External tool launcher"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Launch external tools from the UI."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Custom export plugins"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Add custom export formats."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Authentication plugins"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Extend authentication providers."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("SQL parser plugins"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Swap SQL parsing engines."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Custom data providers"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Integrate non-Oracle data sources."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("UI extension points"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Extend menus and panels with plugins."),
    },
    FeatureDefinition {
        name: Cow::Borrowed("Telemetry and usage analytics"),
        category: Cow::Borrowed("Integration & Extensibility"),
        implemented: false,
        description: Cow::Borrowed("Collect usage metrics (opt-in)."),
    },
]}
