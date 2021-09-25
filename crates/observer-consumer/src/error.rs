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
  #[error("decode game record: {0}")]
  DecodeGameRecord(#[from] flo_observer::record::RecordError),
  #[error("Observer fs: {0}")]
  ObserverFs(#[from] flo_observer_fs::error::Error),
  #[error("Get shard iterator: {0}")]
  GetShardIterator(#[from] RusotoError<rusoto_kinesis::GetShardIteratorError>),
  #[error("redis: {0}")]
  Redis(#[from] redis::RedisError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("actor: {0}")]
  Actor(#[from] flo_state::error::Error),
  #[error("invalid S3 credentials: {0}")]
  InvalidS3Credentials(&'static str),
  #[error("json web token: {0}")]
  JsonWebToken(#[from] jsonwebtoken::errors::Error),
  #[error("observer token expired")]
  ObserverTokenExpired,
  #[error("get archived object: {0}")]
  GetArchivedObject(#[from] RusotoError<rusoto_s3::GetObjectError>),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
