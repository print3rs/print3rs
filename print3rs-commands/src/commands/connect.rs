use {
    super::Command,
    print3rs_core::Printer,
    std::{borrow::Borrow, str::FromStr, time::Duration},
    tokio::{
        io::BufReader,
        time::{sleep, timeout},
    },
    tokio_serial::{available_ports, SerialPort, SerialPortBuilderExt, SerialPortInfo},
    winnow::{
        ascii::{alpha0, dec_uint, space0},
        combinator::{alt, dispatch, empty, opt, preceded, terminated},
        prelude::*,
        token::take_till,
    },
};

/// Attempt to enumerate and establish a connection to a device,
/// connecting and returning to said device if any were successful.
///
/// If no valid device is found, return a disconnected device.
pub async fn auto_connect() -> Printer {
    async fn check_port(port: SerialPortInfo) -> Option<Printer> {
        tracing::debug!("checking port {}...", port.port_name);
        let mut printer_port = tokio_serial::new(port.port_name, 115200)
            .timeout(Duration::from_secs(10))
            .open_native_async()
            .ok()?;
        printer_port.write_data_terminal_ready(true).ok()?;
        let printer = Printer::new(BufReader::new(printer_port));

        sleep(Duration::from_secs(1)).await;

        let look_for_ok = printer.send_unsequenced(b"M115\n").await.ok()?;

        if timeout(Duration::from_secs(5), look_for_ok).await.is_ok() {
            Some(printer)
        } else {
            None
        }
    }
    if let Ok(ports) = available_ports() {
        tracing::info!("found available ports: {ports:?}");
        for port in ports {
            if let Some(printer) = check_port(port).await {
                return printer;
            }
        }
    }
    Printer::Disconnected
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostPort(pub String, pub Option<u16>);

impl FromStr for HostPort {
    type Err = winnow::error::ContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (host, port) = parse_hostname_port.parse(s).map_err(|e| e.into_inner())?;
        Ok(HostPort(host.to_string(), port))
    }
}

/// Underlying protocol used to establish communication to device.
#[non_exhaustive]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Connection<S> {
    #[default]
    Auto,
    Serial {
        port: S,
        baud: Option<u32>,
    },
    Tcp {
        hostname: S,
        port: Option<u16>,
    },
    Mqtt {
        hostname: S,
        port: Option<u16>,
        in_topic: Option<S>,
        out_topic: Option<S>,
    },
}

impl<T> Connection<T> {
    /// Name of the protocol being used
    pub fn protocol(&self) -> &str {
        match self {
            Connection::Auto => "Auto",
            Connection::Serial { .. } => "Serial",
            Connection::Tcp { .. } => "TCP/IP",
            Connection::Mqtt { .. } => "Mqtt",
        }
    }
}

impl Connection<&str> {
    /// convert any inner borrowed data into owned
    pub fn into_owned(self) -> Connection<String> {
        match self {
            Connection::Auto => Connection::Auto,
            Connection::Serial { port, baud } => Connection::Serial {
                port: port.to_owned(),
                baud,
            },
            Connection::Tcp { hostname, port } => Connection::Tcp {
                hostname: hostname.to_owned(),
                port,
            },
            Connection::Mqtt {
                hostname,
                port,
                in_topic,
                out_topic,
            } => Connection::Mqtt {
                hostname: hostname.to_owned(),
                port,
                in_topic: in_topic.map(|s| s.to_owned()),
                out_topic: out_topic.map(|s| s.to_owned()),
            },
        }
    }
}
impl Connection<String> {
    /// Get a borrow to any owned data.
    pub fn to_borrowed<Borrowed: ?Sized>(&self) -> Connection<&Borrowed>
    where
        String: Borrow<Borrowed>,
    {
        match self {
            Connection::Auto => Connection::Auto,
            Connection::Serial { port, baud } => Connection::Serial {
                port: port.borrow(),
                baud: *baud,
            },
            Connection::Tcp { hostname, port } => Connection::Tcp {
                hostname: hostname.borrow(),
                port: *port,
            },
            Connection::Mqtt {
                hostname,
                port,
                in_topic,
                out_topic,
            } => Connection::Mqtt {
                hostname: hostname.borrow(),
                port: *port,
                in_topic: in_topic.as_ref().map(|s| s.borrow()),
                out_topic: out_topic.as_ref().map(|s| s.borrow()),
            },
        }
    }
}

fn parse_serial_connection<'a>(input: &mut &'a str) -> PResult<Connection<&'a str>> {
    let (port, baud) = (
        preceded(space0, take_till(1.., ' ')),
        preceded(space0, opt(dec_uint)),
    )
        .parse_next(input)?;
    Ok(Connection::Serial { port, baud })
}

