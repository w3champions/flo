use rusoto_core::RusotoError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Invalid buffer file")]
  InvalidBufferFile,
  #[error("Invalid chunk file")]
  InvalidChunkFile,
  #[error("No shards")]
  NoShards,
  #[error("List shards iterator: {0}")]
  ListShards(#[from] RusotoError<rusoto_kinesis::ListShardsError>),
  #[error("No shard iterator: {0}")]
  NoShardIterator(String),
  #[error("Game data lost: {0}")]
  GameDataLost(i32),
  #[error("Get shard iterator: {0}")]
  GetShardIterator(#[from] RusotoError<rusoto_kinesis::GetShardIteratorError>),
  #[error("decode game record: {0}")]
  DecodeGameRecord(#[from] flo_observer::record::RecordError),
  #[error("decode archive header: {0}")]
  DecodeArchiveHeader(flo_util::binary::BinDecodeError),
  #[error("redis: {0}")]
  Redis(#[from] redis::RedisError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("actor: {0}")]
  Actor(#[from] flo_state::error::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
