use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("unable to determine the Warcraft III user data path")]
  NoUserDataPath,

  #[error("unable to determine the Warcraft III installation path")]
  NoInstallationFolder,

  #[error("unable to get Warcraft III version")]
  GetWar3Version,

  #[error("unable to get running Warcraft III path ({0})")]
  GetRunningWar3Path(u32),

  #[error("config: {0}")]
  Config(#[from] flo_config::error::Error),

  #[cfg(target_os = "macos")]
  #[error("plist: {0}")]
  PList(#[from] plist::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
