use {
    self::{
        connect::Connection,
        log::{Segment, parse_logger},
    },
    crate::commands::connect::parse_connection,
    core::borrow::Borrow,
    std::fmt::Debug,
    winnow::{
        ascii::digit1,
        combinator::terminated,
        stream::{AsChar, Stream},
        token::take_while,
    },
};

use winnow::{
    ascii::{alpha1, space0, space1},
    combinator::{alt, dispatch, empty, fail, opt, preceded, separated},
    prelude::*,
    token::{rest, take_till},
};

pub mod connect;
pub mod help;
pub mod log;
pub mod macros;
pub mod version;

pub fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    const NAME_CHARS: (
        std::ops::RangeInclusive<char>,
        std::ops::RangeInclusive<char>,
        std::ops::RangeInclusive<char>,
        [char; 3],
    ) = ('a'..='z', 'A'..='Z', '0'..='9', ['-', '_', '.']);
    take_while(1.., NAME_CHARS)
        .verify(|ident| plausible_code.parse(ident).is_err())
        .parse_next(input)
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Command<S> {
    Gcodes(Vec<S>),
    Print(S),
    Log(S, Vec<Segment<S>>),
    Repeat(S, Vec<S>),
    Tasks,
    Stop(S),
    Connect(Connection<S>),
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

impl Command<&str> {
    pub fn into_owned(self) -> Command<String> {
        use Command::*;
        match self {
            Gcodes(codes) => Gcodes(codes.into_iter().map(str::to_owned).collect()),
            Print(filename) => Print(filename.to_owned()),
            Log(name, pattern) => Log(
                name.to_owned(),
                pattern.into_iter().map(Segment::into_owned).collect(),
            ),
            Repeat(name, codes) => Repeat(
                name.to_owned(),
                codes.into_iter().map(str::to_owned).collect(),
            ),
            Tasks => Tasks,
            Stop(s) => Stop(s.to_owned()),
            Connect(connection) => Connect(connection.into_owned()),
            Disconnect => Disconnect,
            Macro(name, codes) => Macro(
                name.to_owned(),
                codes.into_iter().map(str::to_owned).collect(),
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
}

impl Command<String> {
    pub fn to_borrowed<Borrowed: ?Sized>(&self) -> Command<&Borrowed>
    where
        String: Borrow<Borrowed>,
    {
        use Command::*;
        match self {
            Gcodes(codes) => Gcodes(codes.iter().map(|s| s.borrow()).collect()),
            Print(filename) => Print(filename.borrow()),
            Log(name, pattern) => Log(
                name.borrow(),
                pattern.iter().map(Segment::to_borrowed).collect(),
            ),
            Repeat(name, codes) => {
                Repeat(name.borrow(), codes.iter().map(|s| s.borrow()).collect())
            }
            Tasks => Tasks,
            Stop(s) => Stop(s.borrow()),
            Connect(connection) => Connect(connection.to_borrowed()),
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
        command.to_borrowed()
    }
}

fn plausible_code<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    let checkpoint = input.checkpoint();
    let _ = preceded(space0, (take_while(1, AsChar::is_alpha), digit1)).parse_next(input)?;
    input.reset(&checkpoint);
    take_till(2.., ';').parse_next(input)
}

fn parse_gcodes<'a>(input: &mut &'a str) -> ModalResult<Vec<&'a str>> {
    terminated(separated(0.., plausible_code, ';'), opt(";")).parse_next(input)
}

fn parse_repeater<'a>(input: &mut &'a str) -> ModalResult<Command<&'a str>> {
    (preceded(space0, identifier), preceded(space1, parse_gcodes))
        .map(|(name, gcodes)| Command::Repeat(name, gcodes))
        .parse_next(input)
}

fn parse_macro<'a>(input: &mut &'a str) -> ModalResult<Command<&'a str>> {
    let (name, steps) =
        (preceded(space0, identifier), preceded(space1, parse_gcodes)).parse_next(input)?;
    Ok(Command::Macro(name, steps))
}

fn inner_command<'a>(input: &mut &'a str) -> ModalResult<Command<&'a str>> {
    dispatch! {preceded(space0, alpha1);
        "log" => parse_logger,
        "repeat" => parse_repeater,
        "print" => preceded(space0, rest).map(Command::Print),
        "tasks" => empty.map(|_| Command::Tasks),
        "stop" => preceded(space0, rest).map(Command::Stop),
        "help" => rest.map(Command::Help),
        "version" => empty.map(|_| Command::Version),
        "disconnect" => empty.map(|_| Command::Disconnect),
        "connect" => parse_connection,
        "macro" => parse_macro,
        "macros" => empty.map(|_| Command::Macros),
        "delmacro" => preceded(space0, rest).map(Command::DeleteMacro),
        "clear" => empty.map(|_| Command::Clear),
        "quit" | "exit" => empty.map(|_| Command::Quit),
        _ => fail
    }
    .parse_next(input)
}

pub fn parse_command<'a>(input: &mut &'a str) -> ModalResult<Command<&'a str>> {
    alt((
        inner_command,
        parse_gcodes.map(|gcodes| {
            let gcodes = gcodes.into_iter().collect();
            Command::Gcodes(gcodes)
        }),
    ))
    .parse_next(input)
}
