use {
    core::borrow::Borrow,
    std::{
        collections::HashMap,
        fmt::Debug,
        sync::{Arc, Mutex},
        time::Duration,
    },
    tokio::io::BufReader,
    tokio_serial::SerialStream,
};

use winnow::{
    ascii::{alpha1, alphanumeric1, dec_uint, space0, space1},
    combinator::{alt, dispatch, empty, fail, opt, preceded, rest, separated},
    prelude::*,
    token::take_till,
};

use tokio::{io::AsyncWriteExt, task::JoinHandle, time::timeout};

use print3rs_core::{Error as PrinterError, Printer};
use tokio_serial::{available_ports, SerialPort, SerialPortBuilderExt, SerialPortInfo};

async fn auto_connect() -> Printer {
    async fn check_port(port: SerialPortInfo) -> Option<Printer> {
        tracing::debug!("checking port {}...", port.port_name);
        let mut printer_port = tokio_serial::new(port.port_name, 115200)
            .timeout(Duration::from_secs(10))
            .open_native_async()
            .ok()?;
        printer_port.write_data_terminal_ready(true).ok()?;
        let mut printer = Printer::new(BufReader::new(printer_port));

        printer.send_raw(b"M115\n").ok()?;
        let look_for_ok = async {
            while let Ok(line) = printer.read_next_line().await {
                if line.to_ascii_lowercase().contains("ok") {
                    return Some(printer);
                }
            }
            None
        };

        timeout(Duration::from_secs(5), look_for_ok).await.ok()?
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

pub struct InfiniteRecursion;
type MacrosInner = HashMap<String, Vec<String>>;
#[derive(Debug, Default)]
pub struct Macros(MacrosInner);
impl Macros {
    pub fn new() -> Self {
        Self(MacrosInner::new())
    }
    pub fn add(
        &mut self,
        name: impl AsRef<str>,
        steps: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), InfiniteRecursion> {
        let commands = self.expand_for_insertion(steps)?;
        self.0.insert(name.as_ref().to_ascii_uppercase(), commands);
        Ok(())
    }
    pub fn get(&self, name: impl AsRef<str>) -> Option<&Vec<String>> {
        self.0.get(&name.as_ref().to_ascii_uppercase())
    }
    pub fn remove(&mut self, name: impl AsRef<str>) -> Option<Vec<String>> {
        self.0.remove(&name.as_ref().to_ascii_uppercase())
    }
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, Vec<String>> {
        self.0.iter()
    }
    fn expand_recursive(
        &self,
        expanded: &mut Vec<String>,
        code: &str,
        already_expanded: Option<Vec<&str>>,
    ) -> Result<(), InfiniteRecursion> {
        // track expressions already expanded to prevent infinite recursion
        let mut already_expanded = already_expanded.unwrap_or_default();
        if already_expanded.contains(&code) {
            return Err(InfiniteRecursion);
        }
        match self.get(code) {
            Some(expansion) => {
                already_expanded.push(code);
                for extra in expansion {
                    self.expand_recursive(expanded, extra, Some(already_expanded.clone()))?
                }
            }
            None => expanded.push(code.to_ascii_uppercase()),
        };
        Ok(())
    }
    /// recursively expand all in input sequence before placing into internal map
    /// placing recursion here eliminates possibility of infinite recursion
    fn expand_for_insertion(
        &self,
        codes: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Vec<String>, InfiniteRecursion> {
        let mut expanded = vec![];

        for code in codes {
            self.expand_recursive(&mut expanded, code.as_ref(), None)?;
        }
        Ok(expanded)
    }

    pub fn expand(&self, codes: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
        let mut expanded = vec![];
        for code in codes {
            match self.get(&code) {
                Some(expansion) => expanded.extend(expansion.iter().cloned()),
                None => expanded.push(code.as_ref().to_ascii_uppercase()),
            }
        }
        expanded
    }
}

pub fn version() -> &'static str {
    const VERSION: Option<&str> = option_env!("CARGO_PKG_VERSION");
    VERSION.unwrap_or("???")
}

static FULL_HELP: &str = "    
Anything entered not matching one of the following commands is uppercased and sent to
the printer for it to interpret.

Some commands cannot be ran until a printer is connected.
Some printers support 'autoconnect', otherwise you will need to connect using the serial port name.

Multiple Gcodes can be sent on the same line by separating with ';'.

Arguments with ? are optional.

Available commands:
help         <command?>       display this message or details for specified command
version                       display version
clear                         clear all text on the screen
printerinfo                   display any information found about the connected printer
print        <file>           send gcodes from file to printer
log          <name> <pattern> begin logging parsed output from printer
repeat       <name> <gcodes>  run the given gcodes in a loop until stop
stop         <name>           stop an active print, log, or repeat
macro        <name> <gcodes>  make an alias for a set of gcodes
delmacro     <name>           remove an existing alias for set of gcodes
macros                        list existing command aliases and contents           
send         <gcodes>         explicitly send commands (split by ;) to printer exactly as typed
connect      <path> <baud?>   connect to a specified serial device at baud (default: 115200)
autoconnect                   attempt to find and connect to a printer
disconnect                    disconnect from printer
quit                          exit program
\n";

pub fn help(command: impl AsRef<str>) -> &'static str {
    let command = command
        .as_ref()
        .trim()
        .strip_prefix(':')
        .unwrap_or(command.as_ref().trim());
    match command {
        "send" => "send: explicitly send one or more commands (separated by gcode comment character `;`) commands to the printer, no uppercasing or additional parsing is performed. This can be used to send commands to the printer that would otherwise be detected as a console command.\n",
        "print" => "print: execute every line of G-code sequentially from the given file. The print job is added as a task which runs in the background with the filename as the task name. Other commands can be sent while a print is running, and a print can be stopped at any time with `stop`\n",
        "log" => "log: begin logging the specified pattern from the printer into a csv with the `name` given. This operation runs in the background and is added as a task which can be stopped with `stop`. The pattern given will be used to parse the logs, with values wrapped in `{}` being given a column of whatever is between the `{}`, and pulling a number in its place. If your pattern needs to include a literal `{` or `}`, double them up like `{{` or `}}` to have the parser read it as just a `{` or `}` in the output.\n",
        "repeat" => "repeat: repeat the given Gcodes (separated by gcode comment character `;`) in a loop until stopped. \n",
        "stop" => "stop: stops a task running in the background. All background tasks are required to have a name, thus this command can be used to stop them. Tasks can also stop themselves if they fail or can complete, after which running this will do nothing.\n",
        "connect" => "connect: Manually connect to a printer by specifying its path and optionally its baudrate. On windows this looks like `connect COM3 115200`, on linux more like `connect /dev/tty/ACM0 250000`. This does not test if the printer is capable of responding to messages, it will only open the port.\n",
        "autoconnect" => "autoconnect: On some supported printer firmwares, this will automatically detect a connected printer and verify that it's capable of receiving and responding to commands. This is done with an `M115` command sent to the device, and waiting at most 5 seconds for an `ok` response. If your printer does not support this command, this will not work and you will need manual connection.\n",
        "disconnect" => "disconnect: disconnect from the currently connected printer. All active tasks will be stopped\n",
        "macro" => "create a case-insensitve alias to some set of gcodes, even containing other macros recursively to build up complex sets of builds with a single word. Macro names cannot start with G,T,M,N, or D to avoid conflict with Gcodes, and cannot have any non-alphanumeric characters. commands in a macro are separated by ';', and macros can be used anywhere Gcodes are passed, including repeat commands and sends.\n",
        _ => FULL_HELP,
    }
}

use crate::logging::parsing::{parse_logger, Segment};

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Command<S> {
    Gcodes(Vec<S>),
    Print(S),
    Log(S, Vec<Segment<S>>),
    Repeat(S, Vec<S>),
    Tasks,
    Stop(S),
    Connect(S, Option<u32>),
    AutoConnect,
    Disconnect,
    Macro(S, Vec<S>),
    Macros,
    DeleteMacro(S),
    Help(S),
    Version,
    Clear,
    Quit,
    Unrecognized,
}

impl<'a, S: ToOwned + ?Sized> Command<&'a S> {
    pub fn into_owned(self) -> Command<S::Owned>
    where
        Segment<S::Owned>: From<Segment<&'a S>>,
    {
        use Command::*;
        match self {
            Gcodes(codes) => Gcodes(codes.into_iter().map(ToOwned::to_owned).collect()),
            Print(filename) => Print(filename.to_owned()),
            Log(name, pattern) => Log(
                name.to_owned(),
                pattern.into_iter().map(|s| s.into()).collect(),
            ),
            Repeat(name, codes) => Repeat(
                name.to_owned(),
                codes.into_iter().map(ToOwned::to_owned).collect(),
            ),
            Tasks => Tasks,
            Stop(s) => Stop(s.to_owned()),
            Connect(path, baud) => Connect(path.to_owned(), baud),
            AutoConnect => AutoConnect,
            Disconnect => Disconnect,
            Macro(name, codes) => Macro(
                name.to_owned(),
                codes.into_iter().map(ToOwned::to_owned).collect(),
            ),
            Macros => Macros,
            DeleteMacro(s) => DeleteMacro(s.to_owned()),
            Help(s) => Help(s.to_owned()),
            Version => Version,
            Clear => Clear,
            Quit => Quit,
            Unrecognized => Unrecognized,
        }
    }
    pub fn into_box(self) -> Command<Box<S>>
    where
        Box<S>: From<&'a S>,
    {
        use Command::*;
        match self {
            Gcodes(codes) => Gcodes(codes.into_iter().map(|s| s.into()).collect()),
            Print(filename) => Print(filename.into()),
            Log(name, pattern) => Log(name.into(), pattern.into_iter().map(|s| s.into()).collect()),
            Repeat(name, codes) => {
                Repeat(name.into(), codes.into_iter().map(|s| s.into()).collect())
            }
            Tasks => Tasks,
            Stop(s) => Stop(s.into()),
            Connect(path, baud) => Connect(path.into(), baud),
            AutoConnect => AutoConnect,
            Disconnect => Disconnect,
            Macro(name, codes) => Macro(name.into(), codes.into_iter().map(|s| s.into()).collect()),
            Macros => Macros,
            DeleteMacro(s) => DeleteMacro(s.into()),
            Help(s) => Help(s.into()),
            Version => Version,
            Clear => Clear,
            Quit => Quit,
            Unrecognized => Unrecognized,
        }
    }
}

impl<S> Command<S> {
    pub fn to_borrowed<'a, Borrowed: ?Sized>(&'a self) -> Command<&'a Borrowed>
    where
        S: Borrow<Borrowed>,
        Segment<&'a Borrowed>: From<Segment<S>>,
    {
        use Command::*;
        match self {
            Gcodes(codes) => Gcodes(codes.iter().map(|s| s.borrow()).collect()),
            Print(filename) => Print(filename.borrow()),
            Log(name, pattern) => Log(
                name.borrow(),
                pattern.iter().map(|s| s.to_borrowed()).collect(),
            ),
            Repeat(name, codes) => {
                Repeat(name.borrow(), codes.iter().map(|s| s.borrow()).collect())
            }
            Tasks => Tasks,
            Stop(s) => Stop(s.borrow()),
            Connect(path, baud) => Connect(path.borrow(), *baud),
            AutoConnect => AutoConnect,
            Disconnect => Disconnect,
            Macro(name, codes) => Macro(name.borrow(), codes.iter().map(|s| s.borrow()).collect()),
            Macros => Macros,
            DeleteMacro(s) => DeleteMacro(s.borrow()),
            Help(s) => Help(s.borrow()),
            Version => Version,
            Clear => Clear,
            Quit => Quit,
            Unrecognized => Unrecognized,
        }
    }
}

