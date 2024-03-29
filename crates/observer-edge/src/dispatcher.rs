use crate::broadcast::BroadcastReceiver;
use crate::constants::FLO_STATS_MAX_IN_MEMORY_GAMES;
use crate::error::{Error, Result};
use crate::game::event::{GameListUpdateEvent, GameUpdateEvent};
use crate::game::snapshot::{GameSnapshot, GameSnapshotMap, GameSnapshotWithStats};
use crate::game::stream::GameStreamMap;
use crate::game::{Game, GameHandler, GameMeta};
use crate::server::peer::GameStreamServer;
use crate::services::Services;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use flo_kinesis::data_stream::DataStreamIterator;
use flo_kinesis::iterator::Chunk;
use flo_net::observer::GameInfo;
use flo_observer::record::GameRecordData;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};
use lru::LruCache;
use std::time::Duration;
use tokio_stream::StreamExt;

pub struct Dispatcher {
  services: Services,
  slots: LruCache<i32, GameHandler>,
  inactive_cache: LruCache<i32, ()>,
  snapshots: GameSnapshotMap,
  streams: GameStreamMap,
}

impl Dispatcher {
  pub fn new(services: Services) -> Self {
    Self {
      services,
      slots: LruCache::new(*FLO_STATS_MAX_IN_MEMORY_GAMES),
      inactive_cache: LruCache::new(*FLO_STATS_MAX_IN_MEMORY_GAMES),
      snapshots: GameSnapshotMap::new(),
      streams: GameStreamMap::new(),
    }
  }

  async fn run_iter(addr: Addr<Self>, mut iter: DataStreamIterator) {
    while let Some(v) = iter.next().await {
      if addr.notify(HandleChunk(v)).await.is_err() {
        break;
      }
    }
  }

  fn handle_chunk(&mut self, ctx: &mut Context<Self>, chunk: Chunk) {
    for (game_id, game_chunk) in chunk.game_records {
      self.streams.dispatch_game_records(game_id, &game_chunk);

      if self.inactive_cache.get(&game_id).is_some() {
        continue;
      }

      let mut should_remove = false;

      match self.slots.get_mut(&game_id) {
        Some(handler) => {
          let is_last_chunk = if let Some(&GameRecordData::GameEnd) = game_chunk.records.last() {
            true
          } else {
            false
          };
          if let Err(err) = handler.handle_chunk(game_chunk, &mut self.snapshots) {
            should_remove = true;
            tracing::error!(game_id, "handle records: {}", err);
          } else {
            if is_last_chunk {
              Self::upload_archive(self.services.clone(), handler);
            }
          }
        }
        None => {
          if game_chunk.min_seq_id != 0 {
            if game_chunk.records.len() < 8 {
              tracing::debug!(
                game_id,
                "unexpected initial records: {:?}: {:?}",
                [game_chunk.min_seq_id, game_chunk.max_seq_id],
                game_chunk.records
              );
            } else {
              tracing::debug!(
                game_id,
                "unexpected initial records: {:?}",
                [game_chunk.min_seq_id, game_chunk.max_seq_id]
              );
            }
            if self.slots.len() == self.slots.cap() {
              if let Some((game_id, mut removed)) = self.slots.pop_lru() {
                tracing::info!(game_id, "expired");
                self.snapshots.remove_game(game_id);
                Self::upload_archive(self.services.clone(), &mut removed);
              }
            }
            self.inactive_cache.put(game_id, ());
            continue;
          }
          let mut handler = GameHandler::new(
            self.services.clone(),
            game_id,
            game_chunk.approximate_arrival_timestamp,
          );

          if let Err(err) = handler.handle_chunk(game_chunk, &mut self.snapshots) {
            tracing::error!(game_id, "handle initial records: {}", err);
          } else {
            if self.slots.len() == self.slots.cap() {
              if let Some((game_id, mut removed)) = self.slots.pop_lru() {
                tracing::info!(game_id, "expired");
                self.snapshots.remove_game(game_id);
                Self::upload_archive(self.services.clone(), &mut removed);
              }
            }
            self.slots.put(game_id, handler);
            ctx.spawn(Self::fetch_game(self.services.clone(), ctx.addr(), game_id));
          }
        }
      }
      if should_remove {
        if let Some(mut removed) = self.slots.pop(&game_id) {
          Self::upload_archive(self.services.clone(), &mut removed);
        }
        self.snapshots.remove_game(game_id);
      }
    }
  }

