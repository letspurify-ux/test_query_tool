# CLAUDE.md - AI Assistant Guide for Oracle Query Tool

This document provides comprehensive guidance for AI assistants working on the Oracle Query Tool codebase. It covers architecture, conventions, workflows, and best practices specific to this project.

## Project Overview

**Oracle Query Tool** is a Toad-like database client built with Rust and FLTK. It provides a desktop GUI for Oracle database management, featuring SQL editing, query execution, schema browsing, and various productivity features.

**Key Technologies:**
- **Language:** Rust (edition 2021)
- **GUI Framework:** FLTK 1.4 (cross-platform)
- **Database:** Oracle 0.6 driver
- **Serialization:** serde/serde_json for configuration
- **Build:** Cargo with LTO optimizations in release mode

**Project Goals:**
- Lightweight desktop Oracle client
- Toad-like feature parity (200+ features cataloged)
- Dark-themed modern UI
- Fast query execution and schema browsing

## Architecture Overview

### Module Structure

```
src/
├── main.rs                      # Entry point - initializes App
├── app.rs                       # Application coordinator
├── db/                          # Database layer
│   ├── mod.rs
│   ├── connection.rs            # Connection management (ConnectionInfo, DatabaseConnection)
│   └── query.rs                 # Query execution (QueryExecutor, ObjectBrowser)
├── ui/                          # User interface components
│   ├── mod.rs
│   ├── main_window.rs           # Main application window & orchestration
│   ├── sql_editor.rs            # SQL editor widget with toolbar
│   ├── result_table.rs          # Custom table for query results
│   ├── object_browser.rs        # Schema object tree browser
│   ├── syntax_highlight.rs      # SQL lexer & syntax highlighting
│   ├── intellisense.rs          # Autocomplete system
│   ├── connection_dialog.rs     # Connection UI dialog
│   ├── query_history.rs         # Query history viewer
│   ├── find_replace.rs          # Find/replace dialog
│   ├── feature_catalog.rs       # Feature tracking UI
│   └── menu.rs                  # Menu bar builder
└── utils/                       # Utilities
    ├── mod.rs
    ├── config.rs                # AppConfig & QueryHistory persistence
    └── feature_catalog.rs       # Feature tracking data structures
```

### Data Flow

```
User Input (UI)
    ↓
MainWindow (orchestration)
    ↓
SqlEditor / ObjectBrowser
    ↓
QueryExecutor / ObjectBrowser (db layer)
    ↓
DatabaseConnection (Arc<Mutex>)
    ↓
Oracle Database
    ↓
QueryResult / Metadata
    ↓
ResultTable / IntellisenseData
    ↓
UI Update
```

### Key Design Patterns

