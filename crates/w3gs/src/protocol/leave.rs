


use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::{LeaveReason, PacketTypeId};
use crate::protocol::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct LeaveReq(LeaveReason);

impl LeaveReq {
  pub fn new(reason: LeaveReason) -> Self {
    Self(reason)
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
