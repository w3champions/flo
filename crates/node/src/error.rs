use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("game exists")]
  GameExists,
  #[error("game has no player")]
  NoPlayer,
  #[error("player busy: {0}")]
  PlayerBusy(i32),
  #[error("invalid secret")]
  InvalidSecret,
  #[error("invalid token")]
  InvalidToken,
  #[error("tokio io: {0}")]
  Tokio(#[from] tokio::io::Error),
  #[error("operation timeout")]
  Timeout(#[from] tokio::time::Elapsed),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("proto: {0}")]
  Proto(#[from] s2_grpc_utils::result::Error),
  #[error("http: {0}")]
  Http(#[from] hyper::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
