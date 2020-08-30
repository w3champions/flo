use thiserror::Error;

use crate::packet::PacketTypeId;
use crate::w3gs::ParseW3GSPacketError;

#[derive(Error, Debug)]
pub enum Error {
  #[error("payload too large")]
  PayloadTooLarge,
  #[error("payload too small")]
  PayloadTooSmall,
  #[error("stream timed out")]
  StreamTimeout,
  #[error("stream closed")]
  StreamClosed,
  #[error("unexpected packet type: expected {expected:?}, got {got:?}")]
  UnexpectedPacketType {
    expected: PacketTypeId,
    got: PacketTypeId,
  },
  #[error("unexpected packet type: {got:?}")]
  UnexpectedPacketTypeId { got: PacketTypeId },
  #[error("packet field not present")]
  PacketFieldNotPresent,
  #[error("task cancelled unexpectedly")]
  Cancelled,
  #[error("invalid W3GS frame")]
  ReadW3GSFrame(ParseW3GSPacketError),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("decode: {0}")]
  Decode(#[from] flo_util::binary::BinDecodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
  #[error("protobuf encode: {0}")]
  ProtoBufEncode(#[from] prost::EncodeError),
}

impl Error {
  pub fn unexpected_packet_type_id(got: PacketTypeId) -> Self {
    Self::UnexpectedPacketTypeId { got }
  }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
