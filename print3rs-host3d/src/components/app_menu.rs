use {cosmic::Element, cosmic::widget::menu, std::collections::HashMap};

use crate::app::App;
use crate::messages::Message;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum MenuAction {
    DoMacro(usize),
    KillTask(usize),
    Print,
    Clear,
    Save,
    Quit,
}

impl menu::Action for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::DoMacro(index) => Message::DoMacro(*index),
            MenuAction::KillTask(index) => Message::KillTask(*index),
            MenuAction::Print => Message::PrintDialog,
            MenuAction::Clear => Message::ClearConsole,
            MenuAction::Save => Message::SaveDialog,
            MenuAction::Quit => Message::Quit,
        }
    }
}

pub(crate) fn app_menu(app: &App) -> menu::MenuBar<Message> {
    let keybinds = HashMap::new();
    let file = menu::Tree::with_children(
        Element::from(menu::root("File")),
        menu::items(
            &keybinds,
            vec![
                menu::Item::Button("Print", None, MenuAction::Print),
                menu::Item::Button("Save", None, MenuAction::Save),
                menu::Item::Button("Clear", None, MenuAction::Clear),
                menu::Item::Button("Quit", None, MenuAction::Quit),
            ],
        ),
    );
    let macros = menu::Tree::with_children(
        Element::from(menu::root("Macros")),
        menu::items(
            &keybinds,
            app.commander
                .macros
                .iter()
                .enumerate()
                .map(|(index, (name, _content))| {
                    menu::Item::Button(name.clone(), None, MenuAction::DoMacro(index))
                })
                .collect(),
        ),
    );
    let tasks = menu::Tree::with_children(
        Element::from(menu::root("Tasks")),
        menu::items(
            &keybinds,
            app.commander
                .tasks
                .keys()
                .enumerate()
                .map(|(index, name)| {
                    menu::Item::Button(name.clone(), None, MenuAction::KillTask(index))
                })
                .collect(),
        ),
    );
    menu::MenuBar::new(vec![file, macros, tasks])
}