  async fn fetch_game(services: Services, addr: Addr<Self>, game_id: i32) {
    let mut backoff = ExponentialBackoff::default();
    loop {
      match services.controller.fetch_game(game_id).await {
        Ok(game) => {
          addr
            .notify(FetchGameResult {
              game_id,
              result: Ok(game),
            })
            .await
            .ok();
          break;
        }
        Err(err @ Error::InvalidGameId(_)) => {
          addr
            .notify(FetchGameResult {
              game_id,
              result: Err(err),
            })
            .await
            .ok();
          break;
        }
        Err(err) => {
          if let Some(d) = backoff.next_backoff() {
            tracing::error!("fetch game: {}, retry in {:?}...", err, d);
            tokio::time::sleep(d).await;
          } else {
            addr
              .notify(FetchGameResult {
                game_id,
                result: Err(err),
              })
              .await
              .ok();
            break;
          }
        }
      }
    }
  }

  fn upload_archive(services: Services, handler: &mut GameHandler) {
    let archiver = if let Some(handle) = services.archiver.clone() {
      handle
    } else {
      return
    };
    let game_id = handler.id();
    match handler.make_archive() {
      Ok(Some(archive)) => {
        if !archiver.add_archive(archive) {
          tracing::warn!(game_id, "archive upload cancelled");
        }
      },
      Ok(None) => {},
      Err(e) => {
        tracing::error!(
          game_id = handler.id(),
          "archive: {}",
          e
        );
      }
    }
  } 
}

#[async_trait]
impl Actor for Dispatcher {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let addr = ctx.addr();
    ctx.spawn(async move {
      let mut interval = tokio::time::interval(Duration::from_secs(60));
      interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
      loop {
        interval.tick().await;
        if addr.send(StreamGc).await.is_err() {
          break;
        }
      }
    });
  }
}

pub struct AddIterator(pub DataStreamIterator);

impl Message for AddIterator {
  type Result = ();
}

#[async_trait]
impl Handler<AddIterator> for Dispatcher {
  async fn handle(&mut self, ctx: &mut Context<Self>, AddIterator(iter): AddIterator) {
    ctx.spawn(Self::run_iter(ctx.addr(), iter));
  }
}

pub struct ListGames;

impl Message for ListGames {
  type Result = Vec<GameSnapshot>;
}

#[async_trait]
impl Handler<ListGames> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self>, _: ListGames) -> Vec<GameSnapshot> {
    self.snapshots.list_snapshots()
  }
}

pub struct GetGame {
  pub game_id: i32,
}

impl Message for GetGame {
  type Result = Result<GameSnapshot>;
}

#[async_trait]
impl Handler<GetGame> for Dispatcher {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetGame { game_id }: GetGame,
  ) -> Result<GameSnapshot> {
    self.snapshots.get_snapshot(game_id)
  }
}

pub struct GetGameInfo {
  pub game_id: i32,
}

impl Message for GetGameInfo {
  type Result = Result<(GameMeta, GameInfo)>;
}

#[async_trait]
impl Handler<GetGameInfo> for Dispatcher {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    GetGameInfo { game_id }: GetGameInfo,
  ) -> Result<(GameMeta, GameInfo)> {
    let info = self
      .slots
      .get(&game_id)
      .map(|handler| handler.make_game_info())
      .ok_or_else(|| Error::GameNotFound(game_id))??;
    Ok(info)
  }
}

pub struct SubscribeGameUpdate {
  pub game_id: i32,
}

impl Message for SubscribeGameUpdate {
  type Result = Result<(GameSnapshotWithStats, BroadcastReceiver<GameUpdateEvent>)>;
}

