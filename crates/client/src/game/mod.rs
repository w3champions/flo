use crate::error::{Error, Result};
use flo_types::game::{GameInfo, PlayerInfo, Slot, LocalGameInfo};
use std::collections::HashMap;

pub fn local_game_from_game_info(player_id: i32, game: &GameInfo) -> Result<LocalGameInfo> {
  Ok(LocalGameInfo { 
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