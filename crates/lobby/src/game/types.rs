use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use flo_net::proto::flo_connect as packet;

use crate::map::Map;
use crate::node::NodeRef;
use crate::player::PlayerRef;

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type = "flo_grpc::game::Game")]
pub struct Game {
  pub id: i32,
  pub name: String,
  pub status: GameStatus,
  pub map: Map,
  pub slots: Vec<Slot>,
  pub node: Option<NodeRef>,
  pub is_private: bool,
  pub secret: Option<i32>,
  pub is_live: bool,
  pub num_players: i32,
  pub max_players: i32,
  pub created_by: Option<PlayerRef>,
  pub started_at: Option<DateTime<Utc>>,
  pub ended_at: Option<DateTime<Utc>>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Queryable)]
#[s2_grpc(message_type = "flo_grpc::game::GameEntry")]
pub struct GameEntry {
  pub id: i32,
  pub name: String,
  pub map_name: String,
  pub status: GameStatus,
  pub is_private: bool,
  pub is_live: bool,
  pub num_players: i32,
  pub max_players: i32,
  pub started_at: Option<DateTime<Utc>>,
  pub ended_at: Option<DateTime<Utc>>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub created_by: Option<PlayerRef>,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type = "flo_grpc::game::GameStatus")]
pub enum GameStatus {
  Preparing = 0,
  Playing = 1,
  Ended = 2,
  Paused = 4,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type = "flo_grpc::game::Slot")]
pub struct Slot {
  pub player: Option<PlayerRef>,
  pub settings: SlotSettings,
}

impl Default for Slot {
  fn default() -> Self {
    Slot {
      player: None,
      settings: Default::default(),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type = "flo_grpc::game::SlotSettings")]
pub struct SlotSettings {
  pub team: u32,
  pub color: u32,
  pub computer: Computer,
  pub handicap: u32,
  pub status: SlotStatus,
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

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type = "flo_grpc::game::SlotStatus")]
pub enum SlotStatus {
  Open = 0,
  Closed = 1,
  Occupied = 2,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type = "flo_grpc::game::Race")]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type = "flo_grpc::game::Computer")]
pub enum Computer {
  Easy = 0,
  Normal = 1,
  Insane = 2,
}

impl Game {
  pub fn into_packet(self) -> packet::GameInfo {
    packet::GameInfo {
      id: self.id,
      name: self.name,
      status: match self.status {
        GameStatus::Preparing => packet::GameStatus::Preparing,
        GameStatus::Playing => packet::GameStatus::Playing,
        GameStatus::Ended => packet::GameStatus::Ended,
        GameStatus::Paused => packet::GameStatus::Paused,
      }
      .into(),
      map: Some(packet::Map {
        sha1: self.map.sha1.to_vec(),
        checksum: self.map.checksum,
        path: self.map.path,
      }),
      slots: self
        .slots
        .into_iter()
        .map(|slot| packet::Slot {
          player: slot.player.map(|player| player.into_packet()),
          settings: slot.settings.into_packet().into(),
        })
        .collect(),
      node: self.node.map(|node| node.into_packet()),
      is_private: self.is_private,
      is_live: self.is_live,
      created_by: self.created_by.map(|player| player.into_packet()),
    }
  }
}

impl SlotSettings {
  pub fn into_packet(self) -> packet::SlotSettings {
    packet::SlotSettings {
      team: self.team,
      color: self.color,
      computer: match self.computer {
        Computer::Easy => packet::Computer::Easy,
        Computer::Normal => packet::Computer::Normal,
        Computer::Insane => packet::Computer::Insane,
      }
      .into(),
      handicap: self.handicap,
      status: match self.status {
        SlotStatus::Open => packet::SlotStatus::Open,
        SlotStatus::Closed => packet::SlotStatus::Closed,
        SlotStatus::Occupied => packet::SlotStatus::Occupied,
      }
      .into(),
      race: match self.race {
        Race::Human => packet::Race::Human,
        Race::Orc => packet::Race::Orc,
        Race::NightElf => packet::Race::NightElf,
        Race::Undead => packet::Race::Undead,
        Race::Random => packet::Race::Random,
      }
      .into(),
    }
  }
}
