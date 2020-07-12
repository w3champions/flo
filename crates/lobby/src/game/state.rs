use std::collections::HashMap;

use crate::connect::NotificationSender;
use crate::player::PlayerId;

#[derive(Debug)]
pub struct GameState {
  pub players: Vec<GameStatePlayer>,
}

#[derive(Debug)]
pub struct GameStateSnapshot {
  pub players: Vec<PlayerId>,
}

#[derive(Debug)]
pub struct GameStatePlayer {
  pub player: PlayerId,
  pub sender: NotificationSender,
}

pub struct Storage {
  map: HashMap<i32, GameStateHandle>,
}

#[derive(Debug, Clone)]
pub struct GameStateHandle;

#[derive(Debug, Clone)]
pub struct StorageHandle;
