use crate::{
  gui::*
};

use::iced::{
  Column, Container, Length, Row, Space, Text, Align, Button, button,
  VerticalAlignment, TextInput, text_input
};

pub fn data_container<'a>( config: &mut Opt
    , directory_button_state: &'a mut button::State
    , confirm_settings_button_state: &'a mut button::State
    , token_input_state: &'a mut text_input::State
    ) -> Container<'a, Message> {

  let directory_info_text = Text::new("Install path:").size(14);
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

  let directory_button_title_container =
      Container::new(Text::new("Select").size(14))
          .width(Length::FillPortion(1))
          .center_x()
          .align_x(Align::Center);

  let directory_button: Element<Interaction> =
      Button::new(directory_button_state, directory_button_title_container)
          .width(Length::Units(50))
          .style(style::DefaultButton())
          .on_press(Interaction::SelectDirectory)
          .into();

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

    let path_str = config
        .installation_path
        .as_ref()
        .and_then(|p| p.to_str())
        .unwrap_or("Not specified");
    let directory_data_text = Text::new(path_str)
        .size(14)
        .vertical_alignment(VerticalAlignment::Center);
    let directory_data_text_container = Container::new(directory_data_text)
        .height(Length::Units(25))
        .center_y()
        .style(style::DefaultStyle());

    let directory_data_row = Row::new()
        .push(directory_data_text_container)
        .push(Space::new(Length::Units(2), Length::Units(0)))
        .push(directory_button.map(Message::Interaction));

    let save_settings_row = Row::new()
        .align_items(Align::End)
        .push(Space::new(Length::Fill, Length::Units(0)))
        .push(confirm_settings_button.map(Message::Interaction))
        .push(Space::new(Length::Units(5), Length::Units(0)));

    let col = Column::new()
        .push(Space::new(Length::Units(0), Length::Units(10)))
        .push(directory_info_text)
        .push(Space::new(Length::Units(0), Length::Units(2)))
        .push(directory_data_row)
        .push(Space::new(Length::Units(0), Length::Units(10)))
        .push(token_info_text)
        .push(Space::new(Length::Units(0), Length::Units(2)))
        .push(token_input)
        .push(Space::new(Length::Units(0), Length::Units(10)))
        .push(save_settings_row)
        .push(Space::new(Length::Units(0), Length::Units(20)));

    let row = Row::new()
        .push(Space::new(Length::Units(5), Length::Units(0)))
        .push(col)
        .push(Space::new(Length::Units(5), Length::Units(0)));

    Container::new(row)
        .width(Length::Fill)
        .height(Length::Shrink)
        .style(style::DefaultStyle())
}
