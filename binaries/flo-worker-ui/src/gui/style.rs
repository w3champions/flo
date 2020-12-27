use iced::{button, container, Background, Color};

pub struct DefaultStyle();
impl container::StyleSheet for DefaultStyle {
  fn style(&self) -> container::Style {
    container::Style {
      background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
      text_color: Some(Color::from_rgb(0.9, 0.9, 0.9)),
      ..container::Style::default()
    }
  }
}

pub struct DefaultButton();
impl button::StyleSheet for DefaultButton {
  fn active(&self) -> button::Style {
    button::Style {
      text_color: Color::from_rgb(0.8, 0.8, 0.8),
      border_radius: 2.0,
      ..button::Style::default()
    }
  }
  fn hovered(&self) -> button::Style {
    button::Style {
      background: Some(Background::Color(Color::from_rgb(0.3, 0.3, 0.3))),
      text_color: Color::from_rgb(0.9, 0.9, 0.9),
      ..self.active()
    }
  }
  fn disabled(&self) -> button::Style {
    button::Style {
      text_color: Color::from_rgba(0.75, 0.75, 0.75, 0.25),
      ..self.active()
    }
  }
}

pub struct DefaultBoxedButton();
impl button::StyleSheet for DefaultBoxedButton {
  fn active(&self) -> button::Style {
    button::Style {
      border_color: Color::from_rgba(0.25, 0.25, 0.25, 0.6),
      background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
      border_width: 1.0,
      border_radius: 5.0,
      text_color: Color::from_rgb(0.8, 0.8, 0.8),
      ..button::Style::default()
    }
  }

  fn hovered(&self) -> button::Style {
    button::Style {
      background: Some(Background::Color(Color::from_rgb(0.3, 0.3, 0.3))),
      text_color: Color::from_rgb(0.9, 0.9, 0.9),
      ..self.active()
    }
  }

  fn disabled(&self) -> button::Style {
    button::Style {
      text_color: Color::from_rgba(0.75, 0.75, 0.75, 0.25),
      ..self.active()
    }
  }
}
