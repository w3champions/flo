use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode)]
pub struct PingFromHost(Ping);

impl PacketPayload for PingFromHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PingFromHost;
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct PongToHost(Ping);

impl PacketPayload for PongToHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PongToHost;
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct Ping {
  pub payload: u32,
}
