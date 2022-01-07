use rusoto_core::RusotoError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Cancelled")]
  Cancelled,
  #[error("No shards")]
  NoShards,
  #[error("List shards iterator: {0}")]
  ListShards(#[from] RusotoError<rusoto_kinesis::ListShardsError>),
  #[error("No shard iterator: {0}")]
  NoShardIterator(String),
  #[error("Game data lost: {0}")]
  GameDataLost(i32),
  #[error("decode game record: {0}")]
  DecodeGameRecord(#[from] flo_observer::record::RecordError),
  #[error("Get shard iterator: {0}")]
  GetShardIterator(#[from] RusotoError<rusoto_kinesis::GetShardIteratorError>),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
