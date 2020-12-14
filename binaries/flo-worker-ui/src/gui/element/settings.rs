use crate::{
  gui::*
};

use::iced::{
  Column, Container, Length, Row, Space, Text, Align, Button, button,
  TextInput, text_input, Checkbox
};

pub fn data_container<'a>( config: &mut Opt
    , confirm_settings_button_state: &'a mut button::State
    , token_input_state: &'a mut text_input::State
    ) -> Container<'a, Message> {

  let flo_web_info_text = Text::new("Use flo web:").size(14);
  let token_info_text = Text::new("Token:").size(14);

  let token_str =
    if let Some(conf_token) = config.token.clone() {
      conf_token
    } else {
      String::new()
    };

  let token_input = TextInput::new(
    token_input_state,
    "Token",
    token_str.as_str(),
    Message::UpdateToken
    ).padding(5)
     .size(20)
     .on_submit(Message::UpdateTokenConfirm(()));

  let confirm_settings_button_title_container =
      Container::new(Text::new("Save settings").size(14))
          .width(Length::FillPortion(1))
          .center_x()
          .align_x(Align::Center);

  let confirm_settings_button: Element<Interaction> =
      Button::new(confirm_settings_button_state, confirm_settings_button_title_container)
          .width(Length::Units(100))
          .style(style::DefaultButton())
          .on_press(Interaction::UpdateTokenConfirm)
          .into();

    let checkbox = Checkbox::new(
        config.use_flo_web,
        "Use Flo Web",
        Message::FloWeb,
      )
      .width(Length::Fill);

    let save_settings_row = Row::new()
        .align_items(Align::End)
        .push(Space::new(Length::Fill, Length::Units(0)))
        .push(confirm_settings_button.map(Message::Interaction))
        .push(Space::new(Length::Units(5), Length::Units(0)));

    let col = Column::new()
        .push(Space::new(Length::Units(0), Length::Units(5)))
        .push(flo_web_info_text)
        .push(Space::new(Length::Units(0), Length::Units(2)))
        .push(checkbox)
        .push(Space::new(Length::Units(0), Length::Units(10)))
        .push(token_info_text)
        .push(Space::new(Length::Units(0), Length::Units(2)))
        .push(token_input)
        .push(Space::new(Length::Units(0), Length::Units(10)))
        .push(save_settings_row)
        .push(Space::new(Length::Units(0), Length::Units(5)));

    let row = Row::new()
        .push(Space::new(Length::Units(5), Length::Units(0)))
        .push(col)
        .push(Space::new(Length::Units(5), Length::Units(0)));

    Container::new(row)
        .width(Length::Fill)
        .height(Length::Shrink)
        .style(style::DefaultStyle())
}
