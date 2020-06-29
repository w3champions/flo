use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;
use crate::slot::SlotInfo;

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct ReqJoin {
  pub host_counter: u32,
  pub entry_key: u32,
  #[bin(eq = 0)]
  _unknown_1: u8,
  pub listen_port: u16,
  pub join_counter: u32,
  pub player_name: CString,
  _num_unknown_2: u8,
  #[bin(repeat = "_num_unknown_2")]
  _unknown_2: Vec<u8>,
  pub internal_addr: SockAddr,
}

impl PacketPayload for ReqJoin {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::ReqJoin;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct SlotInfoJoin {
  pub slot_info: SlotInfo,
  pub player_id: u8,
  pub external_addr: SockAddr,
}

impl PacketPayload for SlotInfoJoin {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::SlotInfoJoin;
}

#[test]
fn test_req_join() {
  crate::packet::test_payload_type(
    "req_join.bin",
    &ReqJoin {
      host_counter: 1,
      entry_key: 45397916,
      _unknown_1: 0,
      listen_port: 16001,
      join_counter: 1,
      player_name: CString::new("PLAYER").unwrap(),
      _num_unknown_2: 2,
      _unknown_2: vec![0, 0],
      internal_addr: SockAddr::new_ipv4([62, 128, 192, 168], 1537),
    },
  )
}

#[test]
fn test_slot_info_join() {
  use crate::constants::{RacePref, SlotStatus, AI};
  use crate::slot::SlotData;
  crate::packet::test_payload_type(
    "slot_info_join.bin",
    &SlotInfoJoin {
      slot_info: SlotInfo {
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
            player_id: 0,
            download_status: 255,
            slot_status: SlotStatus::Open,
            computer: false,
            team: 1,
            color: 24,
            race: RacePref::RANDOM | RacePref::SELECTABLE,
            computer_type: AI::ComputerNormal,
            handicap: 100,
          },
        ],
        random_seed: 22699111,
        slot_layout: 0,
        num_players: 2,
      },
      player_id: 2,
      external_addr: SockAddr::new_ipv4([211, 28, 192, 168], 1537),
    },
  )
}
