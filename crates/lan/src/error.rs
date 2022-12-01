use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("invalid version string: {0}")]
  InvalidVersionString(String),
  #[error("invalid game info: {0}")]
  InvalidGameInfo(&'static str),
  #[error("bonjour register: {0}")]
  BonjourRegister(std::io::Error),
  #[error("bonjour update: {0}")]
  BonjourUpdate(String),
  #[error("get hostname: {0}")]
  GetHostName(std::io::Error),
  #[error("couldn't find game info record in the replay file")]
  ReplayNoGameInfoRecord,
  #[error("the game info record in the replay file is invalid")]
  ReplayInvalidGameInfoRecord,
  #[error("string contains null byte")]
  NullByteInString,
  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("platform: {0}")]
  Platform(#[from] flo_platform::error::Error),
  #[error("protobuf encode: {0}")]
  ProtoBufEncode(#[from] prost::EncodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
  #[error("base64 decode: {0}")]
  Base64Decode(#[from] base64::DecodeError),
  #[error("replay: {0}")]
  Replay(#[from] flo_w3replay::error::Error),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
