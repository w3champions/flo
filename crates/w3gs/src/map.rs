use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode)]
pub struct MapCheck {
  #[bin(eq = 0x01)]
  _unknown_1: u32,
  pub file_path: CString,
  pub file_size: u32,
  pub file_crc: u32,
  pub map_xoro: u32,
  pub sha1: [u8; 20],
}

impl PacketPayload for MapCheck {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::MapCheck;
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct MapSize {
  #[bin(eq = 0x01)]
  _unknown_1: u32,
  pub size_flag: u8,
  pub map_size: u32,
}

impl PacketPayload for MapSize {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::MapSize;
}

#[test]
fn test_map_check() {
  crate::packet::test_payload_type::<MapCheck>("map_check.bin")
}

#[test]
fn test_map_size() {
  crate::packet::test_payload_type::<MapSize>("map_size.bin")
}
