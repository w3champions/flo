use backoff::ExponentialBackoff;
use backoff::backoff::Backoff;
use lru::LruCache;
use flo_state::{Actor, async_trait, Context, Addr, Message, Handler};
use flo_kinesis::data_stream::{DataStreamIterator};
use flo_kinesis::iterator::{Chunk};
use crate::error::{Result, Error};
use crate::game::event::{GameEventReceiver, GameUpdateEvent, GameListUpdateEvent};
use crate::game::snapshot::{GameSnapshotMap, GameSnapshot, GameSnapshotWithStats};
use crate::game::{GameHandler, Game};
use crate::services::Services;
use tokio_stream::StreamExt;

const MAX_GAMES: usize = 100;

pub struct Dispatcher {
  services: Services,
  slots: LruCache<i32, Slot>,
  snapshots: GameSnapshotMap,
}

impl Dispatcher {
  pub fn new(services: Services) -> Self {
    Self {
      services,
      slots: LruCache::new(MAX_GAMES),
      snapshots: GameSnapshotMap::new(),
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
      let mut should_remove = false;
      match self.slots.get_mut(&game_id) {
        Some(Slot::Active(handler)) => {
          if let Err(err) = handler.handle_chunk(game_chunk, &mut self.snapshots) {
            should_remove = true;
            tracing::error!(game_id, "handle records: {}", err);
          }
        },
        Some(Slot::InActive) => {},
        None => {
          if game_chunk.min_seq_id != 0 {
            if game_chunk.records.len() < 8 {
              tracing::debug!(game_id, "unexpected initial records: {:?}: {:?}", [game_chunk.min_seq_id, game_chunk.max_seq_id], game_chunk.records);
            } else {
              tracing::debug!(game_id, "unexpected initial records: {:?}", [game_chunk.min_seq_id, game_chunk.max_seq_id]);
            }
            if self.slots.len() == self.slots.cap() {
              if let Some((game_id, Slot::Active(_removed))) = self.slots.pop_lru() {
                tracing::info!(game_id, "expired");
                self.snapshots.remove_game(game_id);
              }
            }
            self.slots.put(game_id, Slot::InActive);
            continue;
          }
          let handler = GameHandler::new(
            self.services.clone(),
            game_id,
            game_chunk
          );
          if self.slots.len() == self.slots.cap() {
            if let Some((game_id, Slot::Active(_removed))) = self.slots.pop_lru() {
              tracing::info!(game_id, "expired");
              self.snapshots.remove_game(game_id);
            }
          }
          self.slots.put(game_id, Slot::Active(handler));
          ctx.spawn(Self::fetch_game(self.services.clone(), ctx.addr(), game_id));
        }
      }
      if should_remove {
        self.slots.pop(&game_id);
      }
    }
  }

  async fn fetch_game(services: Services, addr: Addr<Self>, game_id: i32) {
    let mut backoff = ExponentialBackoff::default();
    loop {
      match services.controller.fetch_game(game_id).await {
        Ok(game) => {
          addr.notify(FetchGameResult {
            game_id,
            result: Ok(game)
          }).await.ok();
          break;
        },
        Err(err @ Error::InvalidGameId(_)) => {
          addr.notify(FetchGameResult {
            game_id,
            result: Err(err),
          }).await.ok();
          break;
        },
        Err(err) => {
          if let Some(d) = backoff.next_backoff() {
            tracing::error!("fetch game: {}, retry in {:?}...", err, d);
            tokio::time::sleep(d).await;
          } else {
            addr.notify(FetchGameResult {
              game_id,
              result: Err(err),
            }).await.ok();
            break;
          }
        }
      }
    }
  }
}

impl Actor for Dispatcher {}

enum Slot {
  InActive,
  Active(GameHandler),
}

pub struct AddIterator(pub DataStreamIterator);

impl Message for AddIterator {
  type Result = ();
}

#[async_trait]
impl Handler<AddIterator> for Dispatcher {
  async fn handle(&mut self, ctx: &mut Context<Self> , AddIterator(iter): AddIterator) {
    ctx.spawn(Self::run_iter(ctx.addr(), iter));
  }
}

pub struct ListGames;

impl Message for ListGames {
  type Result = Vec<GameSnapshot>;
}

#[async_trait]
impl Handler<ListGames> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self> , _: ListGames) -> Vec<GameSnapshot> {
    self.snapshots.list_snapshots()
  }
}

pub struct SubscribeGameUpdate {
  pub game_id: i32,
}

impl Message for SubscribeGameUpdate {
  type Result = Result<(GameSnapshotWithStats, GameEventReceiver<GameUpdateEvent>)>;
}

#[async_trait]
impl Handler<SubscribeGameUpdate> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self> , msg: SubscribeGameUpdate) -> Result<(GameSnapshotWithStats, GameEventReceiver<GameUpdateEvent>)> {
    let snapshot = self.slots.get(&msg.game_id)
      .and_then(|slot| if let Slot::Active(ref handler) = slot {
        Some(handler.make_snapshot_with_stats())
      } else {
        None
      })
      .ok_or_else(|| Error::GameNotFound(msg.game_id))??;
    Ok((snapshot, self.snapshots.subscribe_game_updates(msg.game_id)))
  }
}

pub struct SubscribeGameListUpdate;

impl Message for SubscribeGameListUpdate {
  type Result = Result<(Vec<GameSnapshot>, GameEventReceiver<GameListUpdateEvent>)>;
}

#[async_trait]
impl Handler<SubscribeGameListUpdate> for Dispatcher {
  async fn handle(&mut self, _: &mut Context<Self> , _: SubscribeGameListUpdate) -> Result<(Vec<GameSnapshot>, GameEventReceiver<GameListUpdateEvent>)> {
    let snapshots = self.snapshots.list_snapshots();
    Ok((snapshots, self.snapshots.subscribe_game_list_updates()))
  }
}

struct HandleChunk(Chunk);

impl Message for HandleChunk {
  type Result = ();
}

#[async_trait]
impl Handler<HandleChunk> for Dispatcher {
  async fn handle(&mut self, ctx: &mut Context<Self> , HandleChunk(chunk): HandleChunk) {
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
  async fn handle(&mut self, _: &mut Context<Self> , msg: FetchGameResult) {
    if let Some(Slot::Active(handler)) = self.slots.get_mut(&msg.game_id) {
      handler.set_fetch_result(msg.result, &mut self.snapshots);
      match handler.make_snapshot() {
        Ok(snapshot) => {
          self.snapshots.insert_game(snapshot);
        },
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

#[tokio::test]
async fn test_dispatcher() -> anyhow::Result<()> {
  use flo_kinesis::data_stream::{DataStream};
  use flo_kinesis::iterator::ShardIteratorType;
  use std::time::Duration;

  dotenv::dotenv()?;
  flo_log_subscriber::init();

  let services = Services::from_env();
  let d = Dispatcher::new(services).start();
  let ds = DataStream::from_env();
  let it = ShardIteratorType::at_timestamp_backward(Duration::from_secs(3600));
  d.send(AddIterator(
    ds.into_iter(it).await?
  )).await?;
  futures::future::pending::<()>().await;
  Ok(())
}