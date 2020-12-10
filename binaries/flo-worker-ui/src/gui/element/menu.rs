use crate::{
  gui::*
};

use::iced::{
  Column, Container, Length, Row, Space, Text, Align, Button, button, HorizontalAlignment
};

pub fn data_container<'a>( _config: &Opt
    , error: &Option<anyhow::Error>
    , settings_button_state: &'a mut button::State, ) -> Container<'a, Message> {

  let mut settings_row = Row::new().height(Length::Units(50));

  let mut segmented_flavor_control_row = Row::new();
  segmented_flavor_control_row = segmented_flavor_control_row.spacing(1);

  let segmented_flavor_control_container =
    Container::new(segmented_flavor_control_row).padding(2);

  // Displays an error, if any has occured.
  let error_text = if let Some(error) = error {
    Text::new(error.to_string()).size(14)
  } else {
    // Display nothing.
    Text::new("")
  };

  let error_container = Container::new(error_text)
      .center_y()
      .center_x()
      .padding(5)
      .width(Length::Fill)
      .style(style::DefaultStyle());

  let version_container = Container::new(Text::new("0.1.0"))
      .center_y()
      .padding(5)
      .style(style::DefaultStyle());

  // Surrounds the elements with spacers, in order to make the GUI look good.
  settings_row = settings_row
      .push(Space::new(Length::Units(2), Length::Units(0)))
      .push(segmented_flavor_control_container)
      .push(Space::new(Length::Units(2), Length::Units(0)))
      .push(error_container)
      .push(version_container);

  let settings_mode_button = Button::new(
      settings_button_state,
      Text::new("Settings [Work in progress]")
          .horizontal_alignment(HorizontalAlignment::Center)
          .size(16),
    )
    .style(style::DefaultButton())
    .on_press(Interaction::ModeSelected(Mode::Settings));

  let settings_mode_button: Element<Interaction> = settings_mode_button.into();

  let segmented_mode_control_row = Row::new()
      .push(settings_mode_button.map(Message::Interaction))
      .spacing(1);

  let segmented_mode_control_container = Container::new(segmented_mode_control_row)
      .padding(2)
      .style(style::DefaultStyle());

  settings_row = settings_row
      .push(segmented_mode_control_container)
      .push(Space::new(
          Length::Units(2 + 5),
          Length::Units(0),
      ))
      .align_items(Align::Center);

  let settings_column = Column::new().push(settings_row);

  Container::new(settings_column).style(style::DefaultStyle())
}
