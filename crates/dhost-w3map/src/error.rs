use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("stormlib: {0}")]
  Storm(#[from] stormlib::error::StormError),
  #[error("invalid utf8 bytes: {0}")]
  Utf8(#[from] std::str::Utf8Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
