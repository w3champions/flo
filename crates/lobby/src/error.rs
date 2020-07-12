use thiserror::Error;
use tonic::Status;

#[derive(Error, Debug)]
pub enum Error {
  #[error("player token expired")]
  PlayerTokenExpired,
  #[error("player not found")]
  PlayerNotFound,
  #[error("game not found")]
  GameNotFound,
  #[error("only games with `Waiting` status are deletable")]
  GameNotDeletable,
  #[error("invalid game data, please re-create")]
  GameDataInvalid,
  #[error("this map has no player slot")]
  MapHasNoPlayer,
  #[error("db error: {0}")]
  Db(#[from] bs_diesel_utils::result::DbError),
  #[error("json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("json web token: {0}")]
  JsonWebToken(#[from] jsonwebtoken::errors::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for String {
  fn from(e: Error) -> Self {
    format!("{}", e)
  }
}

impl From<diesel::result::Error> for Error {
  fn from(e: diesel::result::Error) -> Self {
    Self::Db(e.into())
  }
}
