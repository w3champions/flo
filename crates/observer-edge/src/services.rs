use crate::controller::Controller;
use flo_observer_archiver::ArchiverHandle;

#[derive(Clone)]
pub struct Services {
  pub controller: Controller,
  pub archiver: Option<ArchiverHandle>,
}

impl Services {
  pub fn from_env() -> Self {
    Self {
      controller: Controller::from_env(),
      archiver: None,
    }
  }
}
