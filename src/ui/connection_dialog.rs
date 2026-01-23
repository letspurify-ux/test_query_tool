use fltk::{
    app,
    browser::HoldBrowser,
    button::{Button, CheckButton},
    enums::{Color, FrameType},
    group::Flex,
    input::{Input, SecretInput},
    prelude::*,
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::{ConnectionInfo, DatabaseConnection};
use crate::utils::AppConfig;

pub struct ConnectionDialog;

impl ConnectionDialog {
    pub fn show() -> Option<ConnectionInfo> {
        Self::show_with_registry(Rc::new(RefCell::new(Vec::new())))
    }

    pub fn show_with_registry(popups: Rc<RefCell<Vec<Window>>>) -> Option<ConnectionInfo> {
        enum DialogMessage {
            SelectSaved(String),
            DeleteSelected,
            Test(ConnectionInfo),
            Connect(ConnectionInfo, bool),
            Cancel,
        }

        let (sender, receiver) = std::sync::mpsc::channel::<DialogMessage>();

        let result: Rc<RefCell<Option<ConnectionInfo>>> = Rc::new(RefCell::new(None));
        let config = Rc::new(RefCell::new(AppConfig::load()));

        let mut dialog = Window::default()
            .with_size(500, 420)
            .with_label("Connect to Oracle Database");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut main_flex = Flex::default()
            .with_pos(20, 20)
            .with_size(460, 380);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_margin(10);
        main_flex.set_spacing(8);

        // Saved Connections section
        let mut saved_flex = Flex::default();
        saved_flex.set_type(fltk::group::FlexType::Row);
        let mut saved_label = fltk::frame::Frame::default()
            .with_label("Saved:");
        saved_label.set_label_color(Color::White);
        saved_flex.fixed(&saved_label, 100);

        let mut saved_browser = HoldBrowser::default();
        saved_browser.set_color(Color::from_rgb(60, 60, 63));
        saved_browser.set_selection_color(Color::from_rgb(0, 122, 204));

        // Load saved connections
        {
            let cfg = config.borrow();
            for conn in cfg.get_all_connections() {
                saved_browser.add(&conn.name);
            }
        }

        let mut delete_btn = Button::default()
            .with_size(60, 25)
            .with_label("Delete");
        delete_btn.set_color(Color::from_rgb(150, 50, 50));
        delete_btn.set_label_color(Color::White);
        delete_btn.set_frame(FrameType::FlatBox);

        saved_flex.fixed(&delete_btn, 60);
        saved_flex.end();
        main_flex.fixed(&saved_flex, 80);

        // Connection Name
        let mut name_flex = Flex::default();
        name_flex.set_type(fltk::group::FlexType::Row);
        let mut name_label = fltk::frame::Frame::default()
            .with_label("Name:");
        name_label.set_label_color(Color::White);
        name_flex.fixed(&name_label, 100);
        let mut name_input = Input::default();
        name_input.set_value("My Connection");
        name_input.set_color(Color::from_rgb(60, 60, 63));
        name_input.set_text_color(Color::White);
        name_flex.end();
        main_flex.fixed(&name_flex, 30);

        // Username
        let mut user_flex = Flex::default();
        user_flex.set_type(fltk::group::FlexType::Row);
        let mut user_label = fltk::frame::Frame::default()
            .with_label("Username:");
        user_label.set_label_color(Color::White);
        user_flex.fixed(&user_label, 100);
        let mut user_input = Input::default();
        user_input.set_color(Color::from_rgb(60, 60, 63));
        user_input.set_text_color(Color::White);
        user_flex.end();
        main_flex.fixed(&user_flex, 30);

        // Password
        let mut pass_flex = Flex::default();
        pass_flex.set_type(fltk::group::FlexType::Row);
        let mut pass_label = fltk::frame::Frame::default()
            .with_label("Password:");
        pass_label.set_label_color(Color::White);
        pass_flex.fixed(&pass_label, 100);
        let mut pass_input = SecretInput::default();
        pass_input.set_color(Color::from_rgb(60, 60, 63));
        pass_input.set_text_color(Color::White);
        pass_flex.end();
        main_flex.fixed(&pass_flex, 30);

        // Host
        let mut host_flex = Flex::default();
        host_flex.set_type(fltk::group::FlexType::Row);
        let mut host_label = fltk::frame::Frame::default()
            .with_label("Host:");
        host_label.set_label_color(Color::White);
        host_flex.fixed(&host_label, 100);
        let mut host_input = Input::default();
        host_input.set_value("localhost");
        host_input.set_color(Color::from_rgb(60, 60, 63));
        host_input.set_text_color(Color::White);
        host_flex.end();
        main_flex.fixed(&host_flex, 30);

        // Port
        let mut port_flex = Flex::default();
        port_flex.set_type(fltk::group::FlexType::Row);
        let mut port_label = fltk::frame::Frame::default()
            .with_label("Port:");
        port_label.set_label_color(Color::White);
        port_flex.fixed(&port_label, 100);
        let mut port_input = Input::default();
        port_input.set_value("1521");
        port_input.set_color(Color::from_rgb(60, 60, 63));
        port_input.set_text_color(Color::White);
        port_flex.end();
        main_flex.fixed(&port_flex, 30);

        // Service Name
        let mut service_flex = Flex::default();
        service_flex.set_type(fltk::group::FlexType::Row);
        let mut service_label = fltk::frame::Frame::default()
            .with_label("Service:");
        service_label.set_label_color(Color::White);
        service_flex.fixed(&service_label, 100);
        let mut service_input = Input::default();
        service_input.set_value("ORCL");
        service_input.set_color(Color::from_rgb(60, 60, 63));
        service_input.set_text_color(Color::White);
        service_flex.end();
        main_flex.fixed(&service_flex, 30);

        // Save connection checkbox
        let mut save_flex = Flex::default();
        save_flex.set_type(fltk::group::FlexType::Row);
        let _spacer = fltk::frame::Frame::default();
        save_flex.fixed(&_spacer, 100);
        let mut save_check = CheckButton::default()
            .with_label("Save this connection");
        save_check.set_label_color(Color::White);
        save_check.set_value(true);
        save_flex.end();
        main_flex.fixed(&save_flex, 25);

        // Buttons
        let mut button_flex = Flex::default();
        button_flex.set_type(fltk::group::FlexType::Row);
        button_flex.set_spacing(10);

        let _spacer = fltk::frame::Frame::default();

        let mut test_btn = Button::default()
            .with_size(80, 30)
            .with_label("Test");
        test_btn.set_color(Color::from_rgb(104, 33, 122));
        test_btn.set_label_color(Color::White);
        test_btn.set_frame(FrameType::FlatBox);

        let mut connect_btn = Button::default()
            .with_size(80, 30)
            .with_label("Connect");
        connect_btn.set_color(Color::from_rgb(0, 122, 204));
        connect_btn.set_label_color(Color::White);
        connect_btn.set_frame(FrameType::FlatBox);

        let mut cancel_btn = Button::default()
            .with_size(80, 30)
            .with_label("Cancel");
        cancel_btn.set_color(Color::from_rgb(100, 100, 100));
        cancel_btn.set_label_color(Color::White);
        cancel_btn.set_frame(FrameType::FlatBox);

        button_flex.fixed(&test_btn, 80);
        button_flex.fixed(&connect_btn, 80);
        button_flex.fixed(&cancel_btn, 80);
        button_flex.end();
        main_flex.fixed(&button_flex, 35);

        main_flex.end();
        dialog.end();

        popups.borrow_mut().push(dialog.clone());

        // Saved connection selection callback
        let sender_for_select = sender.clone();
        saved_browser.set_callback(move |browser| {
            if let Some(selected) = browser.selected_text() {
                let _ = sender_for_select.send(DialogMessage::SelectSaved(selected));
            }
        });

        // Delete button callback
        let sender_for_delete = sender.clone();
        delete_btn.set_callback(move |_| {
            let _ = sender_for_delete.send(DialogMessage::DeleteSelected);
        });

        // Test button callback
        let sender_for_test = sender.clone();
        let name_input_test = name_input.clone();
        let user_input_test = user_input.clone();
        let pass_input_test = pass_input.clone();
        let host_input_test = host_input.clone();
        let port_input_test = port_input.clone();
        let service_input_test = service_input.clone();

        test_btn.set_callback(move |_| {
            let port: u16 = port_input_test.value().parse().unwrap_or(1521);
            let info = ConnectionInfo::new(
                &name_input_test.value(),
                &user_input_test.value(),
                &pass_input_test.value(),
                &host_input_test.value(),
                port,
                &service_input_test.value(),
            );

            let _ = sender_for_test.send(DialogMessage::Test(info));
            app::awake();
        });

        // Connect button callback
        let sender_for_connect = sender.clone();
        let name_input_conn = name_input.clone();
        let user_input_conn = user_input.clone();
        let pass_input_conn = pass_input.clone();
        let host_input_conn = host_input.clone();
        let port_input_conn = port_input.clone();
        let service_input_conn = service_input.clone();
        let save_check_conn = save_check.clone();

        connect_btn.set_callback(move |_| {
            let port: u16 = port_input_conn.value().parse().unwrap_or(1521);
            let info = ConnectionInfo::new(
                &name_input_conn.value(),
                &user_input_conn.value(),
                &pass_input_conn.value(),
                &host_input_conn.value(),
                port,
                &service_input_conn.value(),
            );

            let _ = sender_for_connect.send(DialogMessage::Connect(info, save_check_conn.value()));
            app::awake();
        });

        // Cancel button callback
        let sender_for_cancel = sender.clone();
        cancel_btn.set_callback(move |_| {
            let _ = sender_for_cancel.send(DialogMessage::Cancel);
            app::awake();
        });

        dialog.show();

        let mut saved_browser = saved_browser.clone();
        let mut name_input = name_input.clone();
        let mut user_input = user_input.clone();
        let mut pass_input = pass_input.clone();
        let mut host_input = host_input.clone();
        let mut port_input = port_input.clone();
        let mut service_input = service_input.clone();
        while dialog.shown() {
            fltk::app::wait();
            while let Ok(message) = receiver.try_recv() {
                match message {
                    DialogMessage::SelectSaved(selected) => {
                        let cfg = config.borrow();
                        if let Some(conn) = cfg.get_connection_by_name(&selected) {
                            name_input.set_value(&conn.name);
                            user_input.set_value(&conn.username);
                            pass_input.set_value(&conn.password);
                            host_input.set_value(&conn.host);
                            port_input.set_value(&conn.port.to_string());
                            service_input.set_value(&conn.service_name);
                        }
                    }
                    DialogMessage::DeleteSelected => {
                        if let Some(selected) = saved_browser.selected_text() {
                            let choice = fltk::dialog::choice2_default(
                                &format!("Delete connection '{}'?", selected),
                                "Cancel",
                                "Delete",
                                "",
                            );
                            if choice == Some(1) {
                                let mut cfg = config.borrow_mut();
                                cfg.remove_connection(&selected);
                                if let Err(e) = cfg.save() {
                                    fltk::dialog::alert_default(&format!(
                                        "Failed to save config: {}",
                                        e
                                    ));
                                }
                                saved_browser.clear();
                                for conn in cfg.get_all_connections() {
                                    saved_browser.add(&conn.name);
                                }
                            }
                        } else {
                            fltk::dialog::alert_default(
                                "Please select a connection to delete",
                            );
                        }
                    }
                    DialogMessage::Test(info) => match DatabaseConnection::test_connection(&info) {
                        Ok(_) => {
                            fltk::dialog::message_default("Connection successful!");
                        }
                        Err(e) => {
                            fltk::dialog::alert_default(&format!("Connection failed: {}", e));
                        }
                    },
                    DialogMessage::Connect(info, save_connection) => {
                        if save_connection {
                            let mut cfg = config.borrow_mut();
                            cfg.add_recent_connection(info.clone());
                            if let Err(e) = cfg.save() {
                                fltk::dialog::alert_default(&format!(
                                    "Failed to save connection: {}",
                                    e
                                ));
                            }
                        }

                        *result.borrow_mut() = Some(info);
                        dialog.hide();
                    }
                    DialogMessage::Cancel => {
                        dialog.hide();
                    }
                }
            }
        }

        let final_result = result.borrow().clone();
        final_result
    }
}
