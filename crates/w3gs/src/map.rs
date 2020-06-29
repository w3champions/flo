use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
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

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
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
  crate::packet::test_payload_type(
    "map_check.bin",
    &MapCheck {
      _unknown_1: 1,
      file_path: CString::new("Maps/(2)bootybay.w3m").unwrap(),
      file_size: 127172,
      file_crc: 1444344839,
      map_xoro: 2039165270,
      sha1: [
        201, 228, 110, 214, 86, 255, 142, 141, 140, 96, 141, 57, 3, 110, 63, 27, 250, 11, 28, 194,
      ],
    },
  )
}

#[test]
fn test_map_size() {
  crate::packet::test_payload_type(
    "map_size.bin",
    &MapSize {
      _unknown_1: 1,
      size_flag: 1,
      map_size: 127172,
    },
  )
}
