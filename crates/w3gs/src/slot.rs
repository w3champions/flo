use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::{PacketTypeId, SlotStatus, AI};
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode)]
pub struct SlotInfo {
  // 7 + sizeof(SlotData) x num_slots
  _length_of_slot_data: u16,
  _num_slots: u8,
  #[bin(repeat = "_num_slots")]
  pub slots: Vec<SlotData>,
  pub random_seed: u32,
  pub slot_layout: u8,
  pub num_players: u8,
}

impl PacketPayload for SlotInfo {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::SlotInfo;
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct SlotData {
  pub player_id: u8,
  pub download_status: u8,
  pub slot_status: SlotStatus,
  pub computer: bool,
  pub team: u8,
  pub color: u8,
  pub race: u8,
  pub computer_type: AI,
  pub handicap: u8,
}

#[test]
fn test_slot_info() {
  crate::packet::test_payload_type::<SlotInfo>("slot_info_1.bin");
  crate::packet::test_payload_type::<SlotInfo>("slot_info_2.bin");
  crate::packet::test_payload_type::<SlotInfo>("slot_info_3.bin");
}