#[async_trait]
impl Handler<SubscribeGameUpdate> for Dispatcher {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    msg: SubscribeGameUpdate,
  ) -> Result<(GameSnapshotWithStats, BroadcastReceiver<GameUpdateEvent>)> {
    let snapshot = self
      .slots
      .get(&msg.game_id)
      .map(|handler| handler.make_snapshot_with_stats())
      .ok_or_else(|| Error::GameNotFound(msg.game_id))??;
    Ok((snapshot, self.snapshots.subscribe_game_updates(msg.game_id)))
  }
}

pub struct SubscribeGameListUpdate;

impl Message for SubscribeGameListUpdate {
  type Result = Result<(Vec<GameSnapshot>, BroadcastReceiver<GameListUpdateEvent>)>;
}

#[async_trait]
impl Handler<SubscribeGameListUpdate> for Dispatcher {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    _: SubscribeGameListUpdate,
  ) -> Result<(Vec<GameSnapshot>, BroadcastReceiver<GameListUpdateEvent>)> {
    let snapshots = self.snapshots.list_snapshots();
    Ok((snapshots, self.snapshots.subscribe_game_list_updates()))
  }
}

pub struct CreateGameStreamServer {
  pub game_id: i32,
  pub delay_secs: Option<i64>,
}

impl Message for CreateGameStreamServer {
  type Result = Result<GameStreamServer>;
}

#[async_trait]
impl Handler<CreateGameStreamServer> for Dispatcher {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CreateGameStreamServer {
      game_id,
      delay_secs,
    }: CreateGameStreamServer,
  ) -> Result<GameStreamServer> {
    match self.slots.get(&game_id) {
      Some(handler) => {
        let (snapshot, rx) =
          self
            .streams
            .subscribe(game_id, handler.initial_arrival_time(), handler.records());
        Ok(GameStreamServer::new(game_id, delay_secs, snapshot, rx))
      }
      _ => {
        return Err(Error::GameNotFound(game_id));
      }
    }
  }
}

struct HandleChunk(Chunk);

impl Message for HandleChunk {
  type Result = ();
}

#[async_trait]
impl Handler<HandleChunk> for Dispatcher {
  async fn handle(&mut self, ctx: &mut Context<Self>, HandleChunk(chunk): HandleChunk) {
    self.handle_chunk(ctx, chunk);
  }
}

struct FetchGameResult {
  game_id: i32,
  result: Result<Game>,
}

impl Message for FetchGameResult {
  type Result = ();
}

#[async_trait]
impl Handler<FetchGameResult> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self>, msg: FetchGameResult) {
    if let Some(handler) = self.slots.get_mut(&msg.game_id) {
      handler.set_fetch_result(msg.result, &mut self.snapshots);
      match handler.make_snapshot() {
        Ok(snapshot) => {
          self.snapshots.insert_game(snapshot);
        }
        Err(err) => {
          let game_id = msg.game_id;
          tracing::error!(game_id, "make snapshot: {}", err);
        }
      }
    } else {
      tracing::warn!(game_id = msg.game_id, "fetch game result discarded");
    }
  }
}

struct StreamGc;

impl Message for StreamGc {
  type Result = ();
}

#[async_trait]
impl Handler<StreamGc> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self>, _: StreamGc) {
    self.streams.remove_all_disconnected();
  }
}

#[tokio::test]
async fn test_dispatcher() -> anyhow::Result<()> {
  use flo_kinesis::data_stream::DataStream;
  use flo_kinesis::iterator::ShardIteratorType;
  use std::time::Duration;

  dotenv::dotenv()?;
  flo_log_subscriber::init();

  let services = Services::from_env();
  let d = Dispatcher::new(services).start();
  let ds = DataStream::from_env();
  let it = ShardIteratorType::at_timestamp_backward(Duration::from_secs(3600));
  d.send(AddIterator(ds.into_iter(it).await?)).await?;
  futures::future::pending::<()>().await;
  Ok(())
}
