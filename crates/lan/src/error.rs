use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("invalid game info: {0}")]
  InvalidGameInfo(&'static str),

  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
  #[error("dns: {0}")]
  DNS(#[from] trust_dns_client::error::ClientError),
  #[error("w3gs: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("protobuf encode: {0}")]
  ProtoBufEncode(#[from] prost::EncodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
  #[error("base64 decode: {0}")]
  Base64Decode(#[from] base64::DecodeError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
