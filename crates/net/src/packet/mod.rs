use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::time::error::Elapsed;
pub use prost::Message;

use flo_util::{BinDecode, BinEncode};

use crate::error::{Error, Result};

pub trait FloPacket
where
  Self: Message + Sized,
{
  const TYPE_ID: PacketTypeId;

  fn encode_as_frame(&self) -> Result<Frame> {
    let payload_len = self.encoded_len();
    let mut buf = BytesMut::with_capacity(payload_len);
    self.encode(&mut buf)?;
    Ok(Frame {
      type_id: Self::TYPE_ID,
      payload: FramePayload::Bytes(buf.freeze()),
    })
  }
}

#[derive(Debug, BinDecode)]
pub(crate) struct Header {
  pub type_id: PacketTypeId,
  pub payload_len: u16,
}

#[derive(Debug, Clone)]
pub struct Frame {
  pub type_id: PacketTypeId,
  pub payload: FramePayload,
}

#[derive(Debug, Clone)]
pub enum FramePayload {
  Bytes(Bytes),
  SubTypeBytes(u8, Bytes),
}

impl FramePayload {
  pub fn len(&self) -> usize {
    match *self {
      FramePayload::Bytes(ref bytes) => bytes.len(),
      FramePayload::SubTypeBytes(_, ref bytes) => 1 + bytes.len(),
    }
  }

  pub fn subtype_id(&self) -> Option<u8> {
    match *self {
      FramePayload::Bytes(_) => None,
      FramePayload::SubTypeBytes(id, _) => Some(id),
    }
  }

  pub fn into_subtype(self) -> Option<(u8, Bytes)> {
    match self {
      FramePayload::Bytes(_) => None,
      FramePayload::SubTypeBytes(id, bytes) => Some((id, bytes)),
    }
  }

  pub fn bytes(&self) -> &[u8] {
    match *self {
      FramePayload::Bytes(ref bytes) => bytes.as_ref(),
      FramePayload::SubTypeBytes(_, ref bytes) => bytes.as_ref(),
    }
  }

  pub fn into_bytes(self) -> Bytes {
    match self {
      FramePayload::Bytes(bytes) => bytes,
      FramePayload::SubTypeBytes(_, bytes) => bytes,
    }
  }
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

    match self.payload {
      FramePayload::Bytes(bytes) => T::decode(bytes).map_err(Into::into),
      FramePayload::SubTypeBytes(sub_type, bytes) => {
        let head = &[sub_type] as &[_];
        T::decode(head.chain(bytes)).map_err(Into::into)
      }
    }
  }

  pub fn encode(&self, dst: &mut BytesMut) {
    use flo_util::binary::{BinDecode, BinEncode};
    dst.reserve(Header::MIN_SIZE + self.payload.len());
    self.type_id.encode(dst);
    (self.payload.len() as u16).encode(dst);
    match self.payload {
      FramePayload::Bytes(ref bytes) => {
        dst.put(bytes.as_ref());
      }
      FramePayload::SubTypeBytes(subtype, ref bytes) => {
        dst.put_u8(subtype);
        dst.put(bytes.as_ref());
      }
    }
  }
}

