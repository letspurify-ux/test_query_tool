use fltk::{
    button::Button,
    enums::{Color, FrameType},
    group::Flex,
    input::{Input, SecretInput},
    prelude::*,
    window::Window,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::db::{ConnectionInfo, DatabaseConnection};

pub struct ConnectionDialog;

impl ConnectionDialog {
    pub fn show() -> Option<ConnectionInfo> {
        let result: Rc<RefCell<Option<ConnectionInfo>>> = Rc::new(RefCell::new(None));

        let mut dialog = Window::default()
            .with_size(400, 320)
            .with_label("Connect to Oracle Database");
        dialog.set_color(Color::from_rgb(45, 45, 48));
        dialog.make_modal(true);

        let mut main_flex = Flex::default()
            .with_pos(20, 20)
            .with_size(360, 280);
        main_flex.set_type(fltk::group::FlexType::Column);
        main_flex.set_margin(10);
        main_flex.set_spacing(10);

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

        // Test button callback
        let name_input_clone = name_input.clone();
        let user_input_clone = user_input.clone();
        let pass_input_clone = pass_input.clone();
        let host_input_clone = host_input.clone();
        let port_input_clone = port_input.clone();
        let service_input_clone = service_input.clone();

        test_btn.set_callback(move |_| {
            let port: u16 = port_input_clone.value().parse().unwrap_or(1521);
            let info = ConnectionInfo::new(
                &name_input_clone.value(),
                &user_input_clone.value(),
                &pass_input_clone.value(),
                &host_input_clone.value(),
                port,
                &service_input_clone.value(),
            );

            match DatabaseConnection::test_connection(&info) {
                Ok(_) => {
                    fltk::dialog::message_default("Connection successful!");
                }
                Err(e) => {
                    fltk::dialog::alert_default(&format!("Connection failed: {}", e));
                }
            }
        });

        // Connect button callback
        let result_clone = result.clone();
        let mut dialog_clone = dialog.clone();
        let name_input_clone = name_input.clone();
        let user_input_clone = user_input.clone();
        let pass_input_clone = pass_input.clone();
        let host_input_clone = host_input.clone();
        let port_input_clone = port_input.clone();
        let service_input_clone = service_input.clone();

        connect_btn.set_callback(move |_| {
            let port: u16 = port_input_clone.value().parse().unwrap_or(1521);
            let info = ConnectionInfo::new(
                &name_input_clone.value(),
                &user_input_clone.value(),
                &pass_input_clone.value(),
                &host_input_clone.value(),
                port,
                &service_input_clone.value(),
            );

            *result_clone.borrow_mut() = Some(info);
            dialog_clone.hide();
        });

        // Cancel button callback
        let mut dialog_clone = dialog.clone();
        cancel_btn.set_callback(move |_| {
            dialog_clone.hide();
        });

        dialog.show();

        while dialog.shown() {
            fltk::app::wait();
        }

        let final_result = result.borrow().clone();
        final_result
    }
}
