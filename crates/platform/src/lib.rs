use flo_config::ClientConfig;
use std::path::PathBuf;

pub mod error;
mod path;

use error::*;

#[derive(Debug)]
pub struct ClientPlatformInfo {
  pub user_data_path: PathBuf,
  pub installation_path: PathBuf,
}

impl ClientPlatformInfo {
  pub fn with_config(config: &ClientConfig) -> Result<Self> {
    Ok(ClientPlatformInfo {
      user_data_path: config
        .user_data_path
        .clone()
        .or_else(|| path::detect_user_data_path())
        .ok_or_else(|| Error::NoUserDataPath)?,
      installation_path: config
        .installation_path
        .clone()
        .or_else(|| path::detect_installation_path())
        .ok_or_else(|| Error::NoInstallationFolder)?,
    })
  }

  pub fn from_env() -> Result<Self> {
    dotenv::dotenv().ok();
    let config = ClientConfig::from_env()?;
    Self::with_config(&config)
  }
}
