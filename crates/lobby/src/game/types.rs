use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use flo_net::proto::flo_connect as packet;
pub use flo_net::proto::flo_node::GameClientStatus;

use crate::map::Map;
use crate::node::NodeRef;
use crate::player::PlayerRef;

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type = "flo_grpc::game::Game")]
pub struct Game {
  pub id: i32,
  pub name: String,
  #[s2_grpc(proto_enum)]
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

impl Game {
  pub fn get_player_ids(&self) -> Vec<i32> {
    self
      .slots
      .iter()
      .filter_map(|slot| slot.player.as_ref().map(|p| p.id))
      .collect()
  }

  pub fn find_player_slot_index(&self, player_id: i32) -> Option<usize> {
    self.slots.iter().position(|slot| {
      slot
        .player
        .as_ref()
        .map(|p| p.id == player_id)
        .unwrap_or_default()
    })
  }

  pub fn get_player_slot_info(&self, player_id: i32) -> Option<PlayerSlotInfo> {
    let slot_index = self.find_player_slot_index(player_id)?;

    let slot = &self.slots[slot_index];

    Some(PlayerSlotInfo {
      slot_index,
      slot,
      player: slot.player.as_ref().expect("player slot at index"),
    })
  }
}

#[derive(Debug)]
pub struct PlayerSlotInfo<'a> {
  pub slot_index: usize,
  pub slot: &'a Slot,
  pub player: &'a PlayerRef,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Queryable)]
#[s2_grpc(message_type = "flo_grpc::game::GameEntry")]
pub struct GameEntry {
  pub id: i32,
  pub name: String,
  pub map_name: String,
  #[s2_grpc(proto_enum)]
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
#[s2_grpc(proto_enum_type(flo_grpc::game::GameStatus, flo_net::proto::flo_connect::GameStatus))]
pub enum GameStatus {
  Preparing = 0,
  Created = 1,
  Running = 2,
  Ended = 3,
  Paused = 4,
  Terminated = 5,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type(flo_grpc::game::Slot, flo_net::proto::flo_connect::Slot))]
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
#[s2_grpc(message_type(
  flo_grpc::game::SlotSettings,
  flo_net::proto::flo_connect::SlotSettings
))]
pub struct SlotSettings {
  pub team: u32,
  pub color: u32,
  #[s2_grpc(proto_enum)]
  pub computer: Computer,
  pub handicap: u32,
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

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type(flo_grpc::game::SlotStatus, flo_net::proto::flo_connect::SlotStatus))]
pub enum SlotStatus {
  Open = 0,
  Closed = 1,
  Occupied = 2,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type(flo_grpc::game::Race, flo_net::proto::flo_connect::Race))]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, S2ProtoEnum)]
#[repr(i32)]
#[s2_grpc(proto_enum_type(flo_grpc::game::Computer, flo_net::proto::flo_connect::Computer))]
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
        GameStatus::Created => packet::GameStatus::Created,
        GameStatus::Running => packet::GameStatus::Running,
        GameStatus::Ended => packet::GameStatus::Ended,
        GameStatus::Paused => packet::GameStatus::Paused,
        GameStatus::Terminated => packet::GameStatus::Terminated,
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
          player: slot.player.map(|player| player.pack().unwrap_or_default()),
          settings: slot.settings.into_packet().into(),
        })
        .collect(),
      node: self.node.map(|node| node.into_packet()),
      is_private: self.is_private,
      is_live: self.is_live,
      created_by: self
        .created_by
        .map(|player| player.pack().unwrap_or_default()),
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
