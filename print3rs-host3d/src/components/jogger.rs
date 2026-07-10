use crate::messages::{JogMove, Message, MoveAxis};
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced::alignment;
use cosmic::iced::widget::{button, column, row};
use cosmic::widget::{Space, container, slider, text};
use {crate::app::App, cosmic::iced::Alignment};

pub(crate) fn jogger(app: &App) -> Element<'_, Message> {
    enum Jog {
        X(f32),
        Y(f32),
        Z(f32),
    }
    const BUTTON_WIDTH: f32 = 72.0;
    let if_connected = |message| app.commander.printer().is_connected().then_some(message);
    let jog_button = |jog: Jog| {
        let (label, jogmove) = match jog {
            Jog::X(scale) => (text(format!("X{scale:+}")), JogMove::x(scale)),
            Jog::Y(scale) => (text(format!("Y{scale:+}")), JogMove::y(scale)),
            Jog::Z(scale) => (
                text(format!("Z{:+.1}", scale / 10.0)),
                JogMove::z(scale / 10.0),
            ),
        };
        button(
            label
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Center),
        )
        .on_press_maybe(if_connected(Message::Jog(jogmove)))
        .width(BUTTON_WIDTH)
    };
    let scale = app.jog_scale.round().max(1.0);
    let xy_buttons = column![
        jog_button(Jog::Y(scale)),
        row![
            jog_button(Jog::X(-scale)),
            Space::new().width(BUTTON_WIDTH),
            jog_button(Jog::X(scale)),
        ]
        .spacing(0.0),
        jog_button(Jog::Y(-scale)),
    ]
    .spacing(0.0)
    .align_x(Alignment::Center);

    container(
        column![
            row![
                xy_buttons,
                column![
                    Space::new().height(10.0),
                    jog_button(Jog::Z(scale)),
                    Space::new().height(10.0),
                    jog_button(Jog::Z(-scale))
                ]
                .spacing(10.0),
            ]
            .spacing(10.0)
            .align_y(Alignment::Center),
            slider(0.0..=100.0, app.jog_scale, Message::JogScale)
                .step(1.0)
                .width(240),
            row![
                button(text("home").align_x(alignment::Horizontal::Center))
                    .width(BUTTON_WIDTH)
                    .on_press_maybe(if_connected(Message::Home(MoveAxis::All))),
                button(text("X").align_x(alignment::Horizontal::Center))
                    .width(BUTTON_WIDTH / 2.0)
                    .on_press_maybe(if_connected(Message::Home(MoveAxis::X))),
                button(text("Y").align_x(alignment::Horizontal::Center))
                    .width(BUTTON_WIDTH / 2.0)
                    .on_press_maybe(if_connected(Message::Home(MoveAxis::Y))),
                button(text("Z").align_x(alignment::Horizontal::Center))
                    .width(BUTTON_WIDTH / 2.0)
                    .on_press_maybe(if_connected(Message::Home(MoveAxis::Z))),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(10.0),
    )
    .center_x(Length::Shrink)
    .padding(10)
    .into()
}
