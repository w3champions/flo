use crate::game::{AckError, SlotClientStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("cancelled")]
  Cancelled,
  #[error("game exists")]
  GameExists,
  #[error("game desync: {0:?}")]
  GameDesync(#[from] AckError),
  #[error("game has no player")]
  NoPlayer,
  #[error("player busy: {0}")]
  PlayerBusy(i32),
  #[error("player not found in game")]
  PlayerNotFoundInGame,
  #[error("player connection exists")]
  PlayerConnectionExists,
  #[error("player channel broken")]
  PlayerChannelBroken,
  #[error("player already left")]
  PlayerAlreadyLeft,
  #[error("invalid player slot client status: {0:?}")]
  InvalidPlayerSlotClientStatus(SlotClientStatus),
  #[error("invalid slot id")]
  InvalidSlotId,
  #[error("invalid secret")]
  InvalidSecret,
  #[error("invalid token")]
  InvalidToken,
  #[error("invalid client status transition: {0:?} => {1:?}")]
  InvalidClientStatusTransition(SlotClientStatus, SlotClientStatus),
  #[error("observer put record: {0}")]
  ObsPutRecord(#[from] rusoto_core::RusotoError<rusoto_kinesis::PutRecordError>),
  #[error("tokio io: {0}")]
  Tokio(#[from] tokio::io::Error),
  #[error("operation timeout")]
  Timeout(#[from] tokio::time::error::Elapsed),
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("proto: {0}")]
  Proto(#[from] s2_grpc_utils::result::Error),
  #[error("http: {0}")]
  Http(#[from] hyper::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
