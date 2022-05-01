use rusoto_core::RusotoError;
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
  #[error("game version unknown")]
  GameVersionUnknown,
  #[error("peer lagged: {0} events dropped")]
  ObserverPeerLagged(u64),
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
  #[error("proto: {0}")]
  Proto(#[from] s2_grpc_utils::result::Error),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("get archived object: {0}")]
  GetArchivedObject(#[from] RusotoError<rusoto_s3::GetObjectError>),
  #[error("invalid S3 credentials: {0}")]
  InvalidS3Credentials(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
