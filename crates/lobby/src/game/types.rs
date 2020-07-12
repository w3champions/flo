use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};

use crate::map::Map;
use crate::node::NodeRef;
use crate::player::PlayerRef;

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
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
  pub created_by: PlayerRef,
  pub started_at: Option<DateTime<Utc>>,
  pub ended_at: Option<DateTime<Utc>>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
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

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::Slot")]
pub struct Slot {
  pub player: Option<PlayerRef>,
  pub player_id: Option<u32>,
  pub settings: SlotSettings,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type = "flo_grpc::game::SlotSettings")]
pub struct SlotSettings {
  pub team: u32,
  pub color: u32,
  pub computer: Computer,
  pub handicap: u32,
  pub status: SlotStatus,
  pub race: Race,
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
