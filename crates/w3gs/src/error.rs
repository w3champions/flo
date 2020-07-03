use thiserror::Error;

use crate::protocol::constants::PacketTypeId;

#[derive(Error, Debug)]
pub enum Error {
  #[error("stream closed unexpectedly")]
  StreamClosed,
  #[error("IPv6 is not supported")]
  Ipv6NotSupported,
  #[error("payload size overflow")]
  PayloadSizeOverflow,
  #[error("invalid packet length: {0}")]
  InvalidPacketLength(u16),
  #[error("invalid payload length: {0}")]
  InvalidPayloadLength(usize),
  #[error("invalid state: no header")]
  InvalidStateNoHeader,
  #[error("an interior nul byte was found")]
  InvalidStringNulByte(#[from] std::ffi::NulError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("unexpected bytes after payload: {0}")]
  ExtraPayloadBytes(usize),
  #[error("packet type id mismatch: expected `{expected:?}`, found `{found:?}`")]
  PacketTypeIdMismatch {
    expected: PacketTypeId,
    found: PacketTypeId,
  },
  #[error("invalid checksum")]
  InvalidChecksum,
  #[error("bin decode: {0}")]
  BinDecode(#[from] flo_util::binary::BinDecodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
