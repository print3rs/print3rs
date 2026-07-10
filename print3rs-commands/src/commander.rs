use {
    crate::{
        commands::{
            Command,
            connect::{self, Connection},
            help, macros, version,
        },
        response::Response,
        tasks::{
            BackgroundTask, Tasks, send_gcodes, start_logging, start_print_file, start_repeat,
        },
    },
    print3rs_core::Printer,
    std::sync::Arc,
    tokio::{io::BufReader, net::TcpStream},
    tokio_serial::SerialPortBuilderExt,
};

type CommandReceiver = tokio::sync::mpsc::Receiver<Command<String>>;
type ResponseSender = tokio::sync::broadcast::Sender<Response>;
pub type ResponseReceiver = tokio::sync::broadcast::Receiver<Response>;

#[derive(Debug)]
pub struct Commander {
    printer: Printer,
    pub tasks: Tasks,
    pub macros: macros::Macros,
    responder: ResponseSender,
}
#[derive(Debug, Clone)]
pub struct ErrorKindOf(pub String);

impl<T> From<T> for ErrorKindOf
where
    T: ToString,
{
    fn from(value: T) -> Self {
        Self(value.to_string())
    }
}

impl Default for Commander {
    fn default() -> Self {
        Commander::new()
    }
}

impl Commander {
    pub fn new() -> Self {
        let (responder, _) = tokio::sync::broadcast::channel(32);
        Self {
            printer: Default::default(),
            responder,
            tasks: Default::default(),
            macros: Default::default(),
        }
    }

    pub fn printer(&self) -> &Printer {
        &self.printer
    }

    pub fn set_printer(&mut self, printer: Printer) {
        self.tasks.clear();
        self.printer = printer;
    }

    pub fn subscribe_responses(&self) -> ResponseReceiver {
        self.responder.subscribe()
    }

    fn forward_broadcast(
        mut in_channel: tokio::sync::broadcast::Receiver<Arc<str>>,
        out_channel: tokio::sync::broadcast::Sender<Response>,
    ) {
        tokio::spawn(async move {
            while let Ok(in_message) = in_channel.recv().await {
                out_channel.send(Response::Output(in_message)).unwrap();
            }
        });
    }

    fn add_printer_output_to_responses(&self) {
        if let Ok(print_messages) = self.printer.subscribe_lines() {
            let responder = self.responder.clone();
            Self::forward_broadcast(print_messages, responder);
        }
    }

    pub fn background(mut self, mut commands: CommandReceiver) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                while let Some(command) = commands.recv().await {
                    if let Err(e) = self.dispatch(&command) {
                        let e = e.0;
                        let _ = self.responder.send(format!("Error: {e}").into());
                    }
                }
            }
        })
    }
    pub fn dispatch<'a>(
        &'a mut self,
        command: impl Into<Command<&'a str>>,
    ) -> Result<(), ErrorKindOf> {
        let command = command.into();
        use Command::*;
        match command {
            Clear => {
                self.responder.send(Response::Clear)?;
            }
            Quit => {
                self.responder.send(Response::Quit)?;
            }
            Gcodes(codes) => {
                let socket = self.printer().socket()?.clone();
                let codes = self.macros.expand(codes);
                let task = send_gcodes(socket, codes);
                static COUNTER: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                self.tasks.insert(
                    format!(
                        "gcodes_{}",
                        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    ),
                    task,
                );
            }
            Print(filename) => {
                let socket = self.printer.socket()?.clone();
                let print = start_print_file(filename, socket);
                self.tasks.insert(filename.to_string(), print);
            }
            Log(name, pattern) => {
                let log = start_logging(name, pattern, &self.printer)?;
                self.tasks.insert(name.to_string(), log);
            }
            Repeat(name, gcodes) => {
                let socket = self.printer.socket()?.clone();
                let gcodes = self.macros.expand(gcodes);
                let repeat = start_repeat(gcodes, socket);
                self.tasks.insert(name.to_string(), repeat);
            }
            Tasks => {
                for (
                    name,
                    BackgroundTask {
                        description,
                        abort_handle: _,
                    },
                ) in self.tasks.iter()
                {
                    self.responder
                        .send(format!("{name}\t{description}\n").into())?;
                }
            }
            Stop(name) => {
                self.tasks.remove(name);
            }
            Macro(name, commands) => {
                if self.macros.add(name, commands).is_err() {
                    self.responder
                        .send("Infinite macro detected! Macro not added.\n".into())?;
                }
            }
            Macros => {
                for (name, steps) in self.macros.iter() {
                    let steps = steps.join(";");
                    self.responder
                        .send(format!("{name}:    {steps}\n").into())?;
                }
            }
            DeleteMacro(name) => {
                self.macros.remove(name);
            }
            Connect(connection) => {
                self.tasks.clear();
                match connection {
                    Connection::Auto => {
                        self.tasks.clear();
                        self.responder.send("Connecting...\n".into())?;
                        let autoconnect_responder = self.responder.clone();
                        tokio::spawn(async move {
                            let printer = connect::auto_connect().await;
                            let response = if printer.is_connected() {
                                Response::Output("Found Printer!\n".into())
                            } else {
                                Response::Error("No printer found.\n".into())
                            };
                            if let Ok(printer_responses) = printer.subscribe_lines() {
                                let forward_responder = autoconnect_responder.clone();
                                Self::forward_broadcast(printer_responses, forward_responder);
                            }
                            let _ = autoconnect_responder.send(printer.into());
                            let _ = autoconnect_responder.send(response);
                        });
                    }
                    Connection::Serial { port, baud } => {
                        let connection =
                            tokio_serial::new(port, baud.unwrap_or(115200)).open_native_async()?;
                        let connection = BufReader::new(connection);
                        self.tasks.clear();
                        self.printer.connect(connection);
                        self.add_printer_output_to_responses();
                    }
                    Connection::Tcp { hostname, port } => {
                        let addr = if let Some(port) = port {
                            format!("{hostname}:{port}")
                        } else {
                            hostname.to_owned()
                        };
                        let connection = std::net::TcpStream::connect(addr)?;
                        let connection = BufReader::new(TcpStream::from_std(connection)?);
                        self.tasks.clear();
                        self.printer.connect(connection);
                        self.add_printer_output_to_responses();
                    }
                    Connection::Mqtt {
                        hostname: _,
                        port: _,
                        in_topic: _,
                        out_topic: _,
                    } => todo!(),
                };
            }
            Disconnect => {
                self.tasks.clear();
                self.printer.disconnect()
            }
            Help(subcommand) => {
                self.responder.send(help::help(subcommand).into())?;
            }
            Version => {
                self.responder.send(version::VERSION.into())?;
            }
            _ => {
                self.responder.send("Unsupported command!\n".into())?;
            }
        };
        Ok(())
    }
}
