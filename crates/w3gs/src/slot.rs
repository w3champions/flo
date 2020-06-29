use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::{PacketTypeId, RacePref, SlotStatus, AI};
use crate::packet::PacketPayload;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct SlotInfo {
  // 7 + sizeof(SlotData) x num_slots
  pub(crate) _length_of_slot_data: u16,
  pub(crate) _num_slots: u8,
  #[bin(repeat = "_num_slots")]
  pub slots: Vec<SlotData>,
  pub random_seed: u32,
  pub slot_layout: u8,
  pub num_players: u8,
}

impl PacketPayload for SlotInfo {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::SlotInfo;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct SlotData {
  pub player_id: u8,
  pub download_status: u8,
  pub slot_status: SlotStatus,
  pub computer: bool,
  pub team: u8,
  pub color: u8,
  #[bin(bitflags(u8))]
  pub race: RacePref,
  pub computer_type: AI,
  pub handicap: u8,
}

#[test]
fn test_slot_info() {
  crate::packet::test_payload_type(
    "slot_info_1.bin",
    &SlotInfo {
      _length_of_slot_data: 25,
      _num_slots: 2,
      slots: vec![
        SlotData {
          player_id: 1,
          download_status: 100,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 0,
          color: 0,
          race: RacePref::RANDOM | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
        SlotData {
          player_id: 2,
          download_status: 255,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 1,
          color: 1,
          race: RacePref::RANDOM | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
      ],
      random_seed: 22717299,
      slot_layout: 0,
      num_players: 2,
    },
  );
}

#[test]
fn test_slot_info_2() {
  crate::packet::test_payload_type(
    "slot_info_2.bin",
    &SlotInfo {
      _length_of_slot_data: 25,
      _num_slots: 2,
      slots: vec![
        SlotData {
          player_id: 1,
          download_status: 100,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 0,
          color: 0,
          race: RacePref::RANDOM | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
        SlotData {
          player_id: 2,
          download_status: 100,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 1,
          color: 1,
          race: RacePref::RANDOM | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
      ],
      random_seed: 22717310,
      slot_layout: 0,
      num_players: 2,
    },
  );
}

#[test]
fn test_slot_info_3() {
  crate::packet::test_payload_type(
    "slot_info_3.bin",
    &SlotInfo {
      _length_of_slot_data: 25,
      _num_slots: 2,
      slots: vec![
        SlotData {
          player_id: 1,
          download_status: 100,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 0,
          color: 0,
          race: RacePref::RANDOM | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
        SlotData {
          player_id: 2,
          download_status: 100,
          slot_status: SlotStatus::Occupied,
          computer: false,
          team: 1,
          color: 1,
          race: RacePref::HUMAN | RacePref::SELECTABLE,
          computer_type: AI::ComputerNormal,
          handicap: 100,
        },
      ],
      random_seed: 22741640,
      slot_layout: 0,
      num_players: 2,
    },
  );
}
