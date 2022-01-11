mod env;
mod controller;
pub mod game;
mod error;
mod services;
mod dispatcher;

use std::time::Duration;
use flo_kinesis::{data_stream::DataStream, iterator::ShardIteratorType};
use flo_state::{Owner, Addr, Actor};
use dispatcher::{Dispatcher, AddIterator, ListGames, SubscribeGameListUpdate, SubscribeGameUpdate};
use error::Result;
use game::{snapshot::{GameSnapshot, GameSnapshotWithStats}, event::{GameEventReceiver, GameListUpdateEvent, GameUpdateEvent}};
use services::Services;

pub struct FloObserverEdge {
  dispatcher: Owner<Dispatcher>,
}

impl FloObserverEdge {
  pub async fn from_env() -> Result<Self> {
    let services = Services::from_env();
    let dispatcher = Dispatcher::new(services).start();
    let data_stream = DataStream::from_env();
    let iter_type = ShardIteratorType::at_timestamp_backward(Duration::from_secs(3600));

    tracing::debug!("creating iterator...");

    let iter = data_stream.into_iter(iter_type).await?;

    tracing::debug!("iterator created.");

    dispatcher.send(AddIterator(iter)).await?;

    tracing::debug!("iterator added.");

    Ok(Self {
      dispatcher,
    })
  }

  pub fn handle(&self) -> FloObserverEdgeHandle {
    FloObserverEdgeHandle(self.dispatcher.addr())
  }
}

#[derive(Clone)]
pub struct FloObserverEdgeHandle(Addr<Dispatcher>);

impl FloObserverEdgeHandle {
  pub async fn list_games(&self) -> Result<Vec<GameSnapshot>> {
    self.0.send(ListGames).await.map_err(Into::into)
  }

  pub async fn subscribe_game_list_updates(&self) -> Result<(Vec<GameSnapshot>, GameEventReceiver<GameListUpdateEvent>)> {
    self.0.send(SubscribeGameListUpdate).await?
  }

  pub async fn subscribe_game_updates(&self, game_id: i32) -> Result<(GameSnapshotWithStats, GameEventReceiver<GameUpdateEvent>)> {
    self.0.send(SubscribeGameUpdate {
      game_id
    }).await?
  }
}