use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("parse error: {0}")]
  Parse(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum BinDecodeError {
  #[error("not enough data")]
  Incomplete,
  #[error("invalid data")]
  Failure(String),
}

impl BinDecodeError {
  pub fn failure<T>(msg: T) -> Self
  where
    T: ToString,
  {
    BinDecodeError::Failure(msg.to_string())
  }
}
