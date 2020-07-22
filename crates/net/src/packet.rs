use bytes::{Buf, Bytes, BytesMut};
use prost::Message;

use flo_util::{BinDecode, BinEncode};

use crate::error::{Error, Result};

pub trait FloPacket
where
  Self: Message + Sized,
{
  const TYPE_ID: PacketTypeId;

  fn encode_as_frame(self) -> Result<Frame> {
    let payload_len = self.encoded_len();
    let mut buf = BytesMut::with_capacity(payload_len);
    self.encode(&mut buf)?;
    Ok(Frame {
      type_id: Self::TYPE_ID,
      payload: buf.freeze(),
    })
  }
}

#[derive(Debug, BinDecode)]
pub(crate) struct Header {
  pub type_id: PacketTypeId,
  pub payload_len: u16,
}

#[derive(Debug)]
pub struct Frame {
  pub type_id: PacketTypeId,
  pub payload: Bytes,
}

impl Frame {
  pub fn decode<T>(self) -> Result<T>
  where
    T: FloPacket + Default,
  {
    if self.type_id != T::TYPE_ID {
      return Err(Error::UnexpectedPacketType {
        expected: T::TYPE_ID,
        got: self.type_id,
      });
    }

    T::decode(self.payload).map_err(Into::into)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum PacketTypeId {
  #[bin(value = 0x01)]
  Ping,
  #[bin(value = 0x02)]
  Pong,
  #[bin(value = 0x03)]
  ConnectLobby,
  #[bin(value = 0x04)]
  ConnectLobbyAccept,
  #[bin(value = 0x05)]
  ConnectLobbyReject,
  #[bin(value = 0x06)]
  LobbyDisconnect,
  UnknownValue(u8),
}

macro_rules! packet_type {
  (
    $type_id:ident, $packet:path
  ) => {
    impl crate::packet::FloPacket for $packet {
      const TYPE_ID: crate::packet::PacketTypeId = crate::packet::PacketTypeId::$type_id;
    }
  };
}

packet_type!(Ping, crate::proto::flo_common::PacketPing);
packet_type!(Pong, crate::proto::flo_common::PacketPong);
