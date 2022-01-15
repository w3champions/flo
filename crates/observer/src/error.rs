use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("observer token expired")]
  ObserverTokenExpired,
  #[error("json web token: {0}")]
  JsonWebToken(#[from] jsonwebtoken::errors::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