1. **Shared State Pattern**
   - UI state: `Rc<RefCell<T>>` for single-threaded shared mutable state
   - Database connection: `Arc<Mutex<DatabaseConnection>>` for thread-safe sharing
   - Clone-before-move for FLTK callbacks (requires 'static lifetime)

2. **Component Pattern**
   - Custom widgets wrap FLTK primitives
   - Provide `clone()` method (derived from FLTK widget)
   - Expose internal widgets via accessors
   - Example: `SqlEditorWidget` contains Flex + TextEditor + Buttons

3. **Callback Forwarding**
   - Menu: Single callback with pattern matching on menu path
   - Widgets: Callbacks notify parent with data payloads
   - Example: SqlEditor executes SQL → calls callback with QueryResult

4. **Configuration Persistence**
   - Lazy-loaded on startup
   - Auto-saved on changes
   - Platform-specific paths via `dirs` crate

## Key Components Deep Dive

### 1. Database Layer (`db/`)

#### Connection Management (`connection.rs`)

**Core Types:**
- `ConnectionInfo` - Serializable connection parameters
  - Fields: name, username, password, host, port, service_name
  - Connection string format: `//host:port/service_name`

- `DatabaseConnection` - Wraps Oracle connection with state tracking
  - Methods: `connect()`, `disconnect()`, `is_connected()`, `get_connection()`

- `SharedConnection` - Type alias for `Arc<Mutex<DatabaseConnection>>`
  - Used throughout UI to share connection safely

**Important Notes:**
- Passwords stored in plain text in config (security consideration)
- Connection pooling not implemented (single connection)
- Synchronous connection (blocks UI thread)

#### Query Execution (`query.rs`)

**QueryExecutor:**
- `execute_batch(sql, conn)` - Main entry point for SQL execution
  - Splits SQL by semicolons (respects quotes/comments)
  - Detects statement type: SELECT/DML/DDL
  - Returns `QueryResult` with rows/columns or affected count

- Statement splitting algorithm:
  ```rust
  // Handles: 'strings', "identifiers", -- comments, /* block comments */
  // Respects escaped quotes: 'It''s working'
  // Splits on semicolons outside strings/comments
  ```

- `get_explain_plan(sql, conn)` - Execution plan retrieval
  - Uses `EXPLAIN PLAN FOR` + `DBMS_XPLAN.DISPLAY`

**ObjectBrowser:**
- Schema metadata queries for: Tables, Views, Procedures, Functions, Sequences
- DDL generation via `DBMS_METADATA.GET_DDL`
- Table structure queries (columns, types, nullability)
- Index and constraint queries

**Query Methods:**
- `get_tables()`, `get_views()`, `get_procedures()`, `get_functions()`, `get_sequences()`
- `get_table_structure(table_name)`
- `get_table_indexes(table_name)`
- `get_table_constraints(table_name)`
- `get_ddl(object_type, object_name)`

### 2. UI Layer (`ui/`)

#### MainWindow (`main_window.rs`)

**Responsibilities:**
- Application orchestration
- Menu callback handling (centralized pattern matching)
- Layout management (menu + toolbar + tile + statusbar)
- Connection state management
- Intellisense/highlighting data coordination

**Layout Structure:**
```
Window (1024x768)
 ├── MenuBar
 ├── Toolbar (placeholder for future)
 ├── Tile (resizable splitter)
 │   ├── ObjectBrowser (left, 250px)
 │   └── Flex (right)
 │       ├── SqlEditor (top)
 │       └── ResultTable (bottom)
 └── StatusBar
```

**Theme Colors:**
- Background: RGB(45, 45, 48) - Dark gray
- Text: RGB(220, 220, 220) - Light gray
- Accent: RGB(0, 122, 204) - Blue
- Selection: RGB(51, 153, 255) - Bright blue

**Menu Callback Pattern:**
```rust
menu.set_callback(move |m| {
    if let Some(item) = m.find_item(m.value()) {
        let label = item.label().unwrap_or_default();
        match label.as_str() {
            "Connect\t" => { /* handle connect */ }
            "Execute\tF5" => { /* handle execute */ }
            // ...
        }
    }
});
```

#### SqlEditor (`sql_editor.rs`)

**Features:**
- TextEditor with syntax highlighting
- Toolbar: Execute, Explain, Clear, Commit, Rollback
- Keyboard shortcuts: F5 (execute), Ctrl+Space (intellisense), F4 (quick describe)
- Real-time syntax highlighting on KeyUp
- Auto-save to query history

**Event Handling:**
```rust
editor.handle(move |e, ev| match ev {
    Event::KeyUp => { /* trigger syntax highlighting */ }
    Event::KeyDown => {
        match e.event_key() {
            Key::F5 => { /* execute query */ }
            // ...
        }
    }
    _ => false,
})
```

**Execute Flow:**
1. Get SQL from editor (selected text or all)
2. Call `QueryExecutor::execute_batch()`
3. Build `QueryResult` with execution time
4. Invoke callback to notify MainWindow
5. Add to query history

#### Syntax Highlighting (`syntax_highlight.rs`)

**Style System:**
- Character-based: Each char in TextBuffer has corresponding style byte
- Style types (A-I): Default, Keyword, Function, String, Comment, Number, Operator, Identifier
- Colors follow VS Code dark theme (blue keywords, orange strings, green comments)

**Lexer Algorithm:**
1. Iterate characters in SQL text
2. Detect tokens: strings ('...'), comments (-- and /* */), numbers, words
3. Match words against keyword/function lists
4. Apply style to corresponding positions in style buffer
5. Update TextEditor with new style buffer

**Schema-Aware Highlighting:**
- `HighlightData` contains table/view/column names from schema
- Identifiers matching schema objects get special highlighting
- Updated when connection changes

**Important:** Highlighting is CPU-intensive on large SQL. Consider debouncing for >10k chars.

#### Intellisense (`intellisense.rs`)

**Data Structure:**
```rust
IntellisenseData {
    tables: Vec<String>,
    views: Vec<String>,
    procedures: Vec<String>,
    functions: Vec<String>,
    columns: HashMap<String, Vec<String>>, // table -> columns
}
```

**Popup Behavior:**
- Borderless window positioned at cursor
- HoldBrowser with max 50 suggestions
- Keyboard: Arrow keys navigate, Enter/Tab select, Escape closes
- Auto-dismiss on editor focus loss

**Suggestion Types:**
1. SQL keywords (SELECT, FROM, WHERE, etc.)
2. Oracle functions (with () suffix)
3. Schema objects (tables, views, procedures, functions)
4. Columns (when context detected - partially implemented)

**Trigger Points:**
- Manual: Ctrl+Space
- Auto: After typing 2+ characters
- Context-aware triggering not fully implemented (future enhancement)

#### ObjectBrowser (`object_browser.rs`)

**Tree Structure:**
```
📁 Tables
  └─ TABLE_NAME
📁 Views
  └─ VIEW_NAME
📁 Procedures
  └─ PROCEDURE_NAME
📁 Functions
  └─ FUNCTION_NAME
📁 Sequences
  └─ SEQUENCE_NAME
```

**Context Menu Actions:**
- "Select Data" → `SELECT * FROM ... WHERE ROWNUM <= 100`
- "View Structure" → Shows columns, types, nullability
- "View Indexes" → Shows indexes for table
- "View Constraints" → Shows constraints for table
- "Generate DDL" → Uses DBMS_METADATA

**Double-Click Behavior:**
- Tables/Views: Insert SELECT statement into editor
- Procedures/Functions: Show in editor (future: show source code)

**Filter Feature:**
- Input at top filters visible objects
- Case-insensitive substring match
- Caches all objects for fast filtering

#### ResultTable (`result_table.rs`)

**Custom Table Implementation:**
- Inherits from FLTK Table
- Custom `draw_cell()` callback for complete rendering control
- Features: Headers, alternating row colors, selection, copying

**Draw Cell Logic:**
```rust
fn draw_cell(
    &mut self,
    ctx: TableContext,
    row: i32,
    col: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    match ctx {
        TableContext::Cell => { /* draw data cell */ }
        TableContext::ColHeader => { /* draw column header */ }
        TableContext::RowHeader => { /* draw row number */ }
        _ => {}
    }
}
```

**Copy Operations:**
- Copy: Selected cells as TSV
- Copy with Headers: Include column names
- Copy Cell: Single cell value
- Copy All: Entire result set

**CSV Export:**
- Proper escaping (quotes, commas, newlines)
- Header row included
- RFC 4180 compliant

**Column Auto-sizing:**
- Measures text width using FLTK's `measure()`
- Adds padding (20px)
- Caps at 300px max width
- Updates on new results

### 3. Utils Layer (`utils/`)

#### Configuration (`config.rs`)

**AppConfig Structure:**
```json
{
  "recent_connections": [
    {
      "name": "Production",
      "username": "user",
      "password": "pass",
      "host": "localhost",
      "port": 1521,
      "service_name": "ORCL"
    }
  ],
  "last_connection": "Production",
  "editor_font_size": 14,
  "result_font_size": 12,
  "max_rows": 1000,
  "auto_commit": false
}
```

**Config Path:**
- Linux/Mac: `~/.config/oracle_query_tool/config.json`
- Windows: `%APPDATA%\oracle_query_tool\config.json`

**QueryHistory Structure:**
```json
{
  "queries": [
    {
      "sql": "SELECT * FROM employees",
      "timestamp": "2026-01-20T10:30:00",
      "execution_time_ms": 125,
      "row_count": 100,
      "success": true
    }
  ]
}
```

**History Path:**
- Linux/Mac: `~/.local/share/oracle_query_tool/history.json`
- Windows: `%APPDATA%\Local\oracle_query_tool\history.json`

**Persistence Strategy:**
- Load on app start (graceful fallback to defaults on error)
- Save after connection changes
- Save after each query execution (history)
- Max 1000 queries in history (FIFO)

## Development Workflows

### Adding a New Menu Item

1. **Update `ui/menu.rs`:**
   ```rust
   pub fn build_menu() -> MenuBar {
       // ...
       file_menu.add("New Item\tCtrl+K", Shortcut::Ctrl | 'k', /* ... */);
   }
   ```

2. **Handle in `ui/main_window.rs`:**
   ```rust
   match label.as_str() {
       "New Item\t" => {
           // Handle action
       }
       // ...
   }
   ```

### Adding a New Dialog

1. **Create `ui/my_dialog.rs`:**
   ```rust
   use fltk::*;

   pub struct MyDialog {
       window: window::Window,
       // ... fields
   }

   impl MyDialog {
       pub fn new() -> Self {
           let mut window = window::Window::default()
               .with_size(400, 300)
               .with_label("My Dialog");
           window.make_modal(true);
           // ... build UI
           window.end();
           Self { window }
       }

       pub fn show(&mut self) -> Option<ReturnValue> {
           self.window.show();
           while self.window.shown() {
               app::wait();
           }
           // Return result
       }
   }
   ```

2. **Export from `ui/mod.rs`:**
   ```rust
   pub mod my_dialog;
   pub use my_dialog::MyDialog;
   ```

3. **Use in `main_window.rs`:**
   ```rust
   let mut dialog = MyDialog::new();
   if let Some(result) = dialog.show() {
       // Handle result
   }
   ```

### Adding a New Query Type to ObjectBrowser

1. **Update `db/query.rs`:**
   ```rust
   impl ObjectBrowser {
       pub fn get_my_objects(conn: &Connection) -> Result<Vec<String>, oracle::Error> {
           let mut stmt = conn.statement("SELECT object_name FROM user_objects WHERE object_type = 'MY_TYPE' ORDER BY object_name").build()?;
           let rows = stmt.query(&[])?;
           // ... collect results
       }
   }
   ```

2. **Update `ui/object_browser.rs`:**
   ```rust
   // Add category in constructor
   my_objects_node = tree.add(&root, "My Objects");

   // Load in refresh method
   if let Ok(objects) = ObjectBrowser::get_my_objects(conn) {
       for obj in objects {
           tree.add(&my_objects_node, &obj);
       }
   }

   // Handle in context menu
   if selected_text.parent().label() == Some("My Objects".into()) {
       // Add custom menu items
   }
   ```

### Adding Syntax Highlighting for New Keywords

1. **Update `ui/syntax_highlight.rs`:**
   ```rust
   const KEYWORDS: &[&str] = &[
       // ... existing keywords
       "MYNEWKEYWORD",
   ];
   ```

2. **For functions:**
   ```rust
   const FUNCTIONS: &[&str] = &[
       // ... existing functions
       "MY_NEW_FUNCTION",
   ];
   ```

3. **For new style type (advanced):**
   ```rust
   const STYLE_COUNT: usize = 10; // Increase

   pub enum StyleType {
       // ... existing styles
       MyNewStyle = b'J' as isize,
   }

   pub fn create_style_table() -> Vec<StyleTableEntry> {
       vec![
           // ...
           StyleTableEntry { color: Color::from_rgb(r, g, b), font: Font::Courier, size: 12 },
       ]
   }
   ```

### Adding a New Export Format

1. **Update `ui/result_table.rs`:**
   ```rust
   impl ResultTableWidget {
       pub fn export_as_json(&self) -> String {
           // Implement JSON export
           let mut output = String::from("[\n");
           for row in &self.data {
               output.push_str("  {");
               for (i, (col, val)) in self.columns.iter().zip(row).enumerate() {
                   output.push_str(&format!("\"{}\": \"{}\"", col, val));
                   if i < row.len() - 1 {
                       output.push_str(", ");
                   }
               }
               output.push_str("},\n");
           }
           output.push_str("]\n");
           output
       }
   }
   ```

2. **Add menu item and handler:**
   ```rust
   // In main_window.rs menu callback
   "Export as JSON\t" => {
       if let Some(data) = result_table.export_as_json() {
           // Save to file
       }
   }
   ```

## Code Conventions

### Naming Conventions

- **Modules:** Snake_case (`syntax_highlight.rs`)
- **Types:** PascalCase (`QueryExecutor`, `IntellisenseData`)
- **Functions:** Snake_case (`execute_batch`, `get_tables`)
- **Constants:** SCREAMING_SNAKE_CASE (`KEYWORDS`, `STYLE_COUNT`)
- **Widget suffix:** Custom widgets end with `Widget` (`SqlEditorWidget`)

### Error Handling

**Database Errors:**
```rust
// Return Result, let UI handle display
match QueryExecutor::execute_batch(sql, conn) {
    Ok(result) => { /* update UI */ }
    Err(e) => {
        fltk::dialog::alert_default(&format!("Error: {}", e));
    }
}
```

**Config Errors:**
```rust
// Graceful degradation
let config = AppConfig::load().unwrap_or_default();
```

**Query Errors:**
```rust
// Convert to QueryResult with error flag
QueryResult {
    success: false,
    message: Some(format!("Error: {}", e)),
    // ...
}
```

### Widget Cloning Pattern

```rust
// Clone all needed state before moving into closure
let mut my_widget = my_widget.clone();
let my_state = Rc::clone(&my_state);

button.set_callback(move |_| {
    my_widget.set_label("Clicked");
    let mut state = my_state.borrow_mut();
    state.count += 1;
});
```

### Shared State Pattern

**Single-threaded (UI state):**
```rust
let state = Rc::new(RefCell::new(MyState::default()));

// Clone and share
let state_clone = Rc::clone(&state);
button.set_callback(move |_| {
    let mut s = state_clone.borrow_mut();
    s.modify();
});
```

**Multi-threaded (database):**
```rust
let conn = Arc::new(Mutex::new(DatabaseConnection::new()));

// Clone and share
let conn_clone = Arc::clone(&conn);
std::thread::spawn(move || {
    let mut c = conn_clone.lock().unwrap();
    c.connect(/* ... */);
});
```

### Comment Style

- Use `//` for line comments
- Use `///` for doc comments on public items
- Explain "why" not "what" (code should be self-documenting)
- Mark TODOs: `// TODO: description`
- Mark known issues: `// FIXME: description`

### Formatting

- Use `cargo fmt` before committing
- Max line length: 100 characters (soft limit)
- 4-space indentation (default Rust)

## Testing and Debugging

### Running the Application

```bash
# Development build (fast compile, slow runtime)
cargo run

# Release build (slow compile, fast runtime)
cargo build --release
./target/release/oracle_query_tool
```

### Common Issues

**Issue: "Error connecting to database"**
- Check Oracle client libraries installed (Oracle Instant Client)
- Verify `LD_LIBRARY_PATH` (Linux) or `PATH` (Windows) includes Oracle lib directory
- Test connection string format: `//host:port/service_name`

**Issue: "Syntax highlighting not working"**
- Check if `highlight_sql()` is being called on KeyUp
- Verify style table initialization in `create_style_table()`
- Debug: Print style buffer length vs text buffer length (must match)

**Issue: "Intellisense popup appears but is empty"**
- Check if `IntellisenseData` was loaded after connection
- Verify `get_suggestions()` returns results
- Debug: Print `data.tables.len()` to confirm schema metadata loaded

**Issue: "UI freezes during long query"**
- Expected behavior (synchronous execution on UI thread)
- Future enhancement: Move query execution to background thread
- Workaround: Use smaller result sets or `ROWNUM` limits

### Debugging with println!

```rust
// Quick debug in UI callback
button.set_callback(move |_| {
    println!("Button clicked, state: {:?}", state.borrow());
});

// Debug query execution
pub fn execute_batch(sql: &str, conn: &Connection) -> Result<QueryResult> {
    println!("Executing SQL: {}", sql);
    let start = std::time::Instant::now();
    // ...
    println!("Execution took: {:?}", start.elapsed());
}
```

### Logging (Future Enhancement)

Currently no logging framework. Consider adding:
- `env_logger` or `tracing` for structured logging
- Log levels: ERROR (user-facing errors), WARN (recoverable), INFO (major actions), DEBUG (development)

## Important Gotchas

### FLTK-Specific

1. **Callbacks require 'static lifetime**
   - Must clone all state before moving into closure
   - Cannot capture references (use Rc/Arc instead)

2. **Widget must be shown before event loop**
   ```rust
   window.end();
   window.show();
   app.run().unwrap(); // Event loop
   ```

3. **Modal dialogs block with app::wait()**
   ```rust
   dialog.make_modal(true);
   dialog.show();
   while dialog.shown() {
       app::wait(); // Process events
   }
   ```

4. **TextEditor style buffer must match text length**
   - Every character needs a style byte
   - Mismatch causes crashes or rendering issues

### Oracle-Specific

1. **Connection string format is specific**
   - Correct: `//host:port/service_name`
   - Wrong: `host:port/sid` (SID not supported)

2. **DBMS_METADATA requires permissions**
   - `SELECT_CATALOG_ROLE` or `SELECT ANY DICTIONARY`
   - Fallback to `USER_TABLES` if permission denied

3. **Statement splitting is not perfect**
   - Handles most cases but may fail on complex PL/SQL blocks
   - Known limitation: Nested strings with embedded semicolons

4. **DBMS_OUTPUT requires buffer setup**
   - Must call `DBMS_OUTPUT.ENABLE(buffer_size)`
   - Currently not fully implemented

### Rust-Specific

1. **RefCell borrow panics if already borrowed mutably**
   ```rust
   let mut a = state.borrow_mut();
   let b = state.borrow(); // PANIC! Already mutably borrowed
   ```
   - Solution: Minimize borrow scope, drop before next borrow

2. **Mutex lock() returns Result**
   - Handle or unwrap: `conn.lock().unwrap()`
   - Can panic if mutex poisoned (previous thread panicked while holding lock)

3. **String vs &str conversions**
   - Use `.to_string()` or `.to_owned()` to convert &str → String
   - Use `.as_str()` or `&s` to convert String → &str

## Feature Catalog System

The project includes a self-documenting feature tracking system located in `ui/feature_catalog.rs` and `utils/feature_catalog.rs`.

### Purpose
- Track implementation status of 200+ Toad-like features
- Provide roadmap visibility
- Allow filtering by keyword and status

### Usage
- Menu: Help → Feature Catalog
- Filter by keyword or status (All/Implemented/Planned)
- Shows progress: "Implemented: X/Y"

### Adding New Features

1. **Update `utils/feature_catalog.rs`:**
   ```rust
   features.push(Feature {
       category: "Performance".to_string(),
       name: "Query Profiling".to_string(),
       description: "Profile query execution to identify bottlenecks".to_string(),
       implemented: false,
   });
   ```

2. **Or add to `toad_manual_features.json`:**
   ```json
   [
     {
       "category": "Performance",
       "name": "Query Profiling",
       "description": "Profile query execution to identify bottlenecks",
       "implemented": false
     }
   ]
   ```

3. **Mark as implemented when complete:**
   ```rust
   implemented: true,
   ```

## Performance Considerations

### UI Responsiveness
- **Problem:** Long queries block UI thread
- **Solution:** Consider moving query execution to background thread with `std::thread::spawn` or `tokio`
- **Tradeoff:** Adds complexity with thread communication

### Syntax Highlighting
- **Problem:** Slow on large SQL (>10k characters)
- **Solution:** Debounce highlighting (e.g., 300ms delay after typing stops)
- **Alternative:** Only highlight visible portion of editor

### Object Browser
- **Problem:** Loading thousands of objects is slow
- **Solution:** Currently caches all objects, filters in memory (fast)
- **Future:** Virtual tree with lazy loading

### Result Table
- **Problem:** Displaying 10k+ rows is slow
- **Solution:** Virtual table with draw_cell rendering only visible cells
- **Current limit:** Max 1000 rows (configurable in AppConfig)

## Security Considerations

1. **Passwords stored in plain text**
   - Location: `~/.config/oracle_query_tool/config.json`
   - Risk: Anyone with file access can read passwords
   - Future: Encrypt passwords with OS keyring (e.g., `keyring-rs`)

2. **SQL Injection**
   - Not applicable (user is DBA, executing their own SQL)
   - No web interface or parameterized queries needed

3. **Connection security**
   - Currently no SSL/TLS configuration exposed
   - Oracle driver supports encrypted connections (TNS configuration)

## Git Workflow

### Branch Naming
- Feature branches: `claude/feature-name-<session-id>`
- Branch must start with `claude/` and end with session ID for push to succeed

### Committing
1. Review changes: `git status` and `git diff`
2. Stage relevant files: `git add <files>`
3. Commit with descriptive message: `git commit -m "Add feature X"`
4. Push to feature branch: `git push -u origin <branch-name>`

### Pull Requests
- Target branch: `main` (unless specified otherwise)
- Include summary of changes
- Reference issue numbers if applicable

## Common Tasks Reference

### Connect to Database
```rust
use crate::db::connection::{ConnectionInfo, DatabaseConnection};

let info = ConnectionInfo {
    name: "Test".to_string(),
    username: "user".to_string(),
    password: "pass".to_string(),
    host: "localhost".to_string(),
    port: 1521,
    service_name: "ORCL".to_string(),
};

let mut db_conn = DatabaseConnection::new();
db_conn.connect(&info)?;
```

### Execute Query
```rust
use crate::db::query::QueryExecutor;

let sql = "SELECT * FROM employees WHERE department = 'IT'";
let conn = db_conn.get_connection().unwrap();
let result = QueryExecutor::execute_batch(sql, conn)?;

println!("Rows: {}", result.rows.len());
println!("Columns: {:?}", result.columns);
```

### Update Syntax Highlighting
```rust
use crate::ui::syntax_highlight::{highlight_sql, HighlightData};

let sql = editor.buffer().unwrap().text();
let highlight_data = HighlightData::default(); // Or load from schema
highlight_sql(&mut editor, &sql, &highlight_data);
```

### Show Alert Dialog
```rust
use fltk::dialog;

dialog::alert_default("This is an alert message");
```

### Show Choice Dialog
```rust
use fltk::dialog;

let choice = dialog::choice2_default("Choose an option:", "Option 1", "Option 2", "Cancel");
// Returns: 0 (Option 1), 1 (Option 2), 2 (Cancel), or -1 (closed)
```

## Quick Reference

### Key Shortcuts
- `F5` - Execute SQL
- `F4` - Quick Describe (table structure)
- `F7` - Commit
- `F8` - Rollback
- `Ctrl+Space` - Intellisense
- `Ctrl+N` - New Connection
- `Ctrl+F` - Find
- `Ctrl+H` - Replace
- `Ctrl+C` - Copy (in results table)

### Important File Locations
- Config: `~/.config/oracle_query_tool/config.json`
- History: `~/.local/share/oracle_query_tool/history.json`
- Features: `toad_manual_features.json` (project root)

### Common FLTK Widgets
- `Window` - Top-level window
- `TextEditor` - Multi-line text editing
- `Table` - Grid/table display
- `Tree` - Hierarchical tree view
- `MenuBar` - Menu bar
- `Button` - Clickable button
- `Input` - Single-line text input
- `Browser` - List/selection widget
- `Flex` - Flexible box layout
- `Tile` - Resizable split layout

### Common Oracle Queries
```sql
-- List tables
SELECT table_name FROM user_tables ORDER BY table_name;

-- List views
SELECT view_name FROM user_views ORDER BY view_name;

-- List procedures
SELECT object_name FROM user_objects WHERE object_type = 'PROCEDURE' ORDER BY object_name;

-- Table structure
SELECT column_name, data_type, nullable FROM user_tab_columns WHERE table_name = 'TABLE_NAME';

-- Get DDL
SELECT DBMS_METADATA.GET_DDL('TABLE', 'TABLE_NAME') FROM DUAL;
```

## Future Enhancements (From Feature Catalog)

**High Priority:**
- [ ] Async query execution (background thread)
- [ ] Session management (multiple connections)
- [ ] Encrypted password storage
- [ ] PL/SQL debugging
- [ ] Export to Excel/XML/HTML

**Medium Priority:**
- [ ] Code folding in editor
- [ ] Query execution plans (visual)
- [ ] Table data editing (inline)
- [ ] Script recording/playback
- [ ] Schema compare

**Low Priority:**
- [ ] ER diagram generation
- [ ] Query builder (visual)
- [ ] Team collaboration features
- [ ] Plugin system

## Resources

### Documentation
- FLTK Rust: https://docs.rs/fltk/latest/fltk/
- Oracle Rust driver: https://docs.rs/oracle/latest/oracle/
- Rust Book: https://doc.rust-lang.org/book/

### Useful Tools
- `cargo fmt` - Format code
- `cargo clippy` - Lint code
- `cargo doc --open` - Generate and open docs
- `cargo tree` - Show dependency tree

## Questions & Support

For questions about this codebase:
1. Check this CLAUDE.md file
2. Review the Feature Catalog (Help → Feature Catalog in app)
3. Examine similar existing implementations
4. Check recent git commits for context

---

**Last Updated:** 2026-01-20
**Version:** 0.1.0
**Maintained By:** Development Team
