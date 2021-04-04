use std::time::Instant;

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::PacketTypeId;
use crate::protocol::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PingFromHost(Ping);

impl PingFromHost {
  pub fn with_payload(payload: u32) -> Self {
    Self(Ping { payload })
  }

  pub fn with_payload_since(since: Instant) -> Self {
    Self(Ping::payload_since(since))
  }
}

impl PacketPayload for PingFromHost {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PingFromHost;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PongToHost(Ping);

impl PongToHost {
  pub fn payload(&self) -> u32 {
    self.0.payload
  }

  pub fn elapsed_millis(&self, since: Instant) -> u32 {
    let d = Instant::now().saturating_duration_since(since);
    (d.as_millis() as u32).saturating_sub(self.0.payload)
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
    let d = Instant::now().saturating_duration_since(since);
    Self {
      payload: d.as_millis() as u32,
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
