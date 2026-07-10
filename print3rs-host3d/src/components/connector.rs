use cosmic::iced::Alignment;
use cosmic::iced::widget::{button, column, pick_list, row};
use cosmic::widget::radio;
use cosmic::{Element, widget::combo_box};
use {
    cosmic::widget::text_input, print3rs_commands::commands::connect::HostPort, std::str::FromStr,
};

use print3rs_commands::commands::connect::Connection;

use crate::app::App;
use crate::messages::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Auto,
    Serial,
    Tcp,
    Mqtt,
}

impl Protocol {
    fn from_connection(connection: &Connection<String>) -> Self {
        match connection {
            Connection::Auto => Protocol::Auto,
            Connection::Serial { .. } => Protocol::Serial,
            Connection::Tcp { .. } => Protocol::Tcp,
            Connection::Mqtt { .. } => Protocol::Mqtt,
            _ => todo!(),
        }
    }
}

pub(crate) fn connector(app: &App) -> Element<'_, Message> {
    let connection_details: Element<'_, Message> = match app.connection.clone() {
        Connection::Auto => "".into(),
        Connection::Serial { port, baud } => column![
            combo_box(&app.ports, "printer port", Some(&port), move |port| {
                Message::ChangeConnection(Connection::Serial { port, baud })
            },)
            .on_input(move |port| Message::ChangeConnection(Connection::Serial { port, baud })),
            pick_list([9600, 115200], baud, move |baud| Message::ChangeConnection(
                Connection::Serial {
                    port: port.clone(),
                    baud: Some(baud)
                }
            ),),
        ]
        .spacing(5)
        .into(),
        Connection::Tcp { hostname, port } => {
            let host_port_string = if let Some(port) = port {
                format!("{hostname}:{port}")
            } else {
                hostname
            };
            text_input("hostname:port", host_port_string)
                .on_input(move |hostname| {
                    let HostPort(hostname, port) = if hostname.ends_with(':') {
                        HostPort(hostname, None)
                    } else {
                        HostPort::from_str(&hostname).unwrap_or(HostPort(hostname, None))
                    };
                    Message::ChangeConnection(Connection::Tcp { hostname, port })
                })
                .into()
        }
        Connection::Mqtt {
            hostname,
            port,
            in_topic,
            out_topic,
        } => {
            let host_port_string = if let Some(port) = port {
                format!("{hostname}:{port}")
            } else {
                hostname.clone()
            };
            column![
                text_input("hostname:port", host_port_string).on_input({
                    let in_topic = in_topic.clone();
                    let out_topic = out_topic.clone();
                    move |hostname| {
                        let HostPort(hostname, port) =
                            HostPort::from_str(&hostname).unwrap_or(HostPort(hostname, None));
                        let in_topic = in_topic.clone();
                        let out_topic = out_topic.clone();
                        Message::ChangeConnection(Connection::Mqtt {
                            hostname,
                            port,
                            in_topic,
                            out_topic,
                        })
                    }
                }),
                text_input("in topic", in_topic.clone().unwrap_or_default()).on_input({
                    let hostname = hostname.clone();
                    let out_topic = out_topic.clone();
                    move |in_topic| {
                        let hostname = hostname.clone();
                        let in_topic = if in_topic.is_empty() {
                            None
                        } else {
                            Some(in_topic)
                        };
                        let out_topic = out_topic.clone();
                        Message::ChangeConnection(Connection::Mqtt {
                            hostname,
                            port,
                            in_topic,
                            out_topic,
                        })
                    }
                }),
                text_input("out topic", out_topic.unwrap_or_default()).on_input({
                    let hostname = hostname.clone();
                    let in_topic = in_topic.clone();
                    move |out_topic| {
                        let hostname = hostname.clone();
                        let in_topic = in_topic.clone();
                        let out_topic = if out_topic.is_empty() {
                            None
                        } else {
                            Some(out_topic)
                        };
                        Message::ChangeConnection(Connection::Mqtt {
                            hostname,
                            port,
                            in_topic,
                            out_topic,
                        })
                    }
                })
            ]
            .spacing(5)
        }
        .into(),
        _ => todo!(),
    };
    let auto = radio(
        "Auto",
        Protocol::Auto,
        Some(Protocol::from_connection(&app.connection)),
        Message::SelectProtocol,
    )
    .spacing(5);
    let serial = radio(
        "Serial",
        Protocol::Serial,
        Some(Protocol::from_connection(&app.connection)),
        Message::SelectProtocol,
    )
    .spacing(5);
    let tcp = radio(
        "TCP/IP",
        Protocol::Tcp,
        Some(Protocol::from_connection(&app.connection)),
        Message::SelectProtocol,
    )
    .spacing(5);
    let mqtt = radio(
        "MQTT",
        Protocol::Mqtt,
        Some(Protocol::from_connection(&app.connection)),
        Message::SelectProtocol,
    )
    .spacing(5);
    let protocol_selector = row!["Protocol:", auto, serial, tcp, mqtt]
        .spacing(20.0)
        .align_y(Alignment::Center);
    column![
        protocol_selector,
        connection_details,
        row![
            button(if app.commander.printer().is_connected() {
                "disconnect"
            } else {
                "connect"
            })
            .on_press(Message::ToggleConnect)
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(10)
    .padding(10)
    .into()
}