impl<'a> From<Command<&'a str>> for Command<String> {
    fn from(command: Command<&'a str>) -> Self {
        command.into_owned()
    }
}

impl<'a> From<&'a Command<String>> for Command<&'a str> {
    fn from(command: &'a Command<String>) -> Self {
        use Command::*;
        match command {
            Gcodes(codes) => Gcodes(codes.iter().map(|s| s.as_str()).collect()),
            Print(filename) => Print(filename.as_str()),
            Log(name, pattern) => Log(name.as_str(), pattern.iter().map(|s| s.into()).collect()),
            Repeat(name, codes) => {
                Repeat(name.as_str(), codes.iter().map(|s| s.as_str()).collect())
            }
            Tasks => Tasks,
            Stop(s) => Stop(s.as_str()),
            Connect(path, baud) => Connect(path.as_str(), *baud),
            AutoConnect => AutoConnect,
            Disconnect => Disconnect,
            Macro(name, codes) => Macro(name.as_str(), codes.iter().map(|s| s.as_str()).collect()),
            Macros => Macros,
            DeleteMacro(s) => DeleteMacro(s.as_str()),
            Help(s) => Help(s.as_str()),
            Version => Version,
            Clear => Clear,
            Quit => Quit,
            Unrecognized => Unrecognized,
        }
    }
}

