use crate::controller::Controller;

#[derive(Debug, Clone)]
pub struct Services {
  pub controller: Controller,
}

impl Services {
  pub fn from_env() -> Self {
    Self {
      controller: Controller::from_env(),
    }
  }
}