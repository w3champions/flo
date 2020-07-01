use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::{PacketTypeId, ProtoBufMessageTypeId};
use crate::packet::{PacketPayload, PacketProtoBufMessage};
pub use crate::proto::{
  PlayerProfileMessage, PlayerProfileRealm, PlayerSkin, PlayerSkinsMessage, PlayerUnknown5Message,
};

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PlayerInfo {
  pub join_counter: u32,
  pub player_id: u8,
  pub player_name: CString,
  _num_unknown_1: u8,
  #[bin(repeat = "_num_unknown_1")]
  _unknown_1: Vec<u8>,
  pub external_addr: SockAddr,
  pub internal_addr: SockAddr,
}

impl PacketPayload for PlayerInfo {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PlayerInfo;
}

impl PacketProtoBufMessage for PlayerProfileMessage {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerProfile;
}

impl PacketProtoBufMessage for PlayerSkinsMessage {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerSkins;
}

impl PacketProtoBufMessage for PlayerUnknown5Message {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerUnknown5;
}

#[test]
fn test_player_info() {
  crate::packet::test_payload_type(
    "player_info.bin",
    &PlayerInfo {
      join_counter: 1,
      player_id: 1,
      player_name: CString::new("fluxxu#1815").unwrap(),
      _num_unknown_1: 2,
      _unknown_1: vec![0, 0],
      external_addr: SockAddr::new_null(),
      internal_addr: SockAddr::new_null(),
    },
  )
}

#[test]
fn test_player_profile() {
  crate::packet::test_protobuf_payload_type(
    "protobuf_0x59_0x03.bin",
    &PlayerProfileMessage {
      player_id: 2,
      battle_tag: "PLAYER".to_owned(),
      clan: "".to_owned(),
      portrait: "p042".to_owned(),
      realm: PlayerProfileRealm::Offline.into(),
      unknown_1: "".to_owned(),
    },
  );
}

#[test]
fn test_player_profile_2() {
  crate::packet::test_protobuf_payload_type(
    "protobuf_0x59_0x03_2.bin",
    &PlayerProfileMessage {
      player_id: 2,
      battle_tag: "PLAYER".to_owned(),
      clan: "".to_owned(),
      portrait: "p042".to_owned(),
      realm: PlayerProfileRealm::Offline.into(),
      unknown_1: "".to_owned(),
    },
  );
}

#[test]
fn test_player_skins() {
  crate::packet::test_protobuf_payload_type(
    "protobuf_0x59_0x04.bin",
    &PlayerSkinsMessage {
      player_id: 2,
      skins: vec![],
    },
  );
}

#[test]
fn test_player_skins_2() {
  crate::packet::test_protobuf_payload_type(
    "protobuf_0x59_0x04_2.bin",
    &PlayerSkinsMessage {
      player_id: 1,
      skins: vec![],
    },
  );
}

#[test]
fn test_player_unknown_02() {
  use crate::packet::{Packet, ProtoBufPayload};
  let mut buf =
    BytesMut::from(flo_util::sample_bytes!("packet", "protobuf_0x59_0x02.bin").as_slice());
  let h = Packet::decode_header(&mut buf).unwrap();
  let p = Packet::decode(h, &mut buf).unwrap();
  let p: ProtoBufPayload = p.decode_payload().unwrap();
  dbg!(&p);
}
