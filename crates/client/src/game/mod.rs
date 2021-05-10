use crate::error::{Error, Result};
use flo_types::game::{GameInfo, PlayerInfo, Slot};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LocalGameInfo {
  pub name: String,
  pub game_id: i32,
  pub random_seed: i32,
  pub node_id: Option<i32>,
  pub player_id: i32,
  pub map_path: String,
  pub map_sha1: [u8; 20],
  pub map_checksum: u32,
  pub players: HashMap<i32, PlayerInfo>,
  pub slots: Vec<Slot>,
  pub host_player: Option<PlayerInfo>,
}

impl LocalGameInfo {
  pub fn from_game_info(player_id: i32, game: &GameInfo) -> Result<Self> {
    Ok(Self {
      name: game.name.clone(),
      game_id: game.id,
      random_seed: game.random_seed,
      node_id: game.node.as_ref().map(|v| v.id).clone(),
      player_id,
      map_path: game.map.path.clone(),
      map_sha1: {
        if game.map.sha1.len() != 20 {
          return Err(Error::InvalidMapInfo);
        }
        let mut value = [0_u8; 20];
        value.copy_from_slice(&game.map.sha1[..]);
        value
      },
      map_checksum: game.map.checksum,
      players: game
        .slots
        .iter()
        .filter_map(|slot| slot.player.clone().map(|player| (player.id, player)))
        .collect(),
      slots: game.slots.clone(),
      host_player: game.created_by.clone(),
    })
  }
}
