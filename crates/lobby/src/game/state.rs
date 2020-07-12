use std::collections::HashMap;

use crate::connect::NotificationSender;
use crate::game::GameId;

#[derive(Debug)]
pub struct GameState {
  pub players: Vec<GameStatePlayer>,
}

#[derive(Debug)]
pub struct GameStateSnapshot {
  pub players: Vec<i32>,
}

#[derive(Debug)]
pub struct GameStatePlayer {
  pub player: i32,
  pub sender: NotificationSender,
}

pub struct Storage {
  map: HashMap<i32, GameStateHandle>,
}

#[derive(Debug, Clone)]
pub struct GameStateHandle;

#[derive(Debug, Clone)]
pub struct StorageHandle;

impl StorageHandle {
  pub async fn register_game(&self, id: GameId) -> GameStateHandle {
    GameStateHandle
  }
}
