use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::error::Result;
use crate::protocol::constants::{PacketTypeId, ProtoBufMessageTypeId};
use crate::protocol::join::ReqJoin;
use crate::protocol::packet::{PacketPayload, PacketProtoBufMessage};
pub use crate::protocol::protobuf::{
  PlayerProfileMessage, PlayerProfileRealm, PlayerSkin, PlayerSkinsMessage, PlayerUnknown5Message,
};

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PlayerInfo {
  pub join_counter: u32,
  pub player_id: u8,
  pub player_name: CString,
  pub _num_unknown_1: u8,
  #[bin(repeat = "_num_unknown_1")]
  pub _unknown_1: Vec<u8>,
  pub external_addr: SockAddr,
  pub internal_addr: SockAddr,
}

impl PlayerInfo {
  pub fn new(player_id: u8, player_name: impl IntoCStringLossy) -> Self {
    PlayerInfo {
      join_counter: 1,
      player_id,
      player_name: player_name.into_c_string_lossy(),
      _num_unknown_1: 2,
      _unknown_1: vec![0, 0],
      external_addr: SockAddr::new_null(),
      internal_addr: SockAddr::new_null(),
    }
  }

  pub fn from_req_join(player_id: u8, req: ReqJoin) -> Self {
    Self::new(player_id, req.player_name)
  }
}

impl PacketPayload for PlayerInfo {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PlayerInfo;
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
pub struct PlayerLoaded {
  pub player_id: u8,
}

impl PlayerLoaded {
  pub fn new(player_id: u8) -> Self {
    Self { player_id }
  }
}

impl PacketPayload for PlayerLoaded {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::PlayerLoaded;
}

impl PlayerProfileMessage {
  pub fn new(player_id: u8, battle_tag: &str) -> Self {
    Self {
      player_id: player_id as u32,
      battle_tag: battle_tag.to_string(),
      portrait: "p042".to_owned(),
      ..Default::default()
    }
  }
}

impl PacketProtoBufMessage for PlayerProfileMessage {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerProfile;
}

impl PlayerSkinsMessage {
  pub fn new(player_id: u8) -> Self {
    Self {
      player_id: player_id as u32,
      ..Default::default()
    }
  }
}

impl PacketProtoBufMessage for PlayerSkinsMessage {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerSkins;
}

impl PacketProtoBufMessage for PlayerUnknown5Message {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId = ProtoBufMessageTypeId::PlayerUnknown5;
}

#[test]
fn test_player_info() {
  crate::packet::test_simple_payload_type(
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
  use crate::protocol::packet::{Packet, ProtoBufPayload};
  let mut buf =
    BytesMut::from(flo_util::sample_bytes!("packet", "protobuf_0x59_0x02.bin").as_slice());
  let h = Packet::decode_header(&mut buf).unwrap();
  let p = Packet::decode(h, &mut buf).unwrap();
  let p: ProtoBufPayload = p.decode_simple_payload().unwrap();
  dbg!(&p);
}
