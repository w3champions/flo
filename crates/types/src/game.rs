use crate::node::*;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::ClientDisconnectReason")]
pub enum DisconnectReason {
  Unknown = 0,
  Multi = 1,
  Maintenance = 2,
}

#[derive(Debug, S2ProtoUnpack, Serialize, Clone)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::Session")]
pub struct PlayerSession {
  pub player: PlayerInfo,
  pub status: PlayerStatus,
  pub game_id: Option<i32>,
}

#[derive(Debug, S2ProtoUnpack, Serialize, Clone)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::PacketPlayerSessionUpdate")]
pub struct PlayerSessionUpdate {
  pub status: PlayerStatus,
  pub game_id: Option<i32>,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerStatus")]
pub enum PlayerStatus {
  Idle = 0,
  InGame = 1,
}

#[derive(Debug, S2ProtoUnpack, Serialize, Deserialize, Clone)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::PlayerInfo")]
pub struct PlayerInfo {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerSource")]
pub enum PlayerSource {
  Test = 0,
  BNet = 1,
  Api = 2,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::ClientConnectRejectReason")]
pub enum RejectReason {
  Unknown = 0,
  ClientVersionTooOld = 1,
  InvalidToken = 2,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::GameInfo")]
pub struct GameInfo {
  pub id: i32,
  pub name: String,
  pub status: GameStatus,
  pub map: Map,
  pub slots: Vec<Slot>,
  pub node: Option<Node>,
  pub is_private: bool,
  pub is_live: bool,
  pub random_seed: i32,
  pub created_by: Option<PlayerInfo>,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::GameStatus")]
pub enum GameStatus {
  Preparing = 0,
  Created = 1,
  Running = 2,
  Ended = 3,
  Paused = 4,
  Terminated = 5,
}

#[derive(Debug, S2ProtoUnpack, Serialize, Clone)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::Node")]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub ip_addr: String,
  pub country_id: String,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::Map")]
pub struct Map {
  pub sha1: Vec<u8>,
  pub checksum: u32,
  pub path: String,
}

#[derive(Debug, S2ProtoUnpack, Serialize, Clone)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::Slot))]
pub struct Slot {
  pub player: Option<PlayerInfo>,
  pub settings: SlotSettings,
  #[s2_grpc(proto_enum)]
  pub client_status: SlotClientStatus,
}

impl Default for Slot {
  fn default() -> Self {
    Self {
      player: None,
      settings: SlotSettings::default(),
      client_status: SlotClientStatus::Pending,
    }
  }
}

pub struct LanGameSlot<'a> {
  pub player: Option<LanGamePlayerInfo<'a>>,
  pub settings: SlotSettings,
}

pub struct LanGamePlayerInfo<'a> {
  pub id: i32,
  pub name: &'a str,
}

impl<'a> From<&'a Slot> for LanGameSlot<'a> {
  fn from(slot: &'a Slot) -> Self {
    Self {
      player: slot.player.as_ref().map(|p| LanGamePlayerInfo {
        id: p.id,
        name: p.name.as_str()
      }),
      settings: slot.settings.clone(),
    }
  }
}

#[derive(Debug, S2ProtoUnpack, S2ProtoPack, Serialize, Deserialize, Clone)]
#[s2_grpc(message_type(
  flo_net::proto::flo_connect::SlotSettings,
  flo_grpc::game::SlotSettings,
))]
pub struct SlotSettings {
  pub team: i32,
  pub color: i32,
  #[s2_grpc(proto_enum)]
  pub computer: Computer,
  pub handicap: i32,
  #[s2_grpc(proto_enum)]
  pub status: SlotStatus,
  #[s2_grpc(proto_enum)]
  pub race: Race,
}

impl Default for SlotSettings {
  fn default() -> Self {
    SlotSettings {
      team: 0,
      color: 0,
      computer: Computer::Easy,
      handicap: 100,
      status: SlotStatus::Open,
      race: Race::Human,
    }
  }
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[s2_grpc(proto_enum_type(
  flo_net::proto::flo_connect::Computer,
  flo_grpc::game::Computer
))]
pub enum Computer {
  Easy = 0,
  Normal = 1,
  Insane = 2,
}

impl From<Computer> for flo_w3gs::slot::AI {
  fn from(race: Computer) -> Self {
    use flo_w3gs::slot::AI;
    match race {
      Computer::Easy => AI::ComputerEasy,
      Computer::Normal => AI::ComputerNormal,
      Computer::Insane => AI::ComputerInsane,
    }
  }
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[s2_grpc(proto_enum_type(
  flo_net::proto::flo_connect::Race,
  flo_grpc::game::Race,
))]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

impl From<Race> for flo_w3gs::slot::RacePref {
  fn from(race: Race) -> Self {
    use flo_w3gs::slot::RacePref;
    match race {
      Race::Human => RacePref::HUMAN,
      Race::Orc => RacePref::ORC,
      Race::NightElf => RacePref::NIGHTELF,
      Race::Undead => RacePref::UNDEAD,
      Race::Random => RacePref::RANDOM,
    }
  }
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[s2_grpc(proto_enum_type(
  flo_net::proto::flo_connect::SlotStatus,
  flo_grpc::game::SlotStatus,
))]
pub enum SlotStatus {
  Open = 0,
  Closed = 1,
  Occupied = 2,
}

#[derive(Debug, Serialize, Clone)]
pub struct GameStatusUpdate {
  pub game_id: i32,
  pub status: NodeGameStatus,
  pub updated_player_game_client_status_map: HashMap<i32, SlotClientStatus>,
}

impl From<flo_net::proto::flo_node::PacketNodeGameStatusUpdate> for GameStatusUpdate {
  fn from(pkt: flo_net::proto::flo_node::PacketNodeGameStatusUpdate) -> Self {
    GameStatusUpdate {
      game_id: pkt.game_id,
      status: NodeGameStatus::unpack_enum(pkt.status()),
      updated_player_game_client_status_map: pkt
        .updated_player_game_client_status_map
        .into_iter()
        .map(|(k, v)| {
          (
            k,
            flo_net::proto::flo_connect::SlotClientStatus::from_i32(v)
              .map(|v| SlotClientStatus::unpack_enum(v))
              .unwrap_or(SlotClientStatus::Pending),
          )
        })
        .collect(),
    }
  }
}

#[derive(Debug, Serialize)]
pub struct MapDetail {
  pub path: String,
  pub sha1: String,
  pub crc32: u32,
  pub name: String,
  pub author: String,
  pub description: String,
  pub width: u32,
  pub height: u32,
  pub preview_jpeg_base64: String,
  pub suggested_players: String,
  pub num_players: usize,
  pub players: Vec<MapPlayerOwned>,
  pub forces: Vec<MapForceOwned>,
}

#[derive(Debug, Serialize)]
pub struct MapPlayerOwned {
  pub name: String,
  pub r#type: u32,
  pub race: u32,
  pub flags: u32,
}

#[derive(Debug, Serialize)]
pub struct MapForceOwned {
  pub name: String,
  pub flags: u32,
  pub player_set: u32,
}
