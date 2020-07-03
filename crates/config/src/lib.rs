use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub mod error;

use error::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientConfig {
  pub user_data_path: Option<PathBuf>,
  pub installation_path: Option<PathBuf>,
}

impl ClientConfig {
  pub fn from_env() -> Result<Self> {
    use std::env;
    Ok(Self {
      user_data_path: env::var("FLO_USER_DATA_PATH").ok().map(PathBuf::from),
      installation_path: env::var("FLO_INSTALLATION_PATH").ok().map(PathBuf::from),
    })
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
