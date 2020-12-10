use {
  crate::flo,
  super::*
};

pub fn handle_message(flo_ui: &mut Flo, message: Message) -> anyhow::Result<Command<Message>> {
  match message {
    Message::Init(_) => {
      
    }
    Message::RunFlo(res) => {
      if res {
        flo_ui.flo_running = true;
        tracing::info!("flor is running with success");
      } else {
        tracing::error!("failed to run flo");
      }
    },
    Message::Interaction(Interaction::RunFlo(opt)) => {
      if !flo_ui.flo_running {
        tracing::info!("Running flo");
        return Ok(Command::perform(
          flo::perform_run_flo(opt),
          Message::RunFlo,
        ));
      }
    },
    Message::Interaction(Interaction::Refresh) => {
      
    }
    Message::Interaction(Interaction::ModeSelected(_mode)) => {

    }
    Message::Interaction(Interaction::Exit) => {
      std::process::exit(0);
    }
    Message::Error(error) => {
      flo_ui.error = Some(error);
    },
    Message::RuntimeEvent(_) => {}
  }
  Ok(Command::none())
}
