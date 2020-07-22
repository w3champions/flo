use bs_diesel_utils::{Executor, ExecutorRef};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::connect::NotificationSender;
use crate::error::Result;
use crate::game::{
  db::{get_all_active_game_state, GameStateFromDb},
  GameEntry,
};

mod api_client;
use api_client::ApiClientStorage;
pub use api_client::ApiClientStorageRef;

#[derive(Clone)]
pub struct LobbyStateRef {
  pub db: ExecutorRef,
  pub mem: MemStorageRef,
  pub api_client: ApiClientStorageRef,
}

impl LobbyStateRef {
  pub async fn init() -> Result<Self> {
    let db = Executor::env().into_ref();
    let mem = MemStorage::init(db.clone()).await?.into_ref();
    let api_client = ApiClientStorage::init(db.clone()).await?.into_ref();
    Ok(LobbyStateRef {
      db,
      mem,
      api_client,
    })
  }
}

#[derive(Debug)]
pub struct MemGameState {
  pub players: Vec<i32>,
  closed: bool,
}

impl MemGameState {
  fn new(players: &[i32]) -> Self {
    MemGameState {
      players: players.to_vec(),
      closed: false,
    }
  }
}

#[derive(Debug, Default)]
pub struct MemPlayerState {
  pub game_id: Option<i32>,
  pub sender: Option<NotificationSender>,
}

#[derive(Debug)]
pub struct MemStorage {
  state: RwLock<MemStorageState>,
}

#[derive(Debug, Clone)]
pub struct MemStorageRef(Arc<MemStorage>);

impl std::ops::Deref for MemStorageRef {
  type Target = MemStorage;

  fn deref(&self) -> &Self::Target {
    self.0.as_ref()
  }
}

impl MemStorage {
  pub async fn init(db: ExecutorRef) -> Result<Self> {
    let data = db.exec(|conn| get_all_active_game_state(conn)).await?;

    Ok(MemStorage {
      state: RwLock::new(MemStorageState::new(data)),
    })
  }

  pub fn into_ref(self) -> MemStorageRef {
    MemStorageRef(Arc::new(self))
  }
}

#[derive(Debug)]
struct MemStorageState {
  players: HashMap<i32, Arc<Mutex<MemPlayerState>>>,
  games: HashMap<i32, Arc<Mutex<MemGameState>>>,
  game_num_players: HashMap<i32, usize>,
}

impl MemStorageState {
  fn new(data: Vec<GameStateFromDb>) -> Self {
    let mut players = HashMap::new();
    let mut games = HashMap::new();
    let mut game_num_players = HashMap::new();

    for item in data {
      for player_id in &item.players {
        players.insert(
          *player_id,
          Arc::new(Mutex::new(MemPlayerState {
            game_id: Some(item.id),
            sender: None,
          })),
        );
      }

      game_num_players.insert(item.id, item.players.len());

      games.insert(
        item.id,
        Arc::new(Mutex::new(MemGameState {
          players: item.players,
          closed: false,
        })),
      );
    }

    Self {
      players,
      games,
      game_num_players,
    }
  }
}

impl MemStorageRef {
  pub async fn register_game(&self, id: i32, players: &[i32]) {
    let mut storage_lock = self.state.write();
    if storage_lock.games.contains_key(&id) {
      tracing::warn!("override game state: id = {}", id);
    }
    storage_lock.game_num_players.insert(id, players.len());
    storage_lock
      .games
      .insert(id, Arc::new(Mutex::new(MemGameState::new(players))));
  }

  pub async fn lock_player_state(&self, id: i32) -> LockedPlayerState {
    let state: Arc<Mutex<_>> = {
      let mut storage_lock = self.state.write();
      storage_lock
        .players
        .entry(id)
        .or_insert_with(|| Arc::new(Mutex::new(MemPlayerState::default())))
        .clone()
    };
    LockedPlayerState {
      id,
      guard: state.lock_owned().await,
    }
  }

  pub fn fetch_num_players(&self, games: &mut [GameEntry]) {
    for game in games {
      let state = self.state.read();
      if let Some(num) = state.game_num_players.get(&game.id).cloned() {
        game.num_players = num as i32;
      }
    }
  }

  pub async fn lock_game_state(&self, id: i32) -> Option<LockedGameState> {
    let state = {
      let guard = self.state.read();
      guard.games.get(&id).cloned()
    };
    match state {
      Some(state) => {
        let guard = state.lock_owned().await;
        if guard.closed {
          let mut storage_guard = self.state.write();
          storage_guard.games.remove(&id);
          None
        } else {
          Some(LockedGameState {
            id,
            guard,
            parent: self.clone(),
          })
        }
      }
      None => None,
    }
  }
}

#[derive(Debug)]
pub struct LockedPlayerState {
  id: i32,
  guard: OwnedMutexGuard<MemPlayerState>,
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
  guard: OwnedMutexGuard<MemGameState>,
  parent: MemStorageRef,
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
      self.guard.players.push(player_id);
      {
        let mut s = self.parent.state.write();
        s.game_num_players
          .entry(self.id)
          .and_modify(|v| *v = *v + 1);
      }
    }
  }

  pub fn remove_player(&mut self, player_id: i32) {
    self.guard.players.retain(|id| *id != player_id);
    {
      let mut s = self.parent.state.write();
      s.game_num_players
        .entry(self.id)
        .and_modify(|v| *v = *v - 1);
    }
  }

  pub fn close(&mut self) {
    self.guard.closed = true;
  }
}