fn parse_gcodes<'a>(input: &mut &'a str) -> PResult<Vec<&'a str>> {
    separated(0.., take_till(1.., ';'), ';').parse_next(input)
}

fn parse_repeater<'a>(input: &mut &'a str) -> PResult<Command<&'a str>> {
    (
        preceded(space0, alphanumeric1),
        preceded(space1, parse_gcodes),
    )
        .map(|(name, gcodes)| Command::Repeat(name, gcodes))
        .parse_next(input)
}

fn parse_macro<'a>(input: &mut &'a str) -> PResult<Command<&'a str>> {
    let alpha_no_reserved_start =
        alpha1.verify(|s: &str| !s.starts_with(|c: char| "GTMND".contains(c.to_ascii_uppercase())));
    let (name, steps) = (
        preceded(space0, alpha_no_reserved_start),
        preceded(space1, parse_gcodes),
    )
        .parse_next(input)?;
    Ok(Command::Macro(name, steps))
}

fn inner_command<'a>(input: &mut &'a str) -> PResult<Command<&'a str>> {
    let explicit = opt(":").parse_next(input)?;
    let command = opt(dispatch! {alpha1;
        "log" => parse_logger,
        "repeat" => parse_repeater,
        "print" => preceded(space0, rest).map(Command::Print),
        "tasks" => empty.map(|_| Command::Tasks),
        "stop" => preceded(space0, rest).map(Command::Stop),
        "help" => rest.map(Command::Help),
        "version" => empty.map(|_| Command::Version),
        "autoconnect" => empty.map(|_| Command::AutoConnect),
        "disconnect" => empty.map(|_| Command::Disconnect),
        "connect" => (preceded(space0, take_till(1.., [' '])), preceded(space0,opt(dec_uint))).map(|(path, baud)| Command::Connect(path, baud)),
        "macro" => parse_macro,
        "macros" => empty.map(|_| Command::Macros),
        "delmacro" => preceded(space0, rest).map(Command::DeleteMacro),
        "send" => preceded(space0, parse_gcodes).map(Command::Gcodes),
        "clear" => empty.map(|_| Command::Clear),
        "quit" | "exit" => empty.map(|_| Command::Quit),
        _ => empty.map(|_| Command::Unrecognized)
    })
    .parse_next(input)?;
    match (explicit, command) {
        (None, Some(Command::Unrecognized)) => fail.parse_next(input),
        (_, None) => Ok(Command::Unrecognized),
        (_, Some(command)) => Ok(command),
    }
}