fn parse_hostname_port<'a>(input: &mut &'a str) -> PResult<(&'a str, Option<u16>)> {
    (
        preceded(space0, take_till(1.., [' ', ':'])),
        preceded(alt((":", space0)), opt(dec_uint)),
    )
        .parse_next(input)
}

fn parse_tcp_connection<'a>(input: &mut &'a str) -> PResult<Connection<&'a str>> {
    let (hostname, port) = terminated(parse_hostname_port, space0).parse_next(input)?;
    Ok(Connection::Tcp { hostname, port })
}

fn parse_mqtt_connection<'a>(input: &mut &'a str) -> PResult<Connection<&'a str>> {
    let (hostname, port) = parse_hostname_port.parse_next(input)?;
    let (in_topic, out_topic) = terminated(
        (
            preceded(space0, opt(take_till(1.., ' '))),
            preceded(space0, opt(take_till(1.., ' '))),
        ),
        space0,
    )
    .parse_next(input)?;
    Ok(Connection::Mqtt {
        hostname,
        port,
        in_topic,
        out_topic,
    })
}

/// Parse connection details from a string, for any known protocol
pub fn parse_connection<'a>(input: &mut &'a str) -> PResult<Command<&'a str>> {
    let connection = dispatch! { preceded(space0, alpha0);
        "serial" => parse_serial_connection,
        "tcp" | "ip" => parse_tcp_connection,
        "mqtt" => parse_mqtt_connection,
        _ => empty.map(|_| Connection::Auto),
    }
    .parse_next(input)?;
    Ok(Command::Connect(connection))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn serial_space_parsing() {
        let serial = parse_serial_connection.parse("  /dev/ttyS0  9600").unwrap();
        assert_eq!(
            serial,
            Connection::Serial {
                port: "/dev/ttyS0",
                baud: Some(9600)
            }
        );
    }

    #[test]
    fn serial_baudless_parsing() {
        let serial = parse_serial_connection.parse("COM1").unwrap();
        assert_eq!(
            serial,
            Connection::Serial {
                port: "COM1",
                baud: None
            }
        );
    }

    #[test]
    fn ip_space_parsing() {
        let ip = parse_hostname_port.parse("  1.1.1.1  8080").unwrap();
        assert_eq!(ip, ("1.1.1.1", Some(8080)));
    }

    #[test]
    fn ip_colon_parsing() {
        let ip = parse_hostname_port.parse("google.com:80").unwrap();
        assert_eq!(ip, ("google.com", Some(80)));
    }

    #[test]
    fn tcp_parsing() {
        let tcp = parse_tcp_connection
            .parse(" dopewebsite.biz:10000 ")
            .unwrap();
        assert_eq!(
            tcp,
            Connection::Tcp {
                hostname: "dopewebsite.biz",
                port: Some(10000)
            }
        );
    }

    #[test]
    fn tcp_portless_parsing() {
        let tcp = parse_tcp_connection.parse(" 8.8.8.8 ").unwrap();
        assert_eq!(
            tcp,
            Connection::Tcp {
                hostname: "8.8.8.8",
                port: None
            }
        );
    }

    #[test]
    fn mqtt_default_parsing() {
        let mqtt = parse_mqtt_connection.parse("printer.local").unwrap();
        assert_eq!(
            mqtt,
            Connection::Mqtt {
                hostname: "printer.local",
                port: None,
                in_topic: None,
                out_topic: None
            }
        );
    }

    #[test]
    fn mqtt_in_parsing() {
        let mqtt = parse_mqtt_connection
            .parse("printer.local /control/gcode")
            .unwrap();
        assert_eq!(
            mqtt,
            Connection::Mqtt {
                hostname: "printer.local",
                port: None,
                in_topic: Some("/control/gcode"),
                out_topic: None
            }
        );
    }

    #[test]
    fn mqtt_all_parsing() {
        let mqtt = parse_mqtt_connection
            .parse("printer.local:1963 /control/gcode /printer/log")
            .unwrap();
        assert_eq!(
            mqtt,
            Connection::Mqtt {
                hostname: "printer.local",
                port: Some(1963),
                in_topic: Some("/control/gcode"),
                out_topic: Some("/printer/log")
            }
        );
    }

    #[test]
    fn conversion() {
        let borrowed = Connection::Mqtt {
            hostname: "test",
            port: None,
            in_topic: Some("thing"),
            out_topic: Some("thing2"),
        };
        let owned = borrowed.clone().into_owned();
        assert_eq!(borrowed, owned.to_borrowed());
    }

    #[test]
    fn command_parse() {
        let input = "serial COM1 9600";
        let command = parse_connection.parse(input).unwrap();
        assert_eq!(
            command,
            Command::Connect(Connection::Serial {
                port: "COM1",
                baud: Some(9600)
            })
        );
    }
}
