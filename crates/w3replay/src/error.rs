use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("read header: {0}")]
  ReadHeader(std::io::Error),
  #[error("unsupported block size, expected 8192, got {0}")]
  UnsupportedBlockSize(usize),
  #[error("read block header: {0}")]
  ReadBlockHeader(std::io::Error),
  #[error("invalid checksum: subject = {subject}, expected = {expected}, got = {got}")]
  InvalidChecksum {
    subject: &'static str,
    expected: u16,
    got: u16
  },
  #[error("no game info record")]
  NoGameInfoRecord,
  #[error("no slot info record")]
  NoSlotInfoRecord,
  #[error("decompress: {0}")]
  Decompress(#[from] flate2::DecompressError),
  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
