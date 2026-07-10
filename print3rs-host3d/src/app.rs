use cosmic::{
    Application, Command,
    app::Core,
    iced::Subscription,
    prelude::*,
    widget::{self, Toast, Toasts, combo_box::State as ComboState, toaster},
};
use {
    crate::components, print3rs_commands::commander::Commander, print3rs_core::Printer,
    std::sync::Arc,
};
use {crate::components::Console, print3rs_commands::commands::connect::Connection};

use tokio_serial::available_ports;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use winnow::prelude::*;

use rfd::AsyncFileDialog;

use crate::messages::{JogMove, Message};

pub(crate) struct App {
    pub(crate) cosmic: Core,
    pub(crate) ports: ComboState<String>,
    pub(crate) connection: Connection<String>,
    pub(crate) commander: Commander,
    pub(crate) console: Console,
    pub(crate) toasts: Toasts<Message>,
    pub(crate) jog_scale: f32,
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Message = Message;
    type Flags = ();

    const APP_ID: &'static str = "com.print3rs.Host3d";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Command<cosmic::app::Message<Message>>) {
        let mut ports: Vec<String> = available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|port| port.port_name)
            .collect();
        ports.push("auto".to_string());
        (
            Self {
                cosmic: core,
                ports: ComboState::new(ports),
                connection: Connection::Auto,
                commander: Default::default(),
                console: Default::default(),
                toasts: Toasts::new(Message::PopToast),
                jog_scale: 10.0,
            },
            Command::none(),
        )
    }

    fn core(&self) -> &Core {
        &self.cosmic
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.cosmic
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![components::app_menu(self).into()]
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct PrinterResponseSubscription;
        let responses = self.commander.subscribe_responses();
        let response_stream =
            BroadcastStream::new(responses).map(|response| Message::from(response.unwrap()));
        cosmic::iced::subscription::run_with_id(
            std::any::TypeId::of::<PrinterResponseSubscription>(),
            response_stream,
        )
    }

    fn update(&mut self, message: Self::Message) -> Command<cosmic::app::Message<Self::Message>> {
        match message {
            Message::Jog(JogMove { x, y, z }) => {
                if let Err(msg) = self
                    .commander
                    .printer()
                    .try_send_unsequenced(format!("G7X{x}Y{y}Z{z}"))
                {
                    self.toasts
                        .push(Toast::new(msg.to_string()))
                        .map(cosmic::app::Message::App)
                } else {
                    Command::none()
                }
            }
            Message::ToggleConnect => {
                if self.commander.printer().is_connected() {
                    self.commander.set_printer(Printer::Disconnected);
                } else if let Err(msg) =
                    self.commander
                        .dispatch(print3rs_commands::commands::Command::Connect(
                            self.connection.to_borrowed(),
                        ))
                {
                    return self
                        .toasts
                        .push(Toast::new(msg.0))
                        .map(cosmic::app::Message::App);
                }

                Command::none()
            }
            Message::CommandInput(s) => {
                self.console.command = s;
                Command::none()
            }
            Message::SubmitCommand => {
                let command_string = &mut self.console.command;
                if command_string.is_empty() {
                    return Command::none();
                }
                if let Ok(command) =
                    print3rs_commands::commands::parse_command.parse(command_string)
                {
                    if let Err(msg) = self.commander.dispatch(command) {
                        return self
                            .toasts
                            .push(Toast::new(msg.0))
                            .map(cosmic::app::Message::App);
                    }
                    if !self.console.command_history.contains(command_string) {
                        self.console
                            .command_history
                            .push_back(command_string.clone());
                        if self.console.command_history.len() > 1000 {
                            self.console.command_history.pop_front();
                        }
                        self.console.command_history.make_contiguous();
                        self.console.command_state =
                            ComboState::new(self.console.command_history.as_slices().0.to_owned());
                    }
                    command_string.clear();
                } else {
                    return self
                        .toasts
                        .push(Toast::new("Could not parse command"))
                        .map(cosmic::app::Message::App);
                }
                Command::none()
            }
            Message::ProcessCommand(command) => {
                if let Err(msg) = self.commander.dispatch(&command) {
                    self.toasts
                        .push(Toast::new(msg.0))
                        .map(cosmic::app::Message::App)
                } else {
                    Command::none()
                }
            }
            Message::ConsoleAppend(s) => {
                use widget::text_editor::{Action, Edit};
                for c in s.chars() {
                    let action = Action::Edit(Edit::Insert(c));
                    self.console.output.perform(action)
                }
                self.console.output.perform(Action::Edit(Edit::Enter));
                Command::none()
            }
            Message::AutoConnectComplete(a_printer) => {
                let printer = Arc::into_inner(a_printer)
                    .unwrap_or_default()
                    .into_inner()
                    .unwrap_or_default();
                self.commander.set_printer(printer);
                Command::none()
            }
            Message::ClearConsole => {
                self.console.output = cosmic::widget::text_editor::Content::new();
                Command::none()
            }
            Message::Quit => cosmic::command::message(cosmic::app::Message::Cosmic(
                cosmic::app::cosmic::Message::Close,
            )),
            Message::PrintDialog => Command::perform(
                AsyncFileDialog::new()
                    .set_directory(directories_next::BaseDirs::new().unwrap().home_dir())
                    .pick_file(),
                |f| match f {
                    Some(file) => cosmic::app::Message::App(Message::ProcessCommand(
                        print3rs_commands::commands::Command::Print(
                            file.path().to_string_lossy().into_owned(),
                        ),
                    )),
                    None => cosmic::app::Message::App(Message::NoOp),
                },
            ),
            Message::SaveDialog => Command::perform(
                AsyncFileDialog::new()
                    .set_directory(directories_next::BaseDirs::new().unwrap().home_dir())
                    .save_file(),
                |f| match f {
                    Some(file) => cosmic::app::Message::App(Message::SaveConsole(file.into())),
                    None => cosmic::app::Message::App(Message::NoOp),
                },
            ),
            Message::SaveConsole(file) => {
                Command::perform(tokio::fs::write(file, self.console.output.text()), |_| {
                    cosmic::app::Message::App(Message::NoOp)
                })
            }
            Message::PushToast(msg) => self
                .toasts
                .push(Toast::new(msg))
                .map(cosmic::app::Message::App),
            Message::PopToast(id) => {
                self.toasts.remove(id);
                Command::none()
            }
            Message::OutputAction(action) => {
                if !action.is_edit() {
                    self.console.output.perform(action);
                }
                Command::none()
            }
            Message::NoOp => Command::none(),
            Message::JogScale(scale) => {
                self.jog_scale = scale;
                Command::none()
            }
            Message::Home(axis) => {
                let arg = match axis {
                    crate::messages::MoveAxis::X => "X",
                    crate::messages::MoveAxis::Y => "Y",
                    crate::messages::MoveAxis::Z => "Z",
                    crate::messages::MoveAxis::All => "",
                };
                if let Err(msg) = self
                    .commander
                    .printer()
                    .try_send_unsequenced(format!("G28{arg}"))
                {
                    self.toasts
                        .push(Toast::new(msg.to_string()))
                        .map(cosmic::app::Message::App)
                } else {
                    Command::none()
                }
            }
            Message::SelectProtocol(proto) => {
                self.connection = match proto {
                    components::Protocol::Auto => Connection::Auto,
                    components::Protocol::Serial => Connection::Serial {
                        port: "".to_string(),
                        baud: None,
                    },
                    components::Protocol::Tcp => Connection::Tcp {
                        hostname: "".to_string(),
                        port: None,
                    },
                    components::Protocol::Mqtt => Connection::Mqtt {
                        hostname: "".to_string(),
                        port: None,
                        in_topic: None,
                        out_topic: None,
                    },
                };
                Command::none()
            }
            Message::ChangeConnection(connection) => {
                self.connection = connection;
                Command::none()
            }
            Message::DoMacro(index) => {
                if let Some((_name, commands)) = self.commander.macros.iter().nth(index) {
                    cosmic::command::message(Message::ProcessCommand(
                        print3rs_commands::commands::Command::Gcodes(commands.clone()),
                    ))
                } else {
                    Command::none()
                }
            }
            Message::KillTask(index) => {
                if let Some(key) = self.commander.tasks.keys().nth(index).cloned() {
                    self.commander.tasks.remove(&key);
                }
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let main_content = widget::row()
            .push(
                widget::column()
                    .push(components::connector(self))
                    .push(cosmic::iced::widget::horizontal_rule(4))
                    .push(components::jogger(self))
                    .padding(10),
            )
            .push(self.console.view())
            .padding(10);
        toaster(&self.toasts, main_content)
    }
}
