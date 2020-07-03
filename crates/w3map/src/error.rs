use flo_util::binary::BinDecodeError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("storage file not found")]
  StorageFileNotFound,
  #[error("storage: {0}")]
  Storage(#[from] flo_w3storage::error::Error),
  #[error("stormlib: {0}")]
  Storm(#[from] stormlib::error::StormError),
  #[error("ceres_mpq: {0}")]
  CeresMpq(#[from] ceres_mpq::Error),
  #[error("invalid utf8 bytes: {0}")]
  Utf8(#[from] std::str::Utf8Error),
  #[error("read map info: {0}")]
  ReadInfo(BinDecodeError),
  #[error("read map image: {0}")]
  ReadImage(BinDecodeError),
  #[error("read map minimap icons: {0}")]
  ReadMinimapIcons(BinDecodeError),
  #[error("read map trigger strings: {0}")]
  ReadTriggerStrings(BinDecodeError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
