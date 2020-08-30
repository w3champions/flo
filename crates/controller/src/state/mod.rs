mod config;
pub mod event;

use bs_diesel_utils::{Executor, ExecutorRef};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::channel;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::connect::{PlayerBroadcaster, PlayerSender};
use crate::error::*;
use crate::game::{
  db::{get_all_active_game_state, GameStateFromDb},
  start::StartGameState,
  GameEntry, GameStatus, SlotClientStatus,
};
use crate::node::{conn::CreatedGameInfo, NodeRegistry, NodeRegistryRef};

pub use config::ConfigClientStorageRef;
use config::ConfigStorage;
use event::{spawn_event_handler, FloControllerEventSender, FloEventContext};

#[derive(Clone)]
pub struct ControllerStateRef {
  event_sender: FloControllerEventSender,
  pub db: ExecutorRef,
  pub mem: MemStorageRef,
  pub config: ConfigClientStorageRef,
  pub nodes: NodeRegistryRef,
}

impl ControllerStateRef {
  pub async fn init() -> Result<Self> {
    let (event_sender, event_receiver) = channel(16);

    let db = Executor::env().into_ref();

    let (mem, config, nodes) = tokio::try_join!(
      MemStorage::init(db.clone(), event_sender.clone()),
      ConfigStorage::init(db.clone()),
      NodeRegistry::init(db.clone(), event_sender.clone())
    )?;

    let mem = mem.into_ref();
    let nodes = nodes.into_ref();

    spawn_event_handler(
      FloEventContext {
        db: db.clone(),
        mem: mem.clone(),
        nodes: nodes.clone(),
      },
      event_receiver,
    );

    Ok(ControllerStateRef {
      event_sender,
      db,
      mem,
      config: config.into_ref(),
      nodes,
    })
  }
}

#[derive(Debug)]
pub struct MemGameState {
  pub status: GameStatus,
  pub host_player: Option<i32>,
  pub players: Vec<i32>,
  pub selected_node_id: Option<i32>,
  pub start_state: Option<StartGameState>,
  pub player_tokens: Option<HashMap<i32, [u8; 16]>>,
}

impl MemGameState {
  fn new(status: GameStatus, host_player: Option<i32>, players: &[i32]) -> Self {
    MemGameState {
      status,
      host_player,
      players: players.to_vec(),
      selected_node_id: None,
      start_state: None,
      player_tokens: None,
    }
  }
}

#[derive(Debug, Default)]
pub struct MemGamePlayerState {
  pub game_id: Option<i32>,
}

#[derive(Debug)]
pub struct MemStorage {
  event_sender: FloControllerEventSender,
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
  pub async fn init(db: ExecutorRef, event_sender: FloControllerEventSender) -> Result<Self> {
    let data = db.exec(|conn| get_all_active_game_state(conn)).await?;

    Ok(MemStorage {
      event_sender,
      state: RwLock::new(MemStorageState::new(data)),
    })
  }

  pub fn into_ref(self) -> MemStorageRef {
    MemStorageRef(Arc::new(self))
  }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct GamePlayerPingMapKey {
  pub node_id: i32,
  pub player_id: i32,
}
pub type GamePlayerPingMap = HashMap<GamePlayerPingMapKey, u32>;
pub type GamePlayerClientStatusMap = HashMap<i32, SlotClientStatus>;

#[derive(Debug)]
struct MemStorageState {
  games: HashMap<i32, Arc<Mutex<MemGameState>>>,
  game_player_ping_maps: Arc<RwLock<HashMap<i32, GamePlayerPingMap>>>,
  game_player_client_status_maps: Arc<RwLock<HashMap<i32, GamePlayerClientStatusMap>>>,
  game_players: HashMap<i32, Vec<i32>>,
  game_selected_node: HashMap<i32, i32>,
  players: HashMap<i32, Arc<Mutex<MemGamePlayerState>>>,
  player_senders: Arc<RwLock<HashMap<i32, PlayerSender>>>,
}

impl MemStorageState {
  fn new(data: Vec<GameStateFromDb>) -> Self {
    let mut players = HashMap::new();
    let mut games = HashMap::new();
    let mut game_players = HashMap::new();
    let mut game_selected_node = HashMap::new();

    for item in data {
      for player_id in &item.players {
        players.insert(
          *player_id,
          Arc::new(Mutex::new(MemGamePlayerState {
            game_id: Some(item.id),
          })),
        );
      }

      game_players.insert(item.id, item.players.clone());

      let selected_node_id = item.node_id;

      if let Some(node_id) = selected_node_id.clone() {
        game_selected_node.insert(item.id, node_id);
      }

      games.insert(
        item.id,
        Arc::new(Mutex::new(MemGameState {
          selected_node_id,
          ..MemGameState::new(item.status, item.created_by, &item.players)
        })),
      );
    }

    Self {
      players,
      player_senders: Arc::new(RwLock::new(HashMap::new())),
      games,
      game_players,
      game_player_ping_maps: Arc::new(RwLock::new(HashMap::new())),
      game_player_client_status_maps: Arc::new(RwLock::new(HashMap::new())),
      game_selected_node,
    }
  }
}

impl MemStorageRef {
  pub async fn register_game(
    &self,
    id: i32,
    status: GameStatus,
    host_player: Option<i32>,
    players: &[i32],
  ) {
    let mut storage_lock = self.state.write();
    if storage_lock.games.contains_key(&id) {
      tracing::warn!("override game state: id = {}", id);
    }
    storage_lock.game_players.insert(id, players.to_vec());
    storage_lock.games.insert(
      id,
      Arc::new(Mutex::new(MemGameState::new(status, host_player, players))),
    );
  }

