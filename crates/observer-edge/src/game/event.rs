use super::{snapshot::GameSnapshot, stats::{PingStats, ActionStats}};
use async_graphql::{SimpleObject, Union};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::{
  Stream,
  StreamExt,
  wrappers::BroadcastStream
};

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
}

#[derive(Clone, Union)]
pub enum GameUpdateEventData {
  Ended(GameUpdateEventDataEnded),
  Removed(GameUpdateEventDataRemoved),
  PingStats(PingStats),
  ActionStats(ActionStats),
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

pub struct GameEventSender<E> {
  tx: broadcast::Sender<E>,
}

impl<E> GameEventSender<E>
where
  E: Clone,
{
  pub fn channel() -> (Self, GameEventReceiver<E>) {
    let (tx, rx) = broadcast::channel(16);
    (GameEventSender { tx }, GameEventReceiver { rx })
  }

  pub fn send(&self, event: E) -> bool {
    self.tx.send(event).is_ok()
  }

  pub fn subscribe(&self) -> GameEventReceiver<E> {
    GameEventReceiver {
      rx: self.tx.subscribe(),
    }
  }
}

pub struct GameEventReceiver<E> {
  rx: broadcast::Receiver<E>,
}

impl<E> GameEventReceiver<E> {
  pub fn into_stream(self) -> impl Stream<Item = E>
  where
    E: Clone + Send + 'static,
  {
    BroadcastStream::new(self.rx).filter_map(|item| item.ok())
  }
}
