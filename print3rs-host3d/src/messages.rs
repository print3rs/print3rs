use {
    cosmic::widget::ToastId,
    print3rs_commands::{
        commands::{Command, connect::Connection},
        response::Response,
    },
    print3rs_core::Printer,
    std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    },
};

use crate::components::Protocol;

#[derive(Debug, Clone, Default)]
pub(crate) struct JogMove {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

impl JogMove {
    pub(crate) fn x(x: f32) -> Self {
        Self {
            x,
            ..Default::default()
        }
    }
    pub(crate) fn y(y: f32) -> Self {
        Self {
            y,
            ..Default::default()
        }
    }
    pub(crate) fn z(z: f32) -> Self {
        Self {
            z,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveAxis {
    X,
    Y,
    Z,
    All,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Jog(JogMove),
    Home(MoveAxis),
    SelectProtocol(Protocol),
    ChangeConnection(Connection<String>),
    ToggleConnect,
    JogScale(f32),
    CommandInput(String),
    SubmitCommand(String),
    ProcessCommand(Command<String>),
    Quit,
    ClearConsole,
    PrintDialog,
    SaveDialog,
    SaveConsole(PathBuf),
    ConsoleAppend(String),
    AutoConnectComplete(Arc<Mutex<Printer>>),
    PushToast(String),
    PopToast(ToastId),
    OutputAction(cosmic::widget::text_editor::Action),
    DoMacro(usize),
    KillTask(usize),
}

impl From<Response> for Message {
    fn from(value: Response) -> Self {
        match value {
            Response::Output(s) => Message::ConsoleAppend(s.to_string()),
            Response::Error(e) => Message::PushToast(e.0),
            Response::AutoConnect(a) => Message::AutoConnectComplete(a),
            Response::Clear => Message::ClearConsole,
            Response::Quit => Message::Quit,
        }
    }
}
