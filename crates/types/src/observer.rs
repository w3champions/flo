pub use crate::game::{Computer, Race, SlotSettings, SlotStatus};
use crate::game::{LanGamePlayerInfo, LanGameSlot};
use crate::game::LocalGameInfo;
//use flo_client::game::LocalGameInfo;
//use crate::game::LocalGameInfo;
use flo_grpc::game::Game;
use s2_grpc_utils::S2ProtoUnpack;
use serde::{Deserialize, Serialize};
#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type(flo_net::proto::flo_observer::GameInfo))]
pub struct GameInfo {
  pub id: i32,
  pub name: String,
  pub map: Map,
  pub slots: Vec<Slot>,
  pub random_seed: i32,
  pub game_version: String,
  pub start_time_millis: i64,
}

impl From<(&LocalGameInfo, String)> for GameInfo {
  fn from(lan_game: (&LocalGameInfo, String)) -> Self {
    Self {
      id: lan_game.0.game_id,
      name: lan_game.0.name.clone(),
      map: Map { sha1: lan_game.0.map_sha1.to_vec(), checksum: lan_game.0.map_checksum, path: lan_game.0.map_path.clone() },
      slots: lan_game.0.slots.iter().map(|lan_slot| { crate::observer::Slot::from(lan_slot.clone()) }).collect::<Vec<_>>(),
      random_seed: lan_game.0.random_seed,
      game_version: lan_game.1,
      start_time_millis: 0 //Trying this out, let's see if it works
    }
  }
}

impl S2ProtoUnpack<Game> for GameInfo {
  fn unpack(value: Game) -> Result<Self, s2_grpc_utils::result::Error> {
    Ok(GameInfo {
      id: value.id,
      name: value.name,
      map: Map::unpack(value.map)?,
      slots: S2ProtoUnpack::unpack(value.slots)?,
      random_seed: value.random_seed,
      game_version: value.game_version.unwrap_or_default(),
      start_time_millis: value
        .started_at
        .map(|v| v.seconds * 1000 + (v.nanos as i64 / 1000000))
        .unwrap_or_default(),
    })
  }
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type(flo_net::proto::flo_observer::Map, flo_grpc::game::Map))]
pub struct Map {
  pub sha1: Vec<u8>,
  pub checksum: u32,
  pub path: String,
}

impl Map {
  pub fn sha1(&self) -> Option<[u8; 20]> {
    if self.sha1.len() == 20 {
      let mut arr = [0; 20];
      arr.copy_from_slice(&self.sha1);
      Some(arr)
    } else {
      None
    }
  }
}

#[derive(Debug, S2ProtoUnpack, Serialize, Clone)]
#[s2_grpc(message_type(flo_net::proto::flo_observer::Slot, flo_grpc::game::Slot))]
pub struct Slot {
  pub player: Option<PlayerInfo>,
  pub settings: SlotSettings,
}

impl Default for Slot {
  fn default() -> Self {
    Self {
      player: None,
      settings: SlotSettings::default(),
    }
  }
}

#[derive(Debug, S2ProtoUnpack, Serialize, Deserialize, Clone)]
#[s2_grpc(message_type(flo_net::proto::flo_observer::PlayerInfo, flo_grpc::player::PlayerRef))]
pub struct PlayerInfo {
  pub id: i32,
  pub name: String,
}

impl<'a> From<&'a Slot> for LanGameSlot<'a> {
  fn from(slot: &'a Slot) -> Self {
    Self {
      player: slot.player.as_ref().map(|p| LanGamePlayerInfo {
        id: p.id,
        name: p.name.as_str(),
      }),
      settings: slot.settings.clone(),
    }
  }
}

impl From<crate::game::Slot> for Slot {
  fn from(slot: crate::game::Slot) -> Self {
    Self {
      player: slot.player.as_ref().map(|p | PlayerInfo { id: p.id, name: p.name.clone() }),
      settings: slot.settings
    }
  }
}
