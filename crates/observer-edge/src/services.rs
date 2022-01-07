use crate::controller::Controller;

#[derive(Debug, Clone)]
pub struct Services {
  pub controller: Controller,
}

impl Services {
  pub fn env() -> Self {
    Self {
      controller: Controller::env(),
    }
  }
}