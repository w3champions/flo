use flo_config::ClientConfig;
use std::path::PathBuf;

#[cfg(windows)]
mod windows_bindings;

pub mod error;
mod path;
mod war3;

use error::*;

#[derive(Debug, Clone)]
pub struct ClientPlatformInfo {
  pub user_data_path: PathBuf,
  pub installation_path: PathBuf,
  pub version: String,
  pub executable_path: PathBuf,
}

impl ClientPlatformInfo {
  #[cfg(windows)]
  pub fn with_config(config: &ClientConfig) -> Result<Self> {
    {
      // first try running process
      let running_executable_path = war3::get_running_war3_executable_path()
        .ok()
        .and_then(|s| s);
      if let Some(executable_path) = running_executable_path {
        let version = crate::war3::get_war3_version(&executable_path).ok();
        if let Some(version) = version {
          return Ok(ClientPlatformInfo {
            user_data_path: config
              .user_data_path
              .clone()
              .or_else(|| path::detect_user_data_path())
              .ok_or_else(|| Error::NoUserDataPath)?,
            installation_path: executable_path
              // parent folder
              .parent()
              // x86_64
              .and_then(|p| p.parent())
              // _retail_
              .and_then(|p| p.parent())
              .ok_or_else(|| Error::NoInstallationFolder)?
              .to_owned(),
            version,
            executable_path,
          });
        }
      }
    }

    let installation_path = config
      .installation_path
      .clone()
      .or_else(|| path::detect_installation_path())
      .ok_or_else(|| Error::NoInstallationFolder)?;

    let executable_path = installation_path.join("_retail_/x86_64/Warcraft III.exe");
    let version = crate::war3::get_war3_version(&executable_path)?;

    Ok(ClientPlatformInfo {
      user_data_path: config
        .user_data_path
        .clone()
        .or_else(|| path::detect_user_data_path())
        .ok_or_else(|| Error::NoUserDataPath)?,
      installation_path,
      version,
      executable_path,
    })
  }

  #[cfg(target_os = "macos")]
  pub fn with_config(config: &ClientConfig) -> Result<Self> {
    let installation_path = config
      .installation_path
      .clone()
      .or_else(|| path::detect_installation_path())
      .ok_or_else(|| Error::NoInstallationFolder)?;

    let executable_path = installation_path.join("_retail_/x86_64/Warcraft III.app");

    tracing::debug!("executable_path: {:?}", executable_path);

    let version = crate::war3::get_war3_version(&executable_path)?;

    tracing::debug!("version: {:?}", version);

    let user_data_path = config
      .user_data_path
      .clone()
      .or_else(|| path::detect_user_data_path())
      .ok_or_else(|| Error::NoUserDataPath)?;

    tracing::debug!("user_data_path: {:?}", user_data_path);

    Ok(ClientPlatformInfo {
      user_data_path,
      installation_path,
      version,
      executable_path,
    })
  }

  #[cfg(target_os = "linux")]
  pub fn with_config(config: &ClientConfig) -> Result<Self> {
    let installation_path = config
      .installation_path
      .clone()
      .ok_or_else(|| Error::NoInstallationFolder)?;

    let executable_path = installation_path.join("_retail_/x86_64/Warcraft III.exe");
    tracing::debug!("executable_path: {:?}", executable_path);

    let user_data_path = config
      .user_data_path
      .clone()
      .ok_or_else(|| Error::NoUserDataPath)?;
    tracing::debug!("user_data_path: {:?}", user_data_path);

    //TODO: maybe put version into config for Linux
    Ok(ClientPlatformInfo {
      user_data_path,
      installation_path,
      version: String::from("1.32.6"),
      executable_path,
    })
  }

  pub fn from_env() -> Result<Self> {
    dotenv::dotenv().ok();
    let config = ClientConfig::from_env()?;
    Self::with_config(&config)
  }
}