/// Decodes packet by type id
/// If no branch matches, returns Err(...)
///
/// ```no_run
/// flo_net::try_flo_packet! {
///   frame => {
///     p = PacketClientDisconnect => {
///       LobbyEvent::Disconnect(S2ProtoUnpack::unpack(p.reason)?)
///     },
///     p = PacketGameInfo => {
///       LobbyEvent::GameInfo(p.game.extract()?)
///     },
///   }
/// };
/// ```
#[macro_export]
macro_rules! try_flo_packet {
  (
    $frame:expr => {
      $(
        $binding:ident: $packet_type:ty => $block:block
      )*
    }
  ) => {
    match $frame.type_id {
      $(
        <$packet_type as $crate::packet::FloPacket>::TYPE_ID => {
          let $binding = <$packet_type as $crate::packet::Message>::decode(
            match $frame.payload {
              $crate::packet::FramePayload::Bytes(bytes) => bytes,
              $crate::packet::FramePayload::SubTypeBytes(_, bytes) => bytes,
            }
          ).map_err($crate::error::Error::from)?;
          $block
        }
      ),*
      other => {
        return Err($crate::error::Error::unexpected_packet_type_id(other).into())
      },
    }
  };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum PacketTypeId {
  // Common
  #[bin(value = 0x01)]
  Ping,
  #[bin(value = 0x02)]
  Pong,

  // Client <-> Lobby
  #[bin(value = 0x03)]
  ConnectController,
  #[bin(value = 0x04)]
  ConnectControllerAccept,
  #[bin(value = 0x05)]
  ConnectControllerReject,
  #[bin(value = 0x06)]
  LobbyDisconnect,
  #[bin(value = 0x07)]
  GameInfo,
  #[bin(value = 0x08)]
  GamePlayerEnter,
  #[bin(value = 0x09)]
  GamePlayerLeave,
  #[bin(value = 0x0A)]
  GameSlotUpdate,
  #[bin(value = 0x0B)]
  GameSlotUpdateRequest,
  #[bin(value = 0x0C)]
  PlayerSessionUpdate,
  #[bin(value = 0x0D)]
  ListNodesRequest,
  #[bin(value = 0x0E)]
  ListNodes,
  #[bin(value = 0x0F)]
  GameSelectNodeRequest,
  #[bin(value = 0x10)]
  GameSelectNode,
  #[bin(value = 0x11)]
  PlayerPingMapUpdateRequest,
  #[bin(value = 0x12)]
  PlayerPingMapUpdate,
  #[bin(value = 0x13)]
  GamePlayerPingMapSnapshotRequest,
  #[bin(value = 0x14)]
  GamePlayerPingMapSnapshot,
  #[bin(value = 0x15)]
  GamePlayerToken,
  #[bin(value = 0x16)]
  GameStartRequest,
  #[bin(value = 0x17)]
  GameStarting,
  #[bin(value = 0x18)]
  GameStartReject,
  #[bin(value = 0x19)]
  GameStartPlayerClientInfoRequest,
  #[bin(value = 0x1A)]
  GameSlotClientStatusUpdate,
  #[bin(value = 0x1B)]
  AddNode,
  #[bin(value = 0x1C)]
  RemoveNode,
  #[bin(value = 0x1D)]
  PlayerMuteListUpdate,
  #[bin(value = 0x1E)]
  PlayerMuteAddRequest,
  #[bin(value = 0x1F)]
  PlayerMuteRemoveRequest,

  // Lobby <-> Node
  #[bin(value = 0x30)]
  ControllerConnect,
  #[bin(value = 0x31)]
  ControllerConnectAccept,
  #[bin(value = 0x32)]
  ControllerConnectReject,
  #[bin(value = 0x33)]
  ControllerCreateGame,
  #[bin(value = 0x34)]
  ControllerCreateGameAccept,
  #[bin(value = 0x35)]
  ControllerCreateGameReject,
  #[bin(value = 0x36)]
  ControllerUpdateSlotStatus,
  #[bin(value = 0x37)]
  ControllerUpdateSlotStatusAccept,
  #[bin(value = 0x38)]
  ControllerUpdateSlotStatusReject,
  #[bin(value = 0x39)]
  ControllerQueryGameStatus,

  // Client <-> Node
  #[bin(value = 0x40)]
  ClientConnect,
  #[bin(value = 0x41)]
  ClientConnectAccept,
  #[bin(value = 0x42)]
  ClientConnectReject,
  #[bin(value = 0x43)]
  ClientUpdateSlotClientStatusRequest,
  #[bin(value = 0x44)]
  ClientUpdateSlotClientStatus,
  #[bin(value = 0x45)]
  ClientUpdateSlotClientStatusReject,

  // Node -> [Client, Controller]
  #[bin(value = 0x50)]
  NodeGameStatusUpdate,
  #[bin(value = 0x51)]
  NodeGameStatusUpdateBulk,

  // Controller <-> Observer
  #[bin(value = 0x60)]
  ObserverConnect,
  #[bin(value = 0x61)]
  ObserverConnectAccept,
  #[bin(value = 0x62)]
  ObserverConnectReject,
  #[bin(value = 0x63)]
  ObserverData,

  #[bin(value = 0xF7)]
  W3GS,
  UnknownValue(u8),
}

impl PacketTypeId {
  pub fn has_subtype(&self) -> bool {
    match *self {
      PacketTypeId::W3GS => true,
      _ => false,
    }
  }
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

pub trait OptionalFieldExt<T> {
  fn extract(self) -> Result<T>;
}

impl<T> OptionalFieldExt<T> for Option<T> {
  fn extract(self) -> Result<T, Error> {
    self.ok_or_else(|| Error::PacketFieldNotPresent)
  }
}

pub trait TimeoutResultExt<T, E> {
  fn flatten_timeout_err<F>(self, f: F) -> Result<T, E>
  where
    F: FnOnce() -> E;

  fn map_timeout_err<F1, F2, E2>(self, map_timeout: F1, map_err: F2) -> Result<T, E2>
  where
    F1: FnOnce() -> E2,
    F2: FnOnce(E) -> E2;
}

impl<T, E> TimeoutResultExt<T, E> for Result<Result<T, E>, Elapsed> {
  fn flatten_timeout_err<F>(self, f: F) -> Result<T, E>
  where
    F: FnOnce() -> E,
  {
    self.map_err(|_| f()).and_then(|res| res)
  }

  fn map_timeout_err<F1, F2, E2>(self, map_timeout: F1, map_err: F2) -> Result<T, E2>
  where
    F1: FnOnce() -> E2,
    F2: FnOnce(E) -> E2,
  {
    self
      .map_err(|_| map_timeout())
      .and_then(|res| res.map_err(map_err))
  }
}
