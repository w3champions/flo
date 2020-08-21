use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("invalid game info: {0}")]
  InvalidGameInfo(&'static str),
  #[error("mdns stream is broken: {0}")]
  MdnsStreamBroken(std::io::Error),
  #[error("mdns broadcast: {0}")]
  MdnsBroadcastError(String),
  #[error("mdns update game info: {0}")]
  MdnsUpdateGameInfo(&'static str),
  #[error("get hostname: {0}")]
  GetHostName(std::io::Error),
  #[error("couldn't find game info record in the replay file")]
  ReplayNoGameInfoRecord,
  #[error("the game info record in the replay file is invalid")]
  ReplayInvalidGameInfoRecord,
  #[error("string contains null byte")]
  NullByteInString,
  #[error("get ip: {0}")]
  GetIp(#[from] ipconfig::error::Error),
  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
  #[error("dns client: {0}")]
  DNSClient(#[from] trust_dns_client::error::ClientError),
  #[error("dns protocol: {0}")]
  DNSProtocol(#[from] trust_dns_client::proto::error::ProtoError),
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("protobuf encode: {0}")]
  ProtoBufEncode(#[from] prost::EncodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
  #[error("base64 decode: {0}")]
  Base64Decode(#[from] base64::DecodeError),
  #[error("replay: {0}")]
  Replay(#[from] flo_w3replay::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
