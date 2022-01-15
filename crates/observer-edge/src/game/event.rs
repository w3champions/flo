use crate::game::{
  snapshot::GameSnapshot,
  stats::{ActionStats, PingStats},
  PlayerLeaveReason,
};
use async_graphql::{SimpleObject, Union};
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Clone, SimpleObject)]
pub struct GameUpdateEvent {
  pub game_id: i32,
  pub data: GameUpdateEventData,
}

impl GameUpdateEvent {
  pub fn ended(game_id: i32, data: GameUpdateEventDataEnded) -> Self {
    GameUpdateEvent {
      game_id,
      data: GameUpdateEventData::Ended(data),
    }
  }

  pub fn removed(snapshot: GameSnapshot) -> Self {
    GameUpdateEvent {
      game_id: snapshot.id,
      data: GameUpdateEventData::Removed(GameUpdateEventDataRemoved {
        snapshot: Arc::new(snapshot),
      }),
    }
  }

  pub fn ping_stats(game_id: i32, item: PingStats) -> Self {
    GameUpdateEvent {
      game_id,
      data: GameUpdateEventData::PingStats(item),
    }
  }

  pub fn action_stats(game_id: i32, item: ActionStats) -> Self {
    GameUpdateEvent {
      game_id,
      data: GameUpdateEventData::ActionStats(item),
    }
  }

  pub fn player_left(game_id: i32, time: u32, player_id: i32, reason: PlayerLeaveReason) -> Self {
    GameUpdateEvent {
      game_id,
      data: GameUpdateEventData::PlayerLeft(GameUpdateEventDataPlayerLeft { time, player_id, reason }),
    }
  }
}

#[derive(Clone, Union)]
pub enum GameUpdateEventData {
  Ended(GameUpdateEventDataEnded),
  Removed(GameUpdateEventDataRemoved),
  PingStats(PingStats),
  ActionStats(ActionStats),
  PlayerLeft(GameUpdateEventDataPlayerLeft),
}

#[derive(Clone, SimpleObject)]
pub struct GameUpdateEventDataEnded {
  pub ended_at: DateTime<Utc>,
  pub duration_millis: i64,
}

#[derive(Clone, SimpleObject)]
pub struct GameUpdateEventDataRemoved {
  pub snapshot: Arc<GameSnapshot>,
}

#[derive(Clone, SimpleObject)]
pub struct GameUpdateEventDataPlayerLeft {
  pub time: u32,
  pub player_id: i32,
  pub reason: PlayerLeaveReason,
}

#[derive(Clone, Union)]
pub enum GameListUpdateEvent {
  Added(GameListUpdateEventAdded),
  Ended(GameListUpdateEventEnded),
  Removed(GameListUpdateEventRemoved),
}

#[derive(Clone, SimpleObject)]
pub struct GameListUpdateEventAdded {
  pub snapshot: Arc<GameSnapshot>,
}

#[derive(Clone, SimpleObject)]
pub struct GameListUpdateEventEnded {
  pub game_id: i32,
  pub ended_at: DateTime<Utc>,
}

#[derive(Clone, SimpleObject)]
pub struct GameListUpdateEventRemoved {
  pub game_id: i32,
}

impl GameListUpdateEvent {
  pub fn add(snapshot: GameSnapshot) -> Self {
    Self::Added(GameListUpdateEventAdded {
      snapshot: Arc::new(snapshot),
    })
  }

  pub fn ended(game_id: i32, ended_at: DateTime<Utc>) -> Self {
    Self::Ended(GameListUpdateEventEnded { game_id, ended_at })
  }

  pub fn removed(game_id: i32) -> Self {
    Self::Removed(GameListUpdateEventRemoved { game_id })
  }
}