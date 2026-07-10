use {
    cosmic::{
        Element,
        iced::widget::{button, column, row},
        widget::{combo_box::State as ComboState, text_editor, text_editor::Content, text_input},
    },
    std::collections::VecDeque,
};

use crate::messages::Message;

#[derive(Debug)]
pub(crate) struct State {
    pub(crate) output: Content,
    pub(crate) command_state: ComboState<String>,
    pub(crate) command_history: VecDeque<String>,
    pub(crate) command: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            output: Default::default(),
            command_state: ComboState::new(vec![]), // TODO: load history from file here
            command_history: Default::default(),
            command: Default::default(),
        }
    }
}

impl State {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let content = text_editor(&self.output)
            .font(cosmic::font::Font::MONOSPACE)
            .on_action(Message::OutputAction);
        column![
            content,
            row![
                text_input("type `help` for list of commands", self.command.as_str())
                    .font(cosmic::font::Font::MONOSPACE)
                    .on_input(Message::CommandInput)
                    .on_submit(Message::SubmitCommand)
                    .trailing_icon(
                        button("send")
                            .on_press(Message::SubmitCommand(self.command.clone()))
                            .into()
                    ),
            ]
        ]
        .into()
    }
}
