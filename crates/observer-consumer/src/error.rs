use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("decode game record: {0}")]
  DecodeGameRecord(#[from] flo_observer::record::RecordError),
  #[error("decode archive header: {0}")]
  DecodeArchiveHeader(flo_util::binary::BinDecodeError),
  #[error("redis: {0}")]
  Redis(#[from] redis::RedisError),
  #[error("redis: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