  // Remove a game and it's players from memory
  fn remove_game(&self, id: i32, players_snapshot: &[i32]) {
    let mut guard = self.state.write();
    guard.games.remove(&id);
    guard.game_players.remove(&id);
    guard.game_player_ping_maps.write().remove(&id);
    guard.game_selected_node.remove(&id);
    guard.game_player_client_status_maps.write().remove(&id);
    for id in players_snapshot {
      guard.players.remove(id);
    }
  }

  pub async fn lock_player_state(&self, id: i32) -> LockedPlayerState {
    let (state, sender_map) = {
      let mut storage_lock = self.state.write();
      (
        storage_lock
          .players
          .entry(id)
          .or_insert_with(|| Arc::new(Mutex::new(MemGamePlayerState::default())))
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
      if let Some(num) = state.game_players.get(&game.id).map(|v| v.len()) {
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
          players_snapshot: guard.players.clone(),
          guard,
          parent: self.clone(),
        })
      }
      None => None,
    }
  }

  pub fn get_player_sender(&self, id: i32) -> Option<PlayerSender> {
    self.state.read().player_senders.read().get(&id).cloned()
  }

  pub fn get_broadcaster(&self, ids: &[i32]) -> PlayerBroadcaster {
    if ids.is_empty() {
      return PlayerBroadcaster::empty();
    }
    let map = self.state.read().player_senders.clone();
    let mut found = Vec::with_capacity(ids.len());
    let guard = map.read();
    for id in ids {
      if let Some(sender) = guard.get(id).cloned() {
        found.push(sender);
      }
    }
    PlayerBroadcaster::new(found)
  }

  pub fn get_game_player_ids(&self, game_id: i32) -> Vec<i32> {
    let guard = self.state.read();
    guard
      .game_players
      .get(&game_id)
      .cloned()
      .unwrap_or_default()
  }

  pub fn update_game_player_ping_map(
    &self,
    game_id: i32,
    player_id: i32,
    ping_map: HashMap<i32, u32>,
  ) {
    let map = self.state.read().game_player_ping_maps.clone();
    let mut guard = map.write();
    guard
      .entry(game_id)
      .and_modify(|map| {
        for (k, v) in &ping_map {
          map.insert(
            GamePlayerPingMapKey {
              node_id: *k,
              player_id,
            },
            *v,
          );
        }
      })
      .or_insert_with(|| {
        let mut map = GamePlayerPingMap::new();
        for (k, v) in &ping_map {
          map.insert(
            GamePlayerPingMapKey {
              node_id: *k,
              player_id,
            },
            *v,
          );
        }
        map
      });
  }

  pub fn get_game_player_ping_map(&self, game_id: i32) -> Option<GamePlayerPingMap> {
    let map = self.state.read().game_player_ping_maps.clone();
    let guard = map.read();
    guard.get(&game_id).cloned()
  }

  pub fn get_game_selected_node(&self, game_id: i32) -> Option<i32> {
    self.state.read().game_selected_node.get(&game_id).cloned()
  }

  pub fn update_game_player_client_status_maps(
    &self,
    maps: Vec<(i32, HashMap<i32, SlotClientStatus>)>,
  ) {
    if maps.is_empty() {
      return;
    }
    let state_maps = self.state.read().game_player_client_status_maps.clone();
    let mut guard = state_maps.write();
    for (game_id, map) in maps {
      use std::collections::hash_map::Entry;
      match guard.entry(game_id) {
        Entry::Vacant(entry) => {
          entry.insert(map);
        }
        Entry::Occupied(mut entry) => {
          entry.get_mut().extend(map);
        }
      }
    }
  }

  pub fn get_game_player_client_status_map(
    &self,
    game_id: i32,
  ) -> Option<GamePlayerClientStatusMap> {
    let map = self.state.read().game_player_client_status_maps.clone();
    let guard = map.read();
    guard.get(&game_id).cloned()
  }
}

