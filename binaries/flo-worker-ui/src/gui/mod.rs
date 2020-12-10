use super::Opt;

mod element;
mod style;
mod update;

use iced::{
  button, Application, Command, Element, Container, Column, Length, Settings, Button, Text,
  HorizontalAlignment, Row
};

pub struct Flo {
  config: Opt,
  run_btn_state: button::State,
  exit_btn_state: button::State,
  settings_btn_state: button::State,
  error: Option<anyhow::Error>,
  flo_running: bool
}

impl Default for Flo {
  fn default() -> Self {
    Self {
      config: Default::default(),
      run_btn_state: Default::default(),
      exit_btn_state: Default::default(),
      settings_btn_state: Default::default(),
      error: None,
      flo_running: false
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Mode {
  Main,
  Settings
}

#[derive(Debug, Clone)]
pub enum Interaction {
  ModeSelected(Mode),
  RunFlo(Opt),
  Exit,
  Refresh
}

#[derive(Debug)]
pub enum Message {
  Init(()),
  RunFlo(bool),
  RuntimeEvent(iced_native::Event),
  Interaction(Interaction),
  Error(anyhow::Error)
}

async fn init() {
  
}

fn apply_config(flo_ui: &mut Flo, config: Opt) {
  flo_ui.config = config;
}

impl Application for Flo {
  type Executor = iced::executor::Default;
  type Message = Message;
  type Flags = Opt;

  fn new(config: Opt) -> (Self, Command<Message>) {
    let init_commands = vec![
      Command::perform(init(), Message::Init),
    ];
    let mut flo_ui = Flo::default();

    apply_config(&mut flo_ui, config);

    (flo_ui, Command::batch(init_commands))
  }
  fn title(&self) -> String {
    String::from("Flo")
  }
  fn scale_factor(&self) -> f64 {
    1.0
  }
  fn update(&mut self, message: Message) -> Command<Message> {
    match update::handle_message(self, message) {
      Ok(x) => x,
      Err(e) => Command::perform(async { e }, Message::Error),
    }
  }
  fn view(&mut self) -> Element<Message> {

    let menu_container = element::menu::data_container( &self.config, &self.error, &mut self.settings_btn_state );

    let mut content = Column::new().push(menu_container);

    let run_flo_button: Element<Interaction> = Button::new(
        &mut self.run_btn_state,
        Text::new("Launch Flo Worker")
          .horizontal_alignment(HorizontalAlignment::Center)
          .size(24),
      )
      .style(style::DefaultBoxedButton())
      .on_press(Interaction::RunFlo(self.config.clone()))
      .into();

    let exit_button: Element<Interaction> = Button::new(
        &mut self.exit_btn_state,
        Text::new("Exit")
          .horizontal_alignment(HorizontalAlignment::Center)
          .size(24),
      )
      .style(style::DefaultBoxedButton())
      .on_press(Interaction::Exit)
      .into();

    let run_button_row = Row::new()
      .spacing(2)
      .push(run_flo_button.map(Message::Interaction));

    let ext_button_row = Row::new()
      .spacing(2)
      .push(exit_button.map(Message::Interaction));

    let run_flo_container = Container::new(run_button_row)
      .center_x()
      .center_y()
      .width(Length::Fill)
      .height(Length::Shrink)
      .style(style::DefaultStyle())
      .padding(10);

    let ext_flo_container = Container::new(ext_button_row)
      .center_x()
      .center_y()
      .width(Length::Fill)
      .height(Length::Shrink)
      .style(style::DefaultStyle())
      .padding(10);

    content = content.push(run_flo_container);
    content = content.push(ext_flo_container);

    Container::new(content)
      .width(Length::Fill)
      .height(Length::Fill)
      .style(style::DefaultStyle())
      .into()
  }
}

pub fn run(opts: Opt) {
  let mut settings = Settings::default();

  settings.antialiasing = true;
  settings.window.size = (300, 200);
  settings.window.resizable = false;
  settings.window.decorations = true;
  settings.flags = opts;

  Flo::run(settings).expect("Running Flo Gui");
}
