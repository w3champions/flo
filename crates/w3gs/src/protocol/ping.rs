use std::time::Instant;

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::PacketTypeId;
use crate::protocol::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PingFromHost(Ping);

impl PingFromHost {
  pub fn payload_since(since: Instant) -> Self {
    Self(Ping::payload_since(since))
  }
}

impl PacketPayload for PingFromHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PingFromHost;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PongToHost(Ping);

impl PongToHost {
  pub fn elapsed_millis(&self, since: Instant) -> Option<u32> {
    let d = Instant::now().checked_duration_since(since)?;
    self
      .0
      .payload
      .checked_sub((d.as_secs() * 1000) as u32 + d.subsec_millis())
  }
}

impl PacketPayload for PongToHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PongToHost;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct Ping {
  pub payload: u32,
}

impl Ping {
  pub fn payload_since(since: Instant) -> Ping {
    let d = Instant::now() - since;
    Self {
      payload: (d.as_secs() * 1000) as u32 + d.subsec_millis(),
    }
  }
}

#[test]
fn test_ping_from_host() {
  crate::packet::test_simple_payload_type(
    "ping_from_host.bin",
    &PingFromHost(Ping { payload: 95750587 }),
  )
}

#[test]
fn test_pong_to_host() {
  crate::packet::test_simple_payload_type(
    "pong_to_host.bin",
    &PongToHost(Ping { payload: 95750587 }),
  )
}
