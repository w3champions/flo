use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("game not ready: {0}")]
  GameNotReady(String),
  #[error("game not found: {0}")]
  GameNotFound(i32),
  #[error("invalid game id: {0}")]
  InvalidGameId(i32),
  #[error("unexpected game records: {expected} << {range:?} {len}")]
  UnexpectedGameRecords {
    expected: u32,
    range: [u32; 2],
    len: usize,
  },
  #[error("controller service: {0}")]
  ControllerService(tonic::Status),
  #[error("kinesis: {0}")]
  Kinesis(#[from] flo_kinesis::error::Error),
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("actor: {0}")]
  Actor(#[from] flo_state::error::Error),
  #[error("json web token: {0}")]
  JsonWebToken(#[from] jsonwebtoken::errors::Error),
  #[error("proto: {0}")]
  Proto(#[from] s2_grpc_utils::result::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
