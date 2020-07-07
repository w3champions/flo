use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("unsupported block size, expected 8192, got {0}")]
  UnsupportedBlockSize(usize),
  #[error("read block header: {0}")]
  ReadBlockHeader(std::io::Error),
  #[error("invalid checksum")]
  InvalidChecksum,
  #[error("decompress: {0}")]
  Decompress(#[from] flate2::DecompressError),
  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
