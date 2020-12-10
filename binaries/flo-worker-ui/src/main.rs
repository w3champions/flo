mod flo;
mod log;
mod gui;

use structopt::StructOpt;
use std::path::PathBuf;

use tracing::{ instrument };

#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "flo-worker", about = "Flo worker process.")]
pub struct Opt {
  #[structopt(long)]
  debug: bool,

  #[structopt(long)]
  token: Option<String>,

  #[structopt(long, parse(from_os_str))]
  installation_path: Option<PathBuf>,

  #[structopt(long)]
  controller_host: Option<String>,
}

impl Default for Opt {
  fn default() -> Self {
    Self {
      debug: false,
      token: None,
      installation_path: None,
      controller_host: None
    }
  }
}

#[instrument]
pub fn main() {
  #[cfg(windows)]
  unsafe {
    winapi::um::processthreadsapi::SetPriorityClass(
      winapi::um::processthreadsapi::GetCurrentProcess(),
      winapi::um::winbase::HIGH_PRIORITY_CLASS,
    );
  }

  //TODO: load options from conf file first

  let opt = Opt::from_args();
  log::init(opt.debug);

  tracing::warn!("Running GUI");

  gui::run(opt);
}