pub fn parse_command<'a>(input: &mut &'a str) -> PResult<Command<&'a str>> {
    alt((
        inner_command,
        parse_gcodes.map(|gcodes| {
            let gcodes = gcodes.into_iter().collect();
            Command::Gcodes(gcodes)
        }),
    ))
    .parse_next(input)
}

pub fn start_print_file(
    filename: &str,
    printer: &Printer,
) -> std::result::Result<BackgroundTask, print3rs_core::Error> {
    let socket = printer
        .socket()
        .ok_or(print3rs_core::Error::Disconnected)?
        .clone();
    let filename = filename.to_owned();
    let task: JoinHandle<Result<(), TaskError>> = tokio::spawn(async move {
        if let Ok(file) = tokio::fs::read_to_string(filename).await {
            for line in file.lines() {
                socket.send(line).await?.await?;
            }
        }
        Ok(())
    });
    Ok(BackgroundTask {
        description: "print",
        abort_handle: task.abort_handle(),
    })
}

#[derive(Debug, thiserror::Error)]
enum TaskError {
    #[error("{0}")]
    Printer(#[from] print3rs_core::Error),
    #[error("failed in background: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub fn start_logging(
    name: &str,
    pattern: Vec<crate::logging::parsing::Segment<&'_ str>>,
    printer: &Printer,
) -> std::result::Result<BackgroundTask, print3rs_core::Error> {
    let filename = format!(
        "{name}_{timestamp}.csv",
        timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    let header = crate::logging::parsing::get_headers(&pattern);

    let mut parser = crate::logging::parsing::make_parser(pattern);
    let mut log_printer_reader = printer.subscribe_lines()?;
    let log_task_handle = tokio::spawn(async move {
        let mut log_file = tokio::fs::File::create(filename).await.unwrap();
        log_file.write_all(header.as_bytes()).await.unwrap();
        while let Ok(log_line) = log_printer_reader.recv().await {
            if let Ok(parsed) = parser.parse(log_line.as_bytes()) {
                let mut record_bytes = String::new();
                for val in parsed {
                    record_bytes.push_str(&val.to_string());
                    record_bytes.push(',');
                }
                record_bytes.pop(); // remove trailing ','
                record_bytes.push('\n');
                log_file
                    .write_all(record_bytes.as_bytes())
                    .await
                    .unwrap_or_default();
            }
        }
    });
    Ok(BackgroundTask {
        description: "log",
        abort_handle: log_task_handle.abort_handle(),
    })
}

pub fn start_repeat(gcodes: Vec<String>, socket: print3rs_core::Socket) -> BackgroundTask {
    let task: JoinHandle<Result<(), TaskError>> = tokio::spawn(async move {
        for ref line in gcodes.into_iter().cycle() {
            socket.send(line).await?.await?;
        }
        Ok(())
    });
    BackgroundTask {
        description: "repeat",
        abort_handle: task.abort_handle(),
    }
}

pub type Tasks = HashMap<String, BackgroundTask>;

#[derive(Debug)]
pub struct BackgroundTask {
    pub description: &'static str,
    pub abort_handle: tokio::task::AbortHandle,
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}

pub fn send_gcodes(
    printer: &Printer,
    codes: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), PrinterError> {
    for code in codes {
        printer.send_unsequenced(code.as_ref())?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Response {
    Output(Arc<str>),
    Error(ErrorKindOf),
    AutoConnect(Arc<Mutex<Printer>>),
    Clear,
    Quit,
}

impl From<String> for Response {
    fn from(value: String) -> Self {
        Response::Output(Arc::from(value))
    }
}

impl<'a> From<&'a str> for Response {
    fn from(value: &'a str) -> Self {
        Response::Output(Arc::from(value))
    }
}

impl From<ErrorKindOf> for Response {
    fn from(value: ErrorKindOf) -> Self {
        Response::Error(value)
    }
}

impl From<Printer> for Response {
    fn from(value: Printer) -> Self {
        Response::AutoConnect(Arc::new(Mutex::new(value)))
    }
}

type CommandReceiver = tokio::sync::mpsc::Receiver<Command<String>>;
type ResponseSender = tokio::sync::broadcast::Sender<Response>;
type ResponseReceiver = tokio::sync::broadcast::Receiver<Response>;

#[derive(Debug)]
pub struct Commander {
    printer: Printer,
    pub tasks: Tasks,
    pub macros: Macros,
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

#[derive(Debug, Clone, Copy)]
pub struct ConnectParams<'a>(&'a str, Option<u32>);

impl<'a> TryFrom<ConnectParams<'a>> for BufReader<SerialStream> {
    type Error = tokio_serial::Error;

    fn try_from(value: ConnectParams<'a>) -> Result<Self, Self::Error> {
        let stream = tokio_serial::new(value.0, value.1.unwrap_or(115200)).open_native_async()?;
        Ok(BufReader::new(stream))
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
        const DISCONNECTED_ERROR: &str = "No printer is connected!\n";
        match command {
            Clear => {
                self.responder.send(Response::Clear)?;
            }
            Quit => {
                self.responder.send(Response::Quit)?;
            }
            Gcodes(codes) => {
                let codes = self.macros.expand(codes);
                if let Err(_e) = send_gcodes(&self.printer, codes) {
                    self.responder.send(DISCONNECTED_ERROR.into())?;
                }
            }
            Print(filename) => {
                if let Ok(print) = start_print_file(filename, &self.printer) {
                    self.tasks.insert(filename.to_string(), print);
                } else {
                    self.responder.send(DISCONNECTED_ERROR.into())?;
                }
            }
            Log(name, pattern) => {
                if let Ok(log) = start_logging(name, pattern, &self.printer) {
                    self.tasks.insert(name.to_string(), log);
                } else {
                    self.responder.send(DISCONNECTED_ERROR.into())?;
                }
            }
            Repeat(name, gcodes) => {
                if let Some(socket) = self.printer.socket() {
                    let gcodes = self.macros.expand(gcodes);
                    let repeat = start_repeat(gcodes, socket.clone());
                    self.tasks.insert(name.to_string(), repeat);
                } else {
                    self.responder.send(DISCONNECTED_ERROR.into())?;
                }
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
            Connect(path, baud) => {
                if let Ok(port) = ConnectParams(path, baud).try_into() {
                    self.tasks.clear();
                    self.printer.connect::<BufReader<SerialStream>>(port);
                    self.add_printer_output_to_responses();
                } else {
                    self.responder
                        .send(Response::Error("Connection failed.\n".into()))?;
                }
            }
            AutoConnect => {
                self.tasks.clear();
                self.responder.send("Connecting...\n".into())?;
                let autoconnect_responder = self.responder.clone();
                tokio::spawn(async move {
                    let printer = auto_connect().await;
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
            Disconnect => {
                self.tasks.clear();
                self.printer.disconnect()
            }
            Help(subcommand) => {
                self.responder.send(help(subcommand).into())?;
            }
            Version => {
                self.responder.send(version().into())?;
            }
            _ => {
                self.responder.send("Unsupported command!\n".into())?;
            }
        };
        Ok(())
    }
}
