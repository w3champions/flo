use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::PacketTypeId;
use crate::packet::PacketPayload;
use crate::slot::SlotInfo;

#[derive(Debug, BinDecode, BinEncode)]
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

#[derive(Debug, BinDecode, BinEncode)]
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
  crate::packet::test_payload_type::<ReqJoin>("req_join.bin")
}

#[test]
fn test_slot_info_join() {
  crate::packet::test_payload_type::<SlotInfoJoin>("slot_info_join.bin")
}
