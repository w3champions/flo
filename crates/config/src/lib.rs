use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub mod error;

use error::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientConfig {
  pub local_port: u16,
  pub user_data_path: Option<PathBuf>,
  pub installation_path: Option<PathBuf>,
  pub lobby_domain: String,
}

impl Default for ClientConfig {
  fn default() -> Self {
    ClientConfig {
      local_port: flo_constants::CONNECT_WS_PORT,
      user_data_path: None,
      installation_path: None,
      lobby_domain: flo_constants::LOBBY_DOMAIN.to_string(),
    }
  }
}

impl ClientConfig {
  pub fn from_env() -> Result<Self> {
    use std::env;
    let mut config = ClientConfig::default();

    if let Ok(Some(port)) = env::var("FLO_LOCAL_PORT")
      .ok()
      .map(|v| v.parse())
      .transpose()
    {
      config.local_port = port;
    }

    if let Some(path) = env::var("FLO_USER_DATA_PATH").ok().map(PathBuf::from) {
      config.user_data_path = Some(path);
    }

    if let Some(path) = env::var("FLO_INSTALLATION_PATH").ok().map(PathBuf::from) {
      config.installation_path = Some(path);
    }

    if let Some(domain) = env::var("FLO_LOBBY_DOMAIN").ok() {
      config.lobby_domain = domain;
    }

    Ok(config)
  }

  pub fn load() -> Result<Self> {
    toml::from_str(&fs::read_to_string("flo.toml")?).map_err(Into::into)
  }

  pub fn save(&self) -> Result<()> {
    fs::write("flo.toml", toml::to_string_pretty(self)?).map_err(Into::into)
  }
}

#[test]
fn test_client() {
  let config: ClientConfig = toml::from_str(
    r#"user_data_path = "C:\\Users\\fluxx\\OneDrive\\Documents\\Warcraft III"
installation_path = "C:\\Program Files (x86)\\Warcraft III""#,
  )
  .unwrap();
  dbg!(&config);

  dbg!(toml::to_string_pretty(&config).unwrap());
}
