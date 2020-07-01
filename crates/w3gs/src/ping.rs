use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PingFromHost(Ping);

impl PacketPayload for PingFromHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PingFromHost;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PongToHost(Ping);

impl PacketPayload for PongToHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PongToHost;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct Ping {
  pub payload: u32,
}

#[test]
fn test_ping_from_host() {
  crate::packet::test_payload_type(
    "ping_from_host.bin",
    &PingFromHost(Ping { payload: 95750587 }),
  )
}

#[test]
fn test_pong_to_host() {
  crate::packet::test_payload_type("pong_to_host.bin", &PongToHost(Ping { payload: 95750587 }))
}
