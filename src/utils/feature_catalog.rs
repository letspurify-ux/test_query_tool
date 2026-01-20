use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct FeatureDefinition {
    pub name: &'static str,
    pub category: &'static str,
    pub implemented: bool,
    pub description: &'static str,
}

pub fn list_features() -> Vec<FeatureDefinition> {
    FEATURES.to_vec()
}

pub fn total_count() -> usize {
    FEATURES.len()
}

pub fn implemented_count() -> usize {
    FEATURES
        .iter()
        .filter(|feature| feature.implemented)
        .count()
}

pub fn build_catalog_text() -> String {
    let mut output = String::new();
    let features = list_features();
    let total = features.len();
    let implemented = implemented_count();

    output.push_str(&format!(
        "Toad Core Feature Catalog (Implemented: {implemented}/{total})\n"
    ));
    output.push_str("Status: [✓ Implemented] [• Planned]\n\n");

    let mut grouped: BTreeMap<&'static str, Vec<FeatureDefinition>> = BTreeMap::new();
    for feature in features {
        grouped.entry(feature.category).or_default().push(feature);
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

static FEATURES: &[FeatureDefinition] = &[
    FeatureDefinition {
        name: "Saved connection profiles",
        category: "Connection & Session",
        implemented: false,
        description: "Store connection profiles with names and metadata.",
    },
    FeatureDefinition {
        name: "Connect to Oracle instance",
        category: "Connection & Session",
        implemented: true,
        description: "Open a database session from the connection dialog.",
    },
    FeatureDefinition {
        name: "Disconnect session",
        category: "Connection & Session",
        implemented: true,
        description: "Explicitly close the active database session.",
    },
    FeatureDefinition {
        name: "Connection status indicator",
        category: "Connection & Session",
        implemented: true,
        description: "Show current connection details in the status bar.",
    },
    FeatureDefinition {
        name: "Multiple simultaneous connections",
        category: "Connection & Session",
        implemented: false,
        description: "Manage more than one active database session.",
    },
    FeatureDefinition {
        name: "Connection timeout handling",
        category: "Connection & Session",
        implemented: false,
        description: "Gracefully handle timeouts and reconnect prompts.",
    },
    FeatureDefinition {
        name: "Connection keep-alive",
        category: "Connection & Session",
        implemented: false,
        description: "Ping the server to keep sessions alive.",
    },
    FeatureDefinition {
        name: "Read-only session mode",
        category: "Connection & Session",
        implemented: false,
        description: "Open a session that blocks DML/DDL changes.",
    },
    FeatureDefinition {
        name: "Session browser",
        category: "Connection & Session",
        implemented: false,
        description: "List current sessions and their state.",
    },
    FeatureDefinition {
        name: "Session kill",
        category: "Connection & Session",
        implemented: false,
        description: "Terminate selected sessions by SID/Serial.",
    },
    FeatureDefinition {
        name: "Session variables editor",
        category: "Connection & Session",
        implemented: false,
        description: "Edit NLS settings and optimizer parameters.",
    },
    FeatureDefinition {
        name: "Proxy user connections",
        category: "Connection & Session",
        implemented: false,
        description: "Connect using proxy authentication.",
    },
    FeatureDefinition {
        name: "Connection by TNS alias",
        category: "Connection & Session",
        implemented: false,
        description: "Use a local TNS alias for connections.",
    },
    FeatureDefinition {
        name: "Service name vs SID toggle",
        category: "Connection & Session",
        implemented: false,
        description: "Choose between service name and SID connection.",
    },
    FeatureDefinition {
        name: "Connection statistics view",
        category: "Connection & Session",
        implemented: false,
        description: "View round-trip latency and session metrics.",
    },
    FeatureDefinition {
        name: "Recent connections list",
        category: "Connection & Session",
        implemented: false,
        description: "Show recently used connections for quick access.",
    },
    FeatureDefinition {
        name: "Connection notes",
        category: "Connection & Session",
        implemented: false,
        description: "Attach notes and tags to connection profiles.",
    },
    FeatureDefinition {
        name: "Password vault integration",
        category: "Security & Authentication",
        implemented: false,
        description: "Retrieve credentials from a secure vault.",
    },
    FeatureDefinition {
        name: "OS authentication",
        category: "Security & Authentication",
        implemented: false,
        description: "Authenticate using OS credentials.",
    },
    FeatureDefinition {
        name: "SSL/TLS configuration",
        category: "Security & Authentication",
        implemented: false,
        description: "Configure encrypted transport settings.",
    },
    FeatureDefinition {
        name: "Wallet-based authentication",
        category: "Security & Authentication",
        implemented: false,
        description: "Use Oracle wallet for connection credentials.",
    },
    FeatureDefinition {
        name: "Role switching",
        category: "Security & Authentication",
        implemented: false,
        description: "Enable or disable roles in a session.",
    },
    FeatureDefinition {
        name: "Privilege inspector",
        category: "Security & Authentication",
        implemented: false,
        description: "Inspect object and system privileges.",
    },
    FeatureDefinition {
        name: "Audit trail viewer",
        category: "Security & Authentication",
        implemented: false,
        description: "View audited actions and login events.",
    },
    FeatureDefinition {
        name: "Data masking presets",
        category: "Security & Authentication",
        implemented: false,
        description: "Apply masking templates to sensitive data.",
    },
    FeatureDefinition {
        name: "Redaction preview",
        category: "Security & Authentication",
        implemented: false,
        description: "Preview data redaction rules.",
    },
    FeatureDefinition {
        name: "Credential expiration alerts",
        category: "Security & Authentication",
        implemented: false,
        description: "Notify when passwords or wallets expire.",
    },
    FeatureDefinition {
        name: "Multi-tab SQL editor",
        category: "SQL Editor",
        implemented: false,
        description: "Work with multiple query tabs in one workspace.",
    },
    FeatureDefinition {
        name: "SQL editor syntax highlighting",
        category: "SQL Editor",
        implemented: true,
        description: "Highlight SQL keywords and object names.",
    },
    FeatureDefinition {
        name: "Auto-complete (intellisense)",
        category: "SQL Editor",
        implemented: true,
        description: "Show schema-driven completion suggestions.",
    },
    FeatureDefinition {
        name: "Keyword auto-format",
        category: "SQL Editor",
        implemented: false,
        description: "Format SQL keywords and indentation.",
    },
    FeatureDefinition {
        name: "Code folding",
        category: "SQL Editor",
        implemented: false,
        description: "Collapse blocks for easier navigation.",
    },
    FeatureDefinition {
        name: "Line numbers",
        category: "SQL Editor",
        implemented: false,
        description: "Show line numbers in the editor margin.",
    },
    FeatureDefinition {
        name: "Bracket matching",
        category: "SQL Editor",
        implemented: false,
        description: "Highlight matching parentheses and blocks.",
    },
    FeatureDefinition {
        name: "SQL snippets library",
        category: "SQL Editor",
        implemented: false,
        description: "Insert reusable SQL templates.",
    },
    FeatureDefinition {
        name: "Editor themes",
        category: "SQL Editor",
        implemented: false,
        description: "Switch between light/dark editor themes.",
    },
    FeatureDefinition {
        name: "Multiple caret editing",
        category: "SQL Editor",
        implemented: false,
        description: "Edit multiple lines with multiple cursors.",
    },
    FeatureDefinition {
        name: "Find in editor",
        category: "SQL Editor",
        implemented: true,
        description: "Search for text in the SQL editor.",
    },
    FeatureDefinition {
        name: "Replace in editor",
        category: "SQL Editor",
        implemented: true,
        description: "Replace text in the SQL editor.",
    },
    FeatureDefinition {
        name: "Find next",
        category: "SQL Editor",
        implemented: false,
        description: "Jump to the next search match.",
    },
    FeatureDefinition {
        name: "Open SQL files",
        category: "SQL Editor",
        implemented: true,
        description: "Open SQL scripts from disk.",
    },
    FeatureDefinition {
        name: "Save SQL files",
        category: "SQL Editor",
        implemented: true,
        description: "Save SQL scripts to disk.",
    },
    FeatureDefinition {
        name: "Auto-save drafts",
        category: "SQL Editor",
        implemented: false,
        description: "Automatically save unsaved query drafts.",
    },
    FeatureDefinition {
        name: "SQL linting",
        category: "SQL Editor",
        implemented: false,
        description: "Surface syntax or style warnings.",
    },
    FeatureDefinition {
        name: "Quick describe (F4)",
        category: "SQL Editor",
        implemented: true,
        description: "Describe the object under the cursor.",
    },
    FeatureDefinition {
        name: "Clipboard history",
        category: "SQL Editor",
        implemented: false,
        description: "Access previously copied snippets.",
    },
    FeatureDefinition {
        name: "Session bind variables",
        category: "SQL Editor",
        implemented: false,
        description: "Define and reuse bind variables.",
    },
    FeatureDefinition {
        name: "SQL templates browser",
        category: "SQL Editor",
        implemented: false,
        description: "Browse built-in SQL starter templates.",
    },
    FeatureDefinition {
        name: "Code comment toggling",
        category: "SQL Editor",
        implemented: false,
        description: "Toggle comment markers for selected lines.",
    },
    FeatureDefinition {
        name: "Upper/lowercase conversion",
        category: "SQL Editor",
        implemented: false,
        description: "Convert selected text case.",
    },
    FeatureDefinition {
        name: "Editor split view",
        category: "SQL Editor",
        implemented: false,
        description: "Split the editor for side-by-side edits.",
    },
    FeatureDefinition {
        name: "Query execution (batch)",
        category: "Query Execution",
        implemented: true,
        description: "Run a batch of SQL statements.",
    },
    FeatureDefinition {
        name: "Execute selected statement",
        category: "Query Execution",
        implemented: false,
        description: "Run only the highlighted SQL.",
    },
    FeatureDefinition {
        name: "Explain plan",
        category: "Query Execution",
        implemented: true,
        description: "Display execution plan for SQL.",
    },
    FeatureDefinition {
        name: "Commit transaction",
        category: "Query Execution",
        implemented: true,
        description: "Commit the current transaction.",
    },
    FeatureDefinition {
        name: "Rollback transaction",
        category: "Query Execution",
        implemented: true,
        description: "Rollback the current transaction.",
    },
    FeatureDefinition {
        name: "Auto-commit mode",
        category: "Query Execution",
        implemented: false,
        description: "Automatically commit after statements.",
    },
    FeatureDefinition {
        name: "Cancel running query",
        category: "Query Execution",
        implemented: false,
        description: "Stop a long-running query.",
    },
    FeatureDefinition {
        name: "Query timeout control",
        category: "Query Execution",
        implemented: false,
        description: "Set maximum execution time.",
    },
    FeatureDefinition {
        name: "Run with tracing",
        category: "Query Execution",
        implemented: false,
        description: "Enable SQL trace for the session.",
    },
    FeatureDefinition {
        name: "Fetch size configuration",
        category: "Query Execution",
        implemented: false,
        description: "Tune rows fetched per round trip.",
    },
    FeatureDefinition {
        name: "Parallel query hint manager",
        category: "Query Execution",
        implemented: false,
        description: "Apply and manage PQ hints.",
    },
    FeatureDefinition {
        name: "SQL history logging",
        category: "Query Execution",
        implemented: true,
        description: "Persist query history entries.",
    },
    FeatureDefinition {
        name: "Query history browser",
        category: "Query Execution",
        implemented: true,
        description: "View and reuse executed queries.",
    },
    FeatureDefinition {
        name: "Execution statistics",
        category: "Query Execution",
        implemented: false,
        description: "Display logical reads, buffer gets, etc.",
    },
    FeatureDefinition {
        name: "SQL monitor",
        category: "Query Execution",
        implemented: false,
        description: "Monitor active SQL execution progress.",
    },
    FeatureDefinition {
        name: "PL/SQL output window",
        category: "Query Execution",
        implemented: false,
        description: "Display DBMS_OUTPUT results.",
    },
    FeatureDefinition {
        name: "Query execution profiles",
        category: "Query Execution",
        implemented: false,
        description: "Save execution settings per query.",
    },
    FeatureDefinition {
        name: "Bind variable prompts",
        category: "Query Execution",
        implemented: false,
        description: "Prompt for bind values at runtime.",
    },
    FeatureDefinition {
        name: "Session NLS options",
        category: "Query Execution",
        implemented: false,
        description: "Adjust NLS settings for executions.",
    },
    FeatureDefinition {
        name: "Result grid display",
        category: "Results & Data Grid",
        implemented: true,
        description: "Display query results in a grid.",
    },
    FeatureDefinition {
        name: "Grid sorting",
        category: "Results & Data Grid",
        implemented: false,
        description: "Sort result columns by clicking headers.",
    },
    FeatureDefinition {
        name: "Grid filtering",
        category: "Results & Data Grid",
        implemented: false,
        description: "Filter rows by column values.",
    },
    FeatureDefinition {
        name: "Grid column resizing",
        category: "Results & Data Grid",
        implemented: false,
        description: "Resize columns in the results grid.",
    },
    FeatureDefinition {
        name: "Grid copy/export",
        category: "Results & Data Grid",
        implemented: false,
        description: "Copy selected rows to clipboard.",
    },
    FeatureDefinition {
        name: "Grid row highlighting",
        category: "Results & Data Grid",
        implemented: false,
        description: "Highlight matching rows.",
    },
    FeatureDefinition {
        name: "Pinned column support",
        category: "Results & Data Grid",
        implemented: false,
        description: "Freeze columns while scrolling.",
    },
    FeatureDefinition {
        name: "Result set pagination",
        category: "Results & Data Grid",
        implemented: false,
        description: "Load large result sets in pages.",
    },
    FeatureDefinition {
        name: "Result export to CSV",
        category: "Results & Data Grid",
        implemented: true,
        description: "Export result rows to CSV.",
    },
    FeatureDefinition {
        name: "Export to Excel",
        category: "Results & Data Grid",
        implemented: false,
        description: "Export results to XLSX.",
    },
    FeatureDefinition {
        name: "Export to JSON",
        category: "Results & Data Grid",
        implemented: false,
        description: "Export results to JSON format.",
    },
    FeatureDefinition {
        name: "Export to XML",
        category: "Results & Data Grid",
        implemented: false,
        description: "Export results to XML format.",
    },
    FeatureDefinition {
        name: "Grid cell editing",
        category: "Results & Data Grid",
        implemented: false,
        description: "Edit data directly in the grid.",
    },
    FeatureDefinition {
        name: "Row insert/update/delete",
        category: "Results & Data Grid",
        implemented: false,
        description: "Apply row-level DML from the grid.",
    },
    FeatureDefinition {
        name: "Data validation rules",
        category: "Results & Data Grid",
        implemented: false,
        description: "Enforce column validation on edits.",
    },
    FeatureDefinition {
        name: "LOB viewer",
        category: "Results & Data Grid",
        implemented: false,
        description: "Open CLOB/BLOB in a viewer.",
    },
    FeatureDefinition {
        name: "Grid preferences",
        category: "Results & Data Grid",
        implemented: false,
        description: "Configure grid fonts and colors.",
    },
    FeatureDefinition {
        name: "Result summary footer",
        category: "Results & Data Grid",
        implemented: false,
        description: "Show row count and execution stats.",
    },
    FeatureDefinition {
        name: "Multiple result tabs",
        category: "Results & Data Grid",
        implemented: false,
        description: "Show each query result in a tab.",
    },
    FeatureDefinition {
        name: "Object browser: tables",
        category: "Schema & Object Browser",
        implemented: true,
        description: "List tables in the schema browser.",
    },
    FeatureDefinition {
        name: "Object browser: views",
        category: "Schema & Object Browser",
        implemented: true,
        description: "List views in the schema browser.",
    },
    FeatureDefinition {
        name: "Object browser: procedures",
        category: "Schema & Object Browser",
        implemented: true,
        description: "List stored procedures in the browser.",
    },
    FeatureDefinition {
        name: "Object browser: functions",
        category: "Schema & Object Browser",
        implemented: true,
        description: "List stored functions in the browser.",
    },
    FeatureDefinition {
        name: "Object browser: packages",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse packages and package bodies.",
    },
    FeatureDefinition {
        name: "Object browser: triggers",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse triggers per table.",
    },
    FeatureDefinition {
        name: "Object browser: indexes",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse index definitions.",
    },
    FeatureDefinition {
        name: "Object browser: sequences",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse sequences and metadata.",
    },
    FeatureDefinition {
        name: "Object browser: synonyms",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse synonyms.",
    },
    FeatureDefinition {
        name: "Object browser: materialized views",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse materialized views and logs.",
    },
    FeatureDefinition {
        name: "Object browser: types",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse user-defined types.",
    },
    FeatureDefinition {
        name: "Object browser: queues",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse AQ queues.",
    },
    FeatureDefinition {
        name: "Object browser: database links",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Browse database links.",
    },
    FeatureDefinition {
        name: "Object browser: grants",
        category: "Schema & Object Browser",
        implemented: false,
        description: "View grants and privileges.",
    },
    FeatureDefinition {
        name: "Object DDL viewer",
        category: "Schema & Object Browser",
        implemented: false,
        description: "View generated CREATE DDL.",
    },
    FeatureDefinition {
        name: "Object search",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Search objects by name pattern.",
    },
    FeatureDefinition {
        name: "Object dependencies",
        category: "Schema & Object Browser",
        implemented: false,
        description: "View object dependencies graph.",
    },
    FeatureDefinition {
        name: "Schema compare",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Compare schema objects across databases.",
    },
    FeatureDefinition {
        name: "Schema export",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Export schema DDL scripts.",
    },
    FeatureDefinition {
        name: "Object refresh",
        category: "Schema & Object Browser",
        implemented: false,
        description: "Refresh object browser lists.",
    },
    FeatureDefinition {
        name: "PL/SQL debugger",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Debug stored procedures with breakpoints.",
    },
    FeatureDefinition {
        name: "PL/SQL compiler warnings",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Surface compiler warnings.",
    },
    FeatureDefinition {
        name: "PL/SQL unit testing",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Run unit tests for packages.",
    },
    FeatureDefinition {
        name: "PL/SQL profiler",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Profile PL/SQL performance.",
    },
    FeatureDefinition {
        name: "PL/SQL code templates",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Insert PL/SQL template snippets.",
    },
    FeatureDefinition {
        name: "PL/SQL execution engine",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Run anonymous PL/SQL blocks.",
    },
    FeatureDefinition {
        name: "Package specification editor",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Edit package specs with syntax support.",
    },
    FeatureDefinition {
        name: "Package body editor",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Edit package bodies with syntax support.",
    },
    FeatureDefinition {
        name: "Code coverage",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Measure coverage of PL/SQL tests.",
    },
    FeatureDefinition {
        name: "Refactor rename",
        category: "PL/SQL & Code Tools",
        implemented: false,
        description: "Rename symbols across PL/SQL.",
    },
    FeatureDefinition {
        name: "Data import wizard",
        category: "Data Management",
        implemented: false,
        description: "Import CSV/Excel data into tables.",
    },
    FeatureDefinition {
        name: "Data export wizard",
        category: "Data Management",
        implemented: false,
        description: "Export table data with mapping.",
    },
    FeatureDefinition {
        name: "Data compare",
        category: "Data Management",
        implemented: false,
        description: "Compare data between schemas.",
    },
    FeatureDefinition {
        name: "Data sync",
        category: "Data Management",
        implemented: false,
        description: "Sync table data between environments.",
    },
    FeatureDefinition {
        name: "Data subset extractor",
        category: "Data Management",
        implemented: false,
        description: "Extract a subset of rows based on rules.",
    },
    FeatureDefinition {
        name: "Copy table",
        category: "Data Management",
        implemented: false,
        description: "Create a copy of a table structure/data.",
    },
    FeatureDefinition {
        name: "Truncate table",
        category: "Data Management",
        implemented: false,
        description: "Truncate table data with confirmation.",
    },
    FeatureDefinition {
        name: "Generate insert statements",
        category: "Data Management",
        implemented: false,
        description: "Generate INSERT scripts from data.",
    },
    FeatureDefinition {
        name: "Data viewer for tables",
        category: "Data Management",
        implemented: false,
        description: "Browse table data without writing SQL.",
    },
    FeatureDefinition {
        name: "Data filter builder",
        category: "Data Management",
        implemented: false,
        description: "Build filters for viewing table data.",
    },
    FeatureDefinition {
        name: "Table editor",
        category: "Data Management",
        implemented: false,
        description: "Edit table data in a grid.",
    },
    FeatureDefinition {
        name: "Data generator",
        category: "Data Management",
        implemented: false,
        description: "Generate test data for tables.",
    },
    FeatureDefinition {
        name: "PL/SQL output logging",
        category: "Data Management",
        implemented: false,
        description: "Store DBMS_OUTPUT logs for sessions.",
    },
    FeatureDefinition {
        name: "Bulk DML loader",
        category: "Data Management",
        implemented: false,
        description: "Load large data sets efficiently.",
    },
    FeatureDefinition {
        name: "Constraint manager",
        category: "Data Management",
        implemented: false,
        description: "Enable/disable constraints quickly.",
    },
    FeatureDefinition {
        name: "Index rebuild wizard",
        category: "Data Management",
        implemented: false,
        description: "Rebuild indexes for performance.",
    },
    FeatureDefinition {
        name: "Statistics gather",
        category: "Data Management",
        implemented: false,
        description: "Gather optimizer statistics.",
    },
    FeatureDefinition {
        name: "Partition manager",
        category: "Data Management",
        implemented: false,
        description: "Manage partitioned tables and indexes.",
    },
    FeatureDefinition {
        name: "Flashback query",
        category: "Data Management",
        implemented: false,
        description: "Query historical data with flashback.",
    },
    FeatureDefinition {
        name: "Data dictionary browser",
        category: "Data Management",
        implemented: false,
        description: "Browse dictionary views with templates.",
    },
    FeatureDefinition {
        name: "SQL optimizer hints catalog",
        category: "Performance & Tuning",
        implemented: false,
        description: "Browse available optimizer hints.",
    },
    FeatureDefinition {
        name: "Explain plan visualization",
        category: "Performance & Tuning",
        implemented: false,
        description: "Visual plan viewer with tree layout.",
    },
    FeatureDefinition {
        name: "SQL tuning advisor",
        category: "Performance & Tuning",
        implemented: false,
        description: "Recommend SQL tuning actions.",
    },
    FeatureDefinition {
        name: "Index usage monitor",
        category: "Performance & Tuning",
        implemented: false,
        description: "Track index usage statistics.",
    },
    FeatureDefinition {
        name: "Top SQL report",
        category: "Performance & Tuning",
        implemented: false,
        description: "Report top SQL by CPU/IO.",
    },
    FeatureDefinition {
        name: "Session waits",
        category: "Performance & Tuning",
        implemented: false,
        description: "Inspect waits and events.",
    },
    FeatureDefinition {
        name: "ASH report",
        category: "Performance & Tuning",
        implemented: false,
        description: "Generate Active Session History report.",
    },
    FeatureDefinition {
        name: "AWR report",
        category: "Performance & Tuning",
        implemented: false,
        description: "Generate Automatic Workload Repository report.",
    },
    FeatureDefinition {
        name: "SQL trace viewer",
        category: "Performance & Tuning",
        implemented: false,
        description: "Open and analyze trace files.",
    },
    FeatureDefinition {
        name: "Plan baseline manager",
        category: "Performance & Tuning",
        implemented: false,
        description: "Manage SQL plan baselines.",
    },
    FeatureDefinition {
        name: "Scheduler jobs browser",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Browse and manage scheduler jobs.",
    },
    FeatureDefinition {
        name: "Job creation wizard",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Create new scheduler jobs.",
    },
    FeatureDefinition {
        name: "Job history viewer",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Review job run history.",
    },
    FeatureDefinition {
        name: "Alerts and notifications",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Configure job completion alerts.",
    },
    FeatureDefinition {
        name: "Script execution scheduler",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Schedule SQL scripts to run automatically.",
    },
    FeatureDefinition {
        name: "Macro recorder",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Record and replay user actions.",
    },
    FeatureDefinition {
        name: "Batch execution queues",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Queue scripts for batch execution.",
    },
    FeatureDefinition {
        name: "Task templates",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Reuse automation task templates.",
    },
    FeatureDefinition {
        name: "Job dependency graphs",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Visualize dependencies between jobs.",
    },
    FeatureDefinition {
        name: "Execution notifications",
        category: "Automation & Scheduling",
        implemented: false,
        description: "Notify on success or failure.",
    },
    FeatureDefinition {
        name: "Report designer",
        category: "Reporting & Export",
        implemented: false,
        description: "Design formatted reports from data.",
    },
    FeatureDefinition {
        name: "Report scheduling",
        category: "Reporting & Export",
        implemented: false,
        description: "Schedule reports for delivery.",
    },
    FeatureDefinition {
        name: "Report templates",
        category: "Reporting & Export",
        implemented: false,
        description: "Use predefined report layouts.",
    },
    FeatureDefinition {
        name: "Export to PDF",
        category: "Reporting & Export",
        implemented: false,
        description: "Export results and reports to PDF.",
    },
    FeatureDefinition {
        name: "Export to HTML",
        category: "Reporting & Export",
        implemented: false,
        description: "Export results to HTML.",
    },
    FeatureDefinition {
        name: "Export to text",
        category: "Reporting & Export",
        implemented: false,
        description: "Export results to delimited text.",
    },
    FeatureDefinition {
        name: "Charting",
        category: "Reporting & Export",
        implemented: false,
        description: "Create charts from query results.",
    },
    FeatureDefinition {
        name: "Pivot table",
        category: "Reporting & Export",
        implemented: false,
        description: "Pivot data for analysis.",
    },
    FeatureDefinition {
        name: "Email report delivery",
        category: "Reporting & Export",
        implemented: false,
        description: "Send reports by email.",
    },
    FeatureDefinition {
        name: "Report archive",
        category: "Reporting & Export",
        implemented: false,
        description: "Store generated reports.",
    },
    FeatureDefinition {
        name: "Workspace layouts",
        category: "UI & Workspace",
        implemented: false,
        description: "Save and switch workspace layouts.",
    },
    FeatureDefinition {
        name: "Dockable panels",
        category: "UI & Workspace",
        implemented: false,
        description: "Dock and float tool panels.",
    },
    FeatureDefinition {
        name: "Tabbed results",
        category: "UI & Workspace",
        implemented: false,
        description: "Show multiple result tabs.",
    },
    FeatureDefinition {
        name: "Custom keyboard shortcuts",
        category: "UI & Workspace",
        implemented: false,
        description: "Customize shortcuts for commands.",
    },
    FeatureDefinition {
        name: "Theme switcher",
        category: "UI & Workspace",
        implemented: false,
        description: "Switch application themes.",
    },
    FeatureDefinition {
        name: "Session workspace restore",
        category: "UI & Workspace",
        implemented: false,
        description: "Restore open tabs on launch.",
    },
    FeatureDefinition {
        name: "Toolbar customization",
        category: "UI & Workspace",
        implemented: false,
        description: "Configure toolbar buttons.",
    },
    FeatureDefinition {
        name: "Status bar widgets",
        category: "UI & Workspace",
        implemented: false,
        description: "Add custom status indicators.",
    },
    FeatureDefinition {
        name: "Notification center",
        category: "UI & Workspace",
        implemented: false,
        description: "View non-modal alerts and messages.",
    },
    FeatureDefinition {
        name: "Session activity indicator",
        category: "UI & Workspace",
        implemented: false,
        description: "Display running query activity.",
    },
    FeatureDefinition {
        name: "Team shared snippets",
        category: "Collaboration & Versioning",
        implemented: false,
        description: "Share snippets with a team library.",
    },
    FeatureDefinition {
        name: "Git integration",
        category: "Collaboration & Versioning",
        implemented: false,
        description: "Version SQL scripts with Git.",
    },
    FeatureDefinition {
        name: "Schema compare reports",
        category: "Collaboration & Versioning",
        implemented: false,
        description: "Generate diff reports for schemas.",
    },
    FeatureDefinition {
        name: "Team dashboards",
        category: "Collaboration & Versioning",
        implemented: false,
        description: "Share project dashboards.",
    },
    FeatureDefinition {
        name: "Change request workflow",
        category: "Collaboration & Versioning",
        implemented: false,
        description: "Track change requests and approvals.",
    },
    FeatureDefinition {
        name: "User management",
        category: "Administration & Monitoring",
        implemented: false,
        description: "Create and manage database users.",
    },
    FeatureDefinition {
        name: "Role management",
        category: "Administration & Monitoring",
        implemented: false,
        description: "Create and manage roles.",
    },
    FeatureDefinition {
        name: "Storage manager",
        category: "Administration & Monitoring",
        implemented: false,
        description: "Monitor tablespace usage.",
    },
    FeatureDefinition {
        name: "Database health dashboard",
        category: "Administration & Monitoring",
        implemented: false,
        description: "Summarize health and alerts.",
    },
    FeatureDefinition {
        name: "Alert log viewer",
        category: "Administration & Monitoring",
        implemented: false,
        description: "View Oracle alert log entries.",
    },
    FeatureDefinition {
        name: "Plugin SDK",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Expose APIs for plugin developers.",
    },
    FeatureDefinition {
        name: "Scripting API",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Automate actions with a scripting API.",
    },
    FeatureDefinition {
        name: "REST API integration",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Connect to REST endpoints for data.",
    },
    FeatureDefinition {
        name: "External tool launcher",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Launch external tools from the UI.",
    },
    FeatureDefinition {
        name: "Custom export plugins",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Add custom export formats.",
    },
    FeatureDefinition {
        name: "Authentication plugins",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Extend authentication providers.",
    },
    FeatureDefinition {
        name: "SQL parser plugins",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Swap SQL parsing engines.",
    },
    FeatureDefinition {
        name: "Custom data providers",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Integrate non-Oracle data sources.",
    },
    FeatureDefinition {
        name: "UI extension points",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Extend menus and panels with plugins.",
    },
    FeatureDefinition {
        name: "Telemetry and usage analytics",
        category: "Integration & Extensibility",
        implemented: false,
        description: "Collect usage metrics (opt-in).",
    },
];
