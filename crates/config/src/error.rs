use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("io: {0}")]
  Io(#[from] std::io::Error),

  #[error("toml serialize: {0}")]
  TomlSer(#[from] toml::ser::Error),

  #[error("toml deserialize: {0}")]
  TomlDe(#[from] toml::de::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