#[derive(Debug)]
pub struct LockedPlayerState {
  id: i32,
  guard: OwnedMutexGuard<MemGamePlayerState>,
  sender_map: Arc<RwLock<HashMap<i32, PlayerSender>>>,
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

  pub fn leave_game(&mut self, game_id: i32) {
    if self.guard.game_id == Some(game_id) {
      self.guard.game_id = None;
    }
  }

  #[tracing::instrument(skip(self, sender))]
  pub async fn replace_sender(&mut self, sender: PlayerSender) {
    let replaced = { self.sender_map.write().insert(self.id, sender) };
    if let Some(mut sender) = replaced {
      tracing::debug!("sender replaced");
      sender.disconnect_multi().await;
    }
  }

  pub fn remove_closed_sender(&mut self, sid: u64) {
    let mut sender_map = self.sender_map.write();
    if sender_map
      .get(&self.id)
      .map(|s| s.sid() == sid)
      .unwrap_or_default()
    {
      sender_map.remove(&self.id);
    }
  }

  pub fn get_sender_cloned(&self) -> Option<PlayerSender> {
    self.sender_map.read().get(&self.id).cloned()
  }

  pub fn with_sender<F, R>(&self, f: F) -> Option<R>
  where
    F: FnOnce(&mut PlayerSender) -> R,
  {
    let mut sender = self.get_sender_cloned()?;
    Some(f(&mut sender))
  }
}

#[derive(Debug)]
pub struct LockedGameState {
  id: i32,
  players_snapshot: Vec<i32>,
  guard: OwnedMutexGuard<MemGameState>,
  parent: MemStorageRef,
}

impl LockedGameState {
  pub fn id(&self) -> i32 {
    self.id
  }

  pub fn status(&self) -> GameStatus {
    self.guard.status
  }

  pub fn set_status(&mut self, status: GameStatus) {
    self.guard.status = status
  }

  pub fn started(&self) -> bool {
    self.guard.start_state.is_some() || self.guard.player_tokens.is_some()
  }

  pub fn players(&self) -> &[i32] {
    &self.guard.players
  }

  pub fn get_broadcaster(&self) -> PlayerBroadcaster {
    self.parent.get_broadcaster(&self.players_snapshot)
  }

  pub fn has_player(&self, player_id: i32) -> bool {
    self.guard.players.contains(&player_id)
  }

  pub fn get_host_player(&self) -> Option<i32> {
    self.guard.host_player.clone()
  }

  pub fn add_player(&mut self, player_id: i32) {
    if !self.guard.players.contains(&player_id) {
      self.guard.players.push(player_id);
      {
        let mut s = self.parent.state.write();
        s.game_players
          .entry(self.id)
          .and_modify(|v| v.push(player_id));
      }
    }
  }

  pub fn remove_player(&mut self, player_id: i32) {
    self.guard.players.retain(|id| *id != player_id);
    {
      let mut s = self.parent.state.write();
      s.game_players
        .entry(self.id)
        .and_modify(|v| v.retain(|id| *id != player_id));
    }
  }

  pub fn close(&mut self) {
    self.parent.remove_game(self.id, &self.players_snapshot)
  }

  pub fn select_node(&mut self, node_id: Option<i32>) {
    self.guard.selected_node_id = node_id.clone();
    let mut guard = self.parent.state.write();
    if let Some(node_id) = node_id {
      guard.game_selected_node.insert(self.id, node_id);
    } else {
      guard.game_selected_node.remove(&self.id);
    }
  }

  pub fn selected_node_id(&self) -> Option<i32> {
    self.guard.selected_node_id.clone()
  }

  pub fn start(&mut self) -> bool {
    if self.guard.start_state.is_some() {
      return false;
    }

    let players = self.players_snapshot.clone();
    self.guard.start_state = Some(StartGameState::new(
      self.parent.event_sender.clone().into(),
      self.id,
      players,
    ));
    true
  }

  pub fn start_game_reset(&mut self) -> Option<StartGameState> {
    self.guard.start_state.take()
  }

  pub fn start_player_ack(
    &mut self,
    player_id: i32,
    pkt: flo_net::proto::flo_connect::PacketGameStartPlayerClientInfoRequest,
  ) {
    self
      .guard
      .start_state
      .as_mut()
      .map(move |state| state.ack_player(player_id, pkt));
  }

  pub fn start_complete(&mut self, info: &CreatedGameInfo) {
    self.guard.start_state.take();
    self.guard.player_tokens = Some(
      info
        .player_tokens
        .iter()
        .map(|token| (token.player_id, token.bytes.clone()))
        .collect(),
    )
  }
}
