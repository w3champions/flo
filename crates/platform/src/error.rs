use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("unable to determine the Warcraft III user data path")]
  NoUserDataPath,

  #[error("unable to determine the Warcraft III installation path")]
  NoInstallationFolder,

  #[error("config: {0}")]
  Config(#[from] flo_config::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
