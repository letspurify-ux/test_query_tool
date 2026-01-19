use fltk::{
    app,
    enums::{Color, Font, FrameType},
    frame::Frame,
    group::{Flex, FlexType, Pack, PackType, Tile},
    menu::MenuBar,
    prelude::*,
    window::Window,
};

use crate::db::{create_shared_connection, ObjectBrowser, SharedConnection};
use crate::ui::{
    ConnectionDialog, IntellisenseData, MenuBarBuilder, ObjectBrowserWidget, ResultTableWidget,
    SqlEditorWidget,
};

pub struct MainWindow {
    window: Window,
    connection: SharedConnection,
    sql_editor: SqlEditorWidget,
    result_table: ResultTableWidget,
    object_browser: ObjectBrowserWidget,
    status_bar: Frame,
}

impl MainWindow {
    pub fn new() -> Self {
        let connection = create_shared_connection();

        let mut window = Window::default()
            .with_size(1200, 800)
            .with_label("Oracle Query Tool - Rust Edition");
        window.set_color(Color::from_rgb(45, 45, 48));

        let mut main_flex = Flex::default_fill();
        main_flex.set_type(FlexType::Column);
        main_flex.set_margin(0);

        // Menu bar
        let menu_bar = MenuBarBuilder::build();
        main_flex.fixed(&menu_bar, 30);

        // Toolbar
        let toolbar = Self::create_toolbar();
        main_flex.fixed(&toolbar, 35);

        // Main content area with Tile for resizable panels
        let mut content_tile = Tile::default();
        content_tile.set_color(Color::from_rgb(45, 45, 48));

        // Left panel - Object Browser (200px wide)
        let object_browser = ObjectBrowserWidget::new(0, 0, 200, 600, connection.clone());

        // Right panel - Editor and Results
        let mut right_flex = Flex::default()
            .with_pos(200, 0)
            .with_size(1000, 600);
        right_flex.set_type(FlexType::Column);
        right_flex.set_margin(5);

        // SQL Editor
        let sql_editor = SqlEditorWidget::new(connection.clone());
        let sql_group = sql_editor.get_group().clone();
        right_flex.fixed(&sql_group, 250);

        // Result Table
        let result_table = ResultTableWidget::new();

        right_flex.end();
        content_tile.end();

        // Status bar
        let mut status_bar = Frame::default()
            .with_label("Not connected | Ctrl+Space for autocomplete");
        status_bar.set_frame(FrameType::FlatBox);
        status_bar.set_color(Color::from_rgb(0, 122, 204));
        status_bar.set_label_color(Color::White);
        status_bar.set_label_font(Font::Helvetica);
        status_bar.set_label_size(12);
        main_flex.fixed(&status_bar, 25);

        main_flex.end();
        window.end();

        window.make_resizable(true);

        Self {
            window,
            connection,
            sql_editor,
            result_table,
            object_browser,
            status_bar,
        }
    }

    fn create_toolbar() -> Pack {
        let mut toolbar = Pack::default()
            .with_size(0, 35);
        toolbar.set_type(PackType::Horizontal);
        toolbar.set_spacing(5);
        toolbar.set_color(Color::from_rgb(60, 60, 63));

        // Spacer
        let mut spacer = Frame::default().with_size(10, 35);
        spacer.set_frame(FrameType::FlatBox);
        spacer.set_color(Color::from_rgb(60, 60, 63));

        toolbar.end();
        toolbar
    }

    pub fn setup_callbacks(&mut self) {
        let mut status_bar = self.status_bar.clone();
        let mut result_table = self.result_table.clone();

        // Setup SQL editor execute callback
        self.sql_editor.set_execute_callback(move |query_result| {
            result_table.display_result(&query_result);

            let status_text = format!(
                "{} | Time: {:.3}s",
                query_result.message,
                query_result.execution_time.as_secs_f64()
            );
            status_bar.set_label(&status_text);
        });

        // Setup menu callbacks
        self.setup_menu_callbacks();
    }

    fn setup_menu_callbacks(&mut self) {
        let connection = self.connection.clone();
        let mut status_bar = self.status_bar.clone();
        let mut object_browser = self.object_browser.clone();
        let intellisense_data = self.sql_editor.get_intellisense_data();

        // Find menu bar and set callbacks
        if let Some(mut menu) = app::widget_from_id::<MenuBar>("main_menu") {
            menu.set_callback(move |m| {
                if let Some(path) = m.choice() {
                    match path.as_str() {
                        "&File/&Connect...\t" => {
                            if let Some(info) = ConnectionDialog::show() {
                                let mut db_conn = connection.lock().unwrap();
                                match db_conn.connect(info.clone()) {
                                    Ok(_) => {
                                        status_bar.set_label(&format!(
                                            "Connected: {} | Ctrl+Space for autocomplete",
                                            info.display_string()
                                        ));

                                        // Update intellisense data
                                        if let Some(conn) = db_conn.get_connection() {
                                            let mut data = IntellisenseData::new();

                                            // Load tables
                                            if let Ok(tables) = ObjectBrowser::get_tables(conn) {
                                                data.tables = tables;
                                            }

                                            // Load views
                                            if let Ok(views) = ObjectBrowser::get_views(conn) {
                                                data.views = views;
                                            }

                                            // Load procedures
                                            if let Ok(procs) = ObjectBrowser::get_procedures(conn) {
                                                data.procedures = procs;
                                            }

                                            // Load functions
                                            if let Ok(funcs) = ObjectBrowser::get_functions(conn) {
                                                data.functions = funcs;
                                            }

                                            // Load columns for each table
                                            for table in &data.tables.clone() {
                                                if let Ok(cols) =
                                                    ObjectBrowser::get_table_columns(conn, table)
                                                {
                                                    let col_names: Vec<String> =
                                                        cols.into_iter().map(|c| c.name).collect();
                                                    data.columns.push((table.clone(), col_names));
                                                }
                                            }

                                            *intellisense_data.borrow_mut() = data;
                                        }

                                        drop(db_conn);
                                        object_browser.refresh();
                                    }
                                    Err(e) => {
                                        fltk::dialog::alert_default(&format!(
                                            "Connection failed: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        "&File/&Disconnect\t" => {
                            let mut db_conn = connection.lock().unwrap();
                            db_conn.disconnect();
                            status_bar.set_label("Disconnected | Ctrl+Space for autocomplete");

                            // Clear intellisense data
                            *intellisense_data.borrow_mut() = IntellisenseData::new();
                        }
                        "&File/E&xit\t" => {
                            app::quit();
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    pub fn show(&mut self) {
        self.window.show();
    }

    pub fn run() {
        let app = app::App::default().with_scheme(app::Scheme::Gtk);

        // Set default colors for dark theme
        app::background(45, 45, 48);
        app::foreground(220, 220, 220);

        let mut main_window = MainWindow::new();
        main_window.setup_callbacks();
        main_window.show();

        app.run().unwrap();
    }
}

impl Default for MainWindow {
    fn default() -> Self {
        Self::new()
    }
}
