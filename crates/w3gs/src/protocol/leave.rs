use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::PacketTypeId;
use crate::protocol::packet::PacketPayload;

pub use crate::protocol::constants::LeaveReason;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct LeaveReq(LeaveReason);

impl LeaveReq {
  pub fn new(reason: LeaveReason) -> Self {
    Self(reason)
  }

  pub fn reason(&self) -> LeaveReason {
    self.0
  }
}

impl PacketPayload for LeaveReq {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::LeaveReq;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct LeaveAck;

impl PacketPayload for LeaveAck {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::LeaveAck;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PlayerLeft {
  pub player_id: u8,
  pub reason: LeaveReason,
}

impl PacketPayload for PlayerLeft {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PlayerLeft;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PlayerKicked {
  pub reason: LeaveReason,
}

impl PacketPayload for PlayerKicked {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PlayerKicked;
}
