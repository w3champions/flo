use super::Opt;

mod element;
mod style;
mod update;

use image::ImageFormat;

use iced::{
  button, text_input, Application, Button, Column, Command, Container, Element,
  HorizontalAlignment, Length, Row, Settings, Text,
};

static WINDOW_ICON: &[u8] = include_bytes!("../../resources/flo.ico");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
  Main,
  Running,
  Settings,
}

pub struct Flo {
  config: Opt,
  run_btn_state: button::State,
  web_btn_state: button::State,
  exit_btn_state: button::State,
  settings_btn_state: button::State,
  confirm_settings_button_state: button::State,
  token_input_state: text_input::State,
  error: Option<String>,
  flo_running: bool,
  version: String,
  mode: Mode,
  return_page: Mode,
  port: Option<String>,
}

impl Default for Flo {
  fn default() -> Self {
    Self {
      config: Default::default(),
      run_btn_state: Default::default(),
      web_btn_state: Default::default(),
      exit_btn_state: Default::default(),
      settings_btn_state: Default::default(),
      confirm_settings_button_state: Default::default(),
      token_input_state: Default::default(),
      error: None,
      flo_running: false,
      version: flo_client::FLO_VERSION.to_string(),
      mode: Mode::Main,
      return_page: Mode::Main,
      port: None,
    }
  }
}

#[derive(Debug, Clone)]
pub enum Interaction {
  ModeSelected(Mode),
  RunFlo(Opt),
  UpdateTokenConfirm,
  OpenWeb(String),
  Exit,
}

#[derive(Debug, Clone)]
pub enum Message {
  Init(()),
  RunFlo((bool, String)),
  Interaction(Interaction),
  Error(String),
  UpdateToken(String),
  UpdateTokenConfirm(()),
  FloWeb(bool),
}

async fn init() {
  // do something maybe?
}

fn apply_config(flo_ui: &mut Flo, config: Opt) {
  flo_ui.config = config;
}

impl Application for Flo {
  type Executor = iced::executor::Default;
  type Message = Message;
  type Flags = Opt;

  fn new(config: Opt) -> (Self, Command<Message>) {
    let init_commands = vec![Command::perform(init(), Message::Init)];
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
      Err(e) => Command::perform(async move { e.to_string() }, Message::Error),
    }
  }
  fn view(&mut self) -> Element<Message> {
    let menu_container = element::menu::data_container(
      &self.config,
      &self.error,
      &mut self.settings_btn_state,
      &self.version,
    );

    let mut content = Column::new().push(menu_container);

    match &self.mode {
      Mode::Main => {
        let run_flo_button: Element<Interaction> = Button::new(
          &mut self.run_btn_state,
          Text::new("Run Hostbot Client")
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
      }

      Mode::Running => {
        if let Some(port) = &self.port {
          let running_str = format!("Running on {} port", port);
          let running_text = Text::new(running_str).size(24);

          let exit_button: Element<Interaction> = Button::new(
            &mut self.exit_btn_state,
            Text::new("Exit")
              .horizontal_alignment(HorizontalAlignment::Center)
              .size(24),
          )
          .style(style::DefaultBoxedButton())
          .on_press(Interaction::Exit)
          .into();

          let ext_button_row = Row::new()
            .spacing(2)
            .push(exit_button.map(Message::Interaction));

          let ext_flo_container = Container::new(ext_button_row)
            .center_x()
            .center_y()
            .width(Length::Fill)
            .height(Length::Shrink)
            .style(style::DefaultStyle())
            .padding(10);

          let run_text_container = Container::new(running_text)
            .center_x()
            .center_y()
            .width(Length::Fill)
            .height(Length::Shrink)
            .style(style::DefaultStyle())
            .padding(10);

          content = content.push(run_text_container);

          if self.config.use_flo_web {
            let web_str = format!("https://w3flo.com/setup?port={}", port);

            let web_button: Element<Interaction> = Button::new(
              &mut self.web_btn_state,
              Text::new(web_str.clone())
                .horizontal_alignment(HorizontalAlignment::Center)
                .size(16),
            )
            .style(style::DefaultButton())
            .on_press(Interaction::OpenWeb(web_str))
            .into();

            let web_button_row = Row::new()
              .spacing(2)
              .push(web_button.map(Message::Interaction));

            let web_flo_container = Container::new(web_button_row)
              .center_x()
              .center_y()
              .width(Length::Fill)
              .height(Length::Shrink)
              .style(style::DefaultStyle())
              .padding(10);

            content = content.push(web_flo_container);
          }

          content = content.push(ext_flo_container);
        }
      }

      Mode::Settings => {
        let settings_container = element::settings::data_container(
          &mut self.config,
          &mut self.confirm_settings_button_state,
          &mut self.token_input_state,
        );
        content = content.push(settings_container);
      }
    };

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

  #[allow(deprecated)]
  let image = image::load_from_memory_with_format(WINDOW_ICON, ImageFormat::Ico)
    .expect("loading icon")
    .to_rgba();
  let (width, height) = image.dimensions();
  let icon = iced::window::Icon::from_rgba(image.into_raw(), width, height);
  settings.window.icon = Some(icon.unwrap());

  Flo::run(settings).expect("Running Flo Gui");
}
