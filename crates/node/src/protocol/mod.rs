use flo_util::binary::*;


mod codec;

pub const PORT: u16 = 0xDDF;
pub const SIGNATURE: u8 = 0xDF;

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum PacketTypeId {
  Ping,
  Pong,
  ControllerHandshakeRequest,
  ControllerHandshakeReply,
  PlayerHandshakeRequest,
  PlayerHandshakeReply,
}

pub struct ControllerHandshake {
  pub secret: Bytes,
}

pub struct PlayerHandshake {
  pub token: String,
}

pub struct PlayerSession {
  pub id: [u8; 16],
  pub player_id: i32,
}

// #[derive(Debug, Copy, Clone, PartialEq, Eq, BinDecode, BinEncode)]
// #[bin(enum_repr(u8))]
// pub enum HandshakeType {
//   #[bin(value = 0x0)]
//   Player,
//   #[bin(value = 0x1)]
//   Controller,
//   UnknownValue(u8),
// }
