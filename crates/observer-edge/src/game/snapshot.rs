use std::collections::BTreeMap;
use chrono::{DateTime, Utc};
use async_graphql::SimpleObject;
use super::stats::{PingStats, ActionStats, GameStatsSnapshot};
use super::{event::*, GameMeta};
use super::{Race, Game};
use crate::error::{Result, Error};

pub struct GameSnapshotMap {
  map: BTreeMap<i32, GameSnapshot>,
  tx_map: BTreeMap<i32, GameEventSender<GameUpdateEvent>>,
  tx_list: Option<GameEventSender<GameListUpdateEvent>>,
}

impl GameSnapshotMap {
  pub fn new() -> Self {
    Self {
      map: BTreeMap::new(),
      tx_map: BTreeMap::new(),
      tx_list: None,
    }
  }

  pub fn get_snapshot(&self, game_id: i32) -> Result<GameSnapshot> {
    self.map.get(&game_id).cloned().ok_or_else(|| Error::GameNotFound(game_id))
  }

  pub fn list_snapshots(&self) -> Vec<GameSnapshot> {
    self.map.values().cloned().collect()
  }

  pub fn insert_game(&mut self, snapshot: GameSnapshot) {
    self.send_game_list_update_event(|| GameListUpdateEvent::add(snapshot.clone()));
    self.map.insert(snapshot.id, snapshot);
  }

  pub fn end_game(&mut self, meta: &GameMeta) {
    if let (Some(ended_at), Some(duration)) = (meta.ended_at.clone(), meta.duration.clone()) {
      self.send_game_list_update_event(|| GameListUpdateEvent::ended(meta.id, ended_at));
      self.send_game_update_event(meta.id, || GameUpdateEvent::ended(meta.id, GameUpdateEventDataEnded {
        ended_at,
        duration_millis: duration.as_millis() as i64,
      }))
    }
  }

  pub fn remove_game(&mut self, game_id: i32) {
    self.send_game_list_update_event(|| GameListUpdateEvent::removed(game_id));
    self.tx_map.remove(&game_id);
    if let Some(snapshot) = self.map.remove(&game_id) {
      self.send_game_update_event(game_id, || {
        GameUpdateEvent::removed(snapshot)
      })
    }
  }

  pub fn insert_game_rtt_stats(&mut self, game_id: i32, item: PingStats) {
    self.send_game_update_event(game_id, || {
      GameUpdateEvent::ping_stats(game_id, item)
    })
  }

  pub fn insert_game_action_stats(&mut self, game_id: i32, item: ActionStats) {
    self.send_game_update_event(game_id, || {
      GameUpdateEvent::action_stats(game_id, item)
    })
  }

  pub fn subscribe_game_updates(&mut self, game_id: i32) -> GameEventReceiver<GameUpdateEvent> {
    match self.tx_map.get(&game_id).map(|tx| tx.subscribe()) {
      Some(rx) => rx,
      None => {
        let (tx, rx) = GameEventSender::channel();
        self.tx_map.insert(game_id, tx);
        rx
      },
    }
  }

  pub fn subscribe_game_list_updates(&mut self) -> GameEventReceiver<GameListUpdateEvent> {
    match self.tx_list.as_ref().map(|tx| tx.subscribe()) {
      Some(rx) => rx,
      None => {
        let (tx, rx) = GameEventSender::channel();
        self.tx_list.replace(tx);
        rx
      },
    }
  }

  fn send_game_list_update_event<F>(&mut self, f: F)
  where F: FnOnce() -> GameListUpdateEvent
  {
    let mut should_remove_tx = false;
    if let Some(tx) = self.tx_list.as_ref() {
      should_remove_tx = !tx.send(f());
    }
    if should_remove_tx {
      self.tx_list.take();
      tracing::debug!("game list update tx dropped");
    }
  }

  fn send_game_update_event<F>(&mut self, game_id: i32, f: F) 
  where F: FnOnce() -> GameUpdateEvent
  {
    let mut should_remove_tx = false;
    if let Some(tx) = self.tx_map.get(&game_id) {
      should_remove_tx = !tx.send(f());
    }
    if should_remove_tx {
      self.tx_map.remove(&game_id);
      tracing::debug!(game_id, "game update tx dropped");
    }
  }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GameSnapshot {
  pub id: i32,
  pub game_name: String,
  pub map_name: String,
  pub map_path: String,
  pub node_id: i32,
  pub node_name: String,
  pub started_at: DateTime<Utc>,
  pub ended_at: Option<DateTime<Utc>>,
  pub players: Vec<Player>,
}

impl GameSnapshot {
  pub fn new(meta: &GameMeta, game: &Game) -> Self {
    let players = game.slots.iter().filter_map(|slot| {
      if slot.settings.team == 24 {
        return None
      }
      if let Some(ref player) = slot.player {
        Some(Player {
          id: player.id,
          name: player.name.clone(),
          race: slot.settings.race,
          team: slot.settings.team,
        })
      } else {
        None
      }
    }).collect();
    Self {
      id: game.id,
      game_name: game.name.clone(),
      map_name: game.map.name.clone(),
      map_path: game.map.path.clone(),
      node_id: game.node.id,
      node_name: game.node.name.clone(),
      started_at: meta.started_at.clone(),
      ended_at: meta.ended_at.clone(),
      players,
    }
  }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct Player {
  pub id: i32,
  pub name: String,
  pub race: Race,
  pub team: i32,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GameSnapshotWithStats {
  pub game: GameSnapshot,
  pub stats: GameStatsSnapshot,
}