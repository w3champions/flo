use bs_diesel_utils::{Executor, ExecutorRef};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::connect::PlayerSenderRef;
use crate::error::Result;
use crate::game::{
  db::{get_all_active_game_state, GameStateFromDb},
  GameEntry,
};

mod config;
pub use config::ConfigClientStorageRef;
use config::ConfigStorage;

#[derive(Clone)]
pub struct LobbyStateRef {
  pub db: ExecutorRef,
  pub mem: MemStorageRef,
  pub config: ConfigClientStorageRef,
}

impl LobbyStateRef {
  pub async fn init() -> Result<Self> {
    let db = Executor::env().into_ref();
    let mem = MemStorage::init(db.clone()).await?.into_ref();
    let api_client = ConfigStorage::init(db.clone()).await?.into_ref();
    Ok(LobbyStateRef {
      db,
      mem,
      config: api_client,
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
  pub sender: Option<PlayerSenderRef>,
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
  player_senders: Arc<RwLock<HashMap<i32, PlayerSenderRef>>>,
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
      player_senders: Arc::new(RwLock::new(HashMap::new())),
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

  fn remove_game(&self, id: i32) {
    self.state.write().games.remove(&id);
    self.state.write().game_num_players.remove(&id);
  }

  pub async fn lock_player_state(&self, id: i32) -> LockedPlayerState {
    let (state, sender_map) = {
      let mut storage_lock = self.state.write();
      (
        storage_lock
          .players
          .entry(id)
          .or_insert_with(|| Arc::new(Mutex::new(MemPlayerState::default())))
          .clone(),
        storage_lock.player_senders.clone(),
      )
    };
    LockedPlayerState {
      id,
      guard: state.lock_owned().await,
      sender_map,
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
        Some(LockedGameState {
          id,
          guard,
          parent: self.clone(),
        })
      }
      None => None,
    }
  }

  pub fn get_player_sender(&self, id: i32) -> Option<PlayerSenderRef> {
    self.state.read().player_senders.read().get(&id).cloned()
  }

  pub fn get_player_senders(&self, ids: &[i32]) -> HashMap<i32, PlayerSenderRef> {
    if ids.is_empty() {
      return HashMap::default();
    }
    let map = self.state.read().player_senders.clone();
    let mut found = HashMap::with_capacity(ids.len());
    let guard = map.read();
    for id in ids {
      if let Some(sender) = guard.get(id).cloned() {
        found.insert(*id, sender);
      }
    }
    found
  }
}

#[derive(Debug)]
pub struct LockedPlayerState {
  id: i32,
  guard: OwnedMutexGuard<MemPlayerState>,
  sender_map: Arc<RwLock<HashMap<i32, PlayerSenderRef>>>,
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

  #[tracing::instrument(skip(self, sender))]
  pub fn replace_sender(&mut self, sender: PlayerSenderRef) {
    if let Some(mut sender) = self.guard.sender.take() {
      tracing::debug!("sender replaced");
      tokio::spawn(async move {
        sender.disconnect_multi().await;
      });
    }
    self.guard.sender = Some(sender.clone());
    self.sender_map.write().insert(self.id, sender);
  }

  pub fn remove_sender(&mut self, sender: PlayerSenderRef) {
    let remove = if let Some(ref current) = self.guard.sender {
      sender.ptr_eq(current)
    } else {
      false
    };
    if remove {
      self.guard.sender.take();
    }
    {
      let mut sender_map = self.sender_map.write();
      if sender_map
        .get(&self.id)
        .map(|s| s.ptr_eq(&sender))
        .unwrap_or_default()
      {
        sender_map.remove(&self.id);
      }
    }
  }

  pub fn get_sender_cloned(&self) -> Option<PlayerSenderRef> {
    self.guard.sender.clone()
  }

  pub fn get_sender_mut(&mut self) -> Option<&mut PlayerSenderRef> {
    self.guard.sender.as_mut()
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
    self.parent.remove_game(self.id)
  }
}
