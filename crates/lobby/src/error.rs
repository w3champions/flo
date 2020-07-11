use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("player not found")]
  PlayerNotFound,
  #[error("game not found")]
  GameNotFound,
  #[error("only games with `Waiting` status are deletable")]
  GameNotDeletable,
  #[error("invalid game data, please re-create")]
  GameDataInvalid,
  #[error("db error: {0}")]
  Db(#[from] bs_diesel_utils::result::DbError),
  #[error("json: {0}")]
  Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<diesel::result::Error> for Error {
  fn from(e: diesel::result::Error) -> Self {
    Self::Db(e.into())
  }
}
