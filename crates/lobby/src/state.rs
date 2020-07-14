use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::connect::NotificationSender;
use crate::error::Result;

#[derive(Debug)]
pub struct GameState {
  pub players: Vec<i32>,
  closed: bool,
}

impl GameState {
  fn new(players: &[i32]) -> Self {
    GameState {
      players: players.to_vec(),
      closed: false,
    }
  }
}

#[derive(Debug, Default)]
pub struct PlayerState {
  pub game_id: Option<i32>,
  pub sender: Option<NotificationSender>,
}

#[derive(Debug)]
pub struct Storage {
  state: Arc<RwLock<StorageState>>,
}

impl Storage {
  pub fn handle(&self) -> StorageHandle {
    StorageHandle(self.state.clone())
  }
}

#[derive(Debug)]
struct StorageState {
  players: HashMap<i32, Arc<Mutex<PlayerState>>>,
  games: HashMap<i32, Arc<Mutex<GameState>>>,
}

#[derive(Debug, Clone)]
pub struct StorageHandle(Arc<RwLock<StorageState>>);

impl StorageHandle {
  pub async fn register_game(&self, id: i32, players: &[i32]) {
    let mut storage_lock = self.0.write();
    if storage_lock.games.contains_key(&id) {
      tracing::warn!("override game state: id = {}", id);
    }
    storage_lock
      .games
      .insert(id, Arc::new(Mutex::new(GameState::new(players))));
  }

  pub async fn lock_player_state(&self, id: i32) -> LockedPlayerState {
    let state: Arc<Mutex<_>> = {
      let mut storage_lock = self.0.write();
      storage_lock
        .players
        .entry(id)
        .or_insert_with(|| Arc::new(Mutex::new(PlayerState::default())))
        .clone()
    };
    LockedPlayerState {
      id,
      guard: state.lock_owned().await,
    }
  }

  pub async fn lock_game_state(&self, id: i32) -> Option<LockedGameState> {
    let state = {
      let guard = self.0.read();
      guard.games.get(&id).cloned()
    };
    match state {
      Some(state) => {
        let guard = state.lock_owned().await;
        if guard.closed {
          let mut storage_guard = self.0.write();
          storage_guard.games.remove(&id);
          None
        } else {
          Some(LockedGameState { id, guard })
        }
      }
      None => None,
    }
  }
}

#[derive(Debug)]
pub struct LockedPlayerState {
  id: i32,
  guard: OwnedMutexGuard<PlayerState>,
}

impl LockedPlayerState {
  pub fn id(&self) -> i32 {
    self.id
  }

  pub fn joined_game_id(&self) -> Option<i32> {
    self.guard.game_id.clone()
  }

  pub fn join_game(&mut self, game_id: i32) {
    self.guard.game_id = Some(game_id)
  }

  pub fn leave_game(&mut self) {
    self.guard.game_id = None;
  }
}

#[derive(Debug)]
pub struct LockedGameState {
  id: i32,
  guard: OwnedMutexGuard<GameState>,
}

impl LockedGameState {
  pub fn id(&self) -> i32 {
    self.id
  }

  pub fn players(&self) -> &[i32] {
    &self.guard.players
  }

  pub fn has_player(&self, player_id: i32) -> bool {
    self.guard.players.contains(&player_id)
  }

  pub fn add_player(&mut self, player_id: i32) {
    if !self.guard.players.contains(&player_id) {
      self.guard.players.push(player_id)
    }
  }

  pub fn remove_player(&mut self, player_id: i32) {
    self.guard.players.retain(|id| *id != player_id)
  }

  pub fn close(&mut self) {
    self.guard.closed = true;
  }
}
