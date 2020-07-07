use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::protocol::constants::PacketTypeId;
use crate::protocol::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct Desync {
  pub unknown_1: u32,
  #[bin(eq = 4)]
  pub unknown_2: u8,
  pub unknown_3: u32,
  #[bin(eq = 0)]
  pub unknown_4: u8,
}

impl PacketPayload for Desync {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::Desync;
}
