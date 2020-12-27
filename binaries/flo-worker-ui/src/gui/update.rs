use {super::*, crate::core::JSON_CONF_FNAME, crate::flo};

pub async fn save_settings() {
  // just processing settings before saving
}

pub fn handle_message(flo_ui: &mut Flo, message: Message) -> anyhow::Result<Command<Message>> {
  match message {
    Message::Init(_) => {
      // message from UI after init complete
    }
    Message::UpdateTokenConfirm(_) => {
      if let Ok(s_to_save) = serde_json::to_string_pretty(&flo_ui.config) {
        if let Err(why) = std::fs::write(JSON_CONF_FNAME, s_to_save) {
          flo_ui.error = Some(why.to_string());
        }
      }
    }
    Message::UpdateToken(s) => {
      flo_ui.config.token = Some(s);
    }
    Message::FloWeb(checked) => {
      flo_ui.config.use_flo_web = checked;
    }
    Message::RunFlo((res, s)) => {
      if res {
        flo_ui.flo_running = true;
        flo_ui.port = Some(s);
        flo_ui.mode = Mode::Running;
        tracing::info!("flo is running with success");
      } else {
        flo_ui.error = Some(s);
        tracing::error!("failed to run flo");
      }
    }
    Message::Interaction(Interaction::RunFlo(opt)) => {
      if !flo_ui.flo_running {
        tracing::debug!("Running flo");
        return Ok(Command::perform(flo::perform_run_flo(opt), Message::RunFlo));
      }
    }
    Message::Interaction(Interaction::ModeSelected(mode)) => {
      tracing::debug!("Interaction::ModeSelected({:?})", mode);
      if flo_ui.mode == Mode::Settings && mode == Mode::Settings {
        flo_ui.mode = flo_ui.return_page;
      } else if flo_ui.mode != Mode::Settings && mode == Mode::Settings {
        flo_ui.return_page = flo_ui.mode;
        flo_ui.mode = mode;
      } else {
        flo_ui.mode = mode;
      }
    }
    Message::Interaction(Interaction::UpdateTokenConfirm) => {
      flo_ui.mode = flo_ui.return_page;
      return Ok(Command::perform(
        save_settings(),
        Message::UpdateTokenConfirm,
      ));
    }
    Message::Interaction(Interaction::OpenWeb(w)) => {
      if let Err(why) = webbrowser::open(&w) {
        flo_ui.error = Some(why.to_string());
      }
    }
    Message::Interaction(Interaction::Exit) => {
      std::process::exit(0);
    }
    Message::Error(error) => {
      flo_ui.error = Some(error);
    }
  }
  Ok(Command::none())
}
