#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod flo;
mod gui;
mod log;

use structopt::StructOpt;
use tracing::instrument;

use crate::core::*;

#[instrument]
pub fn main() {
  #[cfg(windows)]
  unsafe {
    winapi::um::processthreadsapi::SetPriorityClass(
      winapi::um::processthreadsapi::GetCurrentProcess(),
      winapi::um::winbase::ABOVE_NORMAL_PRIORITY_CLASS,
    );
  }

  let mut opt = Default::default();

  if let Ok(json_file) = std::fs::File::open(JSON_CONF_FNAME) {
    if let Ok(json_conf) = serde_json::from_reader::<_, Opt>(json_file) {
      opt = json_conf;
    }
  }

  //TODO: recode that part, not fully sure how
  let opt_from_args = Opt::from_args();
  if opt_from_args.debug {
    opt.debug = true;
  }
  if let Some(token) = opt_from_args.token {
    opt.token = Some(token);
  }
  if let Some(installation_path) = opt_from_args.installation_path {
    opt.installation_path = Some(installation_path);
  }
  if let Some(user_data_path) = opt_from_args.user_data_path {
    opt.installation_path = Some(user_data_path);
  }
  if let Some(controller_host) = opt_from_args.controller_host {
    opt.controller_host = Some(controller_host);
  }

  log::init(opt.debug);

  tracing::warn!("Running GUI");

  gui::run(opt);
}
