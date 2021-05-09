use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Invalid buffer file")]
  InvalidBufferFile,
  #[error("Invalid chunk file")]
  InvalidChunkFile,
  #[error("decode game record: {0}")]
  DecodeGameRecord(#[from] flo_observer::record::RecordError),
  #[error("decode archive header: {0}")]
  DecodeArchiveHeader(flo_util::binary::BinDecodeError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
