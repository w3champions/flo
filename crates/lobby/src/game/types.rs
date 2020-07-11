use bs_diesel_utils::BSDieselEnum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::map::Map;
use crate::node::Node;
use crate::player::PlayerRef;

#[derive(Debug, Serialize, Deserialize)]
pub struct Game {
  pub id: i32,
  pub name: String,
  pub status: GameStatus,
  pub map: Map,
  pub slots: Vec<Slot>,
  pub node: Option<Node>,
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

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, BSDieselEnum)]
#[repr(i32)]
pub enum GameStatus {
  Waiting = 0,
  Ongoing = 1,
  Ended = 2,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Slot {
  pub player: Option<PlayerRef>,
  pub player_id: Option<u32>,
  pub team: u32,
  pub color: u32,
  pub computer: Option<Computer>,
  pub handicap: u32,
  pub status: SlotStatus,
  pub race: Race,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
#[repr(i32)]
pub enum SlotStatus {
  Open = 0,
  Closed = 1,
  Occupied = 2,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
#[repr(i32)]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
#[repr(i32)]
pub enum Computer {
  Easy = 0,
  Normal = 1,
  Insane = 2,
}
