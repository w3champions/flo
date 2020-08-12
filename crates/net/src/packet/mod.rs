use bytes::{Buf, Bytes, BytesMut};
pub use prost::Message;

use flo_util::{BinDecode, BinEncode};

use crate::error::{Error, Result};
use tokio::time::Elapsed;

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

/// Decodes packet by type id
/// If no branch matches, returns Err(...)
///
/// ```no_run
/// let event = flo_net::match_packet! {
///   frame => {
///     p = PacketLobbyDisconnect => {
///       LobbyEvent::Disconnect(S2ProtoUnpack::unpack(p.reason)?)
///     },
///     p = PacketGameInfo => {
///       LobbyEvent::GameInfo(p.game.extract()?)
///     },
///   }
/// };
/// ```
#[macro_export]
macro_rules! match_packet {
  (
    $frame:expr => {
      $(
        $binding:ident = $packet_type:ty => $block:block
      )*
    }
  ) => {
    match $frame.type_id {
      $(
        <$packet_type as $crate::packet::FloPacket>::TYPE_ID => {
          let $binding = <$packet_type as $crate::packet::Message>::decode(
            $frame.payload
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(enum_repr(u8))]
pub enum PacketTypeId {
  // Common
  #[bin(value = 0x01)]
  Ping,
  #[bin(value = 0x02)]
  Pong,

  // Client <-> Lobby
  #[bin(value = 0x03)]
  ConnectLobby,
  #[bin(value = 0x04)]
  ConnectLobbyAccept,
  #[bin(value = 0x05)]
  ConnectLobbyReject,
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
  #[bin(value = 0x1A)]
  GamePlayerPingMapUpdateRequest,
  #[bin(value = 0x1B)]
  GamePlayerPingMapUpdate,
  #[bin(value = 0x1C)]
  GamePlayerPingMapSnapshotRequest,
  #[bin(value = 0x1D)]
  GamePlayerPingMapSnapshot,

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

  // Client <-> Node
  #[bin(value = 0x40)]
  ClientConnect,
  #[bin(value = 0x41)]
  ClientConnectAccept,
  #[bin(value = 0x42)]
  ClientConnectReject,
  #[bin(value = 0x43)]
  ClientPlayerStatusUpdateRequest,
  #[bin(value = 0x44)]
  ClientPlayerStatusUpdate,
  // Node -> [Client, Controller]
  #[bin(value = 0x45)]
  NodeGameStatusUpdate,

  // Controller <-> Observer
  #[bin(value = 0x50)]
  ObserverConnect,
  #[bin(value = 0x51)]
  ObserverConnectAccept,
  #[bin(value = 0x52)]
  ObserverConnectReject,
  #[bin(value = 0x53)]
  ObserverData,

  #[bin(value = 0xF7)]
  W3GS,
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
