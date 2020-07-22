use thiserror::Error;

use crate::packet::PacketTypeId;

#[derive(Error, Debug)]
pub enum Error {
  #[error("stream closed unexpectedly")]
  StreamClosed,
  #[error("unexpected packet type: expected {expected:?}, got {got:?}")]
  UnexpectedPacketType {
    expected: PacketTypeId,
    got: PacketTypeId,
  },

  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("decode: {0}")]
  Decode(#[from] flo_util::binary::BinDecodeError),
  #[error("protobuf decode: {0}")]
  ProtoBufDecode(#[from] prost::DecodeError),
  #[error("protobuf encode: {0}")]
  ProtoBufEncode(#[from] prost::EncodeError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
