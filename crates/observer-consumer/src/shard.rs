use crate::archiver::ArchiverHandle;
use crate::error::{Error, Result};
use crate::fs::GameDataWriter;
use crate::persist::{Persist, PersistGameState};
use crate::{RemoveShard, ShardsMgr};
use backoff::backoff::Backoff;
use bytes::{Bytes, BytesMut};
use flo_observer::record::{GameRecordData, KMSRecord};
use flo_observer::{KINESIS_CLIENT, KINESIS_STREAM_NAME};
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message};
use rusoto_kinesis::Kinesis;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::Span;

#[derive(Debug)]
pub struct ShardConsumer {
  shard_id: String,
  parent: Addr<ShardsMgr>,
  persist: Persist,
  uploader: ArchiverHandle,
  games: BTreeMap<i32, GameEntry>,
  lost_games: BTreeSet<i32>,
  span: Span,
  max_sequence_number: Option<String>,
}

impl ShardConsumer {
  pub(crate) fn new(
    shard_id: String,
    parent: Addr<ShardsMgr>,
    persist: Persist,
    uploader: ArchiverHandle,
  ) -> Self {
    let span = tracing::info_span!("shard_consumer", shard_id = shard_id.as_str());
    Self {
      shard_id,
      parent,
      persist,
      uploader,
      games: BTreeMap::new(),
      lost_games: BTreeSet::new(),
      span,
      max_sequence_number: None,
    }
  }

  async fn flush(&mut self) -> Result<()> {
    if let Some(max_sequence_number) = self.max_sequence_number.take() {
      for entry in self.games.values_mut() {
        entry.writer.flush_state().await?;
      }

      self
        .span
        .in_scope(|| tracing::debug!("flush max sequence number: {}", max_sequence_number));
      self
        .persist
        .set_shard_finished_seq(&self.shard_id, &max_sequence_number)
        .await?;
    }
    Ok(())
  }

  async fn gc(&mut self) -> Result<()> {
    let mut removed = None;
    let now = Instant::now();
    for (id, entry) in self.games.iter_mut() {
      if entry.part_sender.is_some() {
        continue;
      }
      if now.saturating_duration_since(entry.t) > Duration::from_secs(90) {
        let removed = removed.get_or_insert_with(|| vec![]);
        removed.push(*id);
        self.span.in_scope(|| {
          tracing::debug!("archiving game: {}", *id);
        });
        entry.writer.build_archive(true).await?;
        if removed.len() == Gc::MAX_ITEM {
          break;
        }
      }
    }
    if let Some(removed) = removed {
      self.span.in_scope(|| {
        tracing::info!("remove games: {:?}", removed);
      });

      for id in removed {
        let game = self.games.remove(&id);
        self.persist.remove_game(id).await?;
        if let Some(game) = game {
          self
            .uploader
            .add_folder(game.writer.data_dir().to_owned())
            .await;
        }
      }
    }
    let pause_time = Instant::now() - now;
    for entry in self.games.values_mut() {
      entry.t = entry.t + pause_time;
    }
    Ok(())
  }
}

#[async_trait]
impl Actor for ShardConsumer {
  async fn started(&mut self, _: &mut Context<Self>) {}
}

pub struct StartShardConsumer {
  pub recovered_games: Vec<PersistGameState>,
}

impl Message for StartShardConsumer {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<StartShardConsumer> for ShardConsumer {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    StartShardConsumer { recovered_games }: StartShardConsumer,
  ) -> Result<()> {
    self.span.in_scope(|| {
      tracing::info!("starting: recovered_games = {}", recovered_games.len());
    });

    for game in recovered_games {
      let game_id = game.id;
      if let Err(err) = self.recover_entry(game).await {
        self.span.in_scope(|| {
          tracing::error!("recover game {}: {}", game_id, err);
        })
      }
    }

    ctx.spawn(
      Scanner {
        shard_id: self.shard_id.clone(),
        cache: self.persist.clone(),
        addr: ctx.addr(),
        span: tracing::info_span!("scanner", shard_id = self.shard_id.as_str()),
      }
      .run(),
    );

    ctx.send_later(Flush, Flush::INTERVAL);
    ctx.send_later(Gc, Gc::INTERVAL);

    Ok(())
  }
}

struct ImportChunk {
  max_sequence_number: String,
  game_records: BTreeMap<i32, GameRecords>,
}

struct GameRecords {
  min_seq_id: u32,
  max_seq_id: u32,
  records: Vec<GameRecordData>,
}

impl Message for ImportChunk {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<ImportChunk> for ShardConsumer {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    ImportChunk {
      max_sequence_number,
      game_records,
    }: ImportChunk,
  ) -> Result<()> {
    for (game_id, records) in game_records {
      if self.lost_games.contains(&game_id) {
        continue;
      }

      match self.write_records(game_id, records).await {
        Ok(_) => {}
        Err(Error::GameDataLost(_)) => {
          self.lost_games.insert(game_id);
          self.games.remove(&game_id);
          self.persist.remove_game(game_id).await?;
        }
        Err(err) => return Err(err),
      }
    }
    self.max_sequence_number.replace(max_sequence_number);
    Ok(())
  }
}

struct Flush;

impl Message for Flush {
  type Result = ();
}

impl Flush {
  const INTERVAL: Duration = Duration::from_secs(1);
}

#[async_trait]
impl Handler<Flush> for ShardConsumer {
  async fn handle(&mut self, ctx: &mut Context<Self>, _: Flush) {
    if let Err(err) = self.flush().await {
      self
        .span
        .in_scope(|| tracing::error!("flush error: {}", err));
      self
        .parent
        .notify(RemoveShard {
          shard_id: self.shard_id.clone(),
        })
        .await
        .ok();
    } else {
      ctx.send_later(Flush, Flush::INTERVAL);
    }
  }
}

struct Gc;

impl Gc {
  const MAX_ITEM: usize = 10;
  const INTERVAL: Duration = Duration::from_secs(60);
}

impl Message for Gc {
  type Result = ();
}

#[async_trait]
impl Handler<Gc> for ShardConsumer {
  async fn handle(&mut self, ctx: &mut Context<Self>, _: Gc) {
    if let Err(err) = self.gc().await {
      self.span.in_scope(|| tracing::error!("gc error: {}", err));
      self
        .parent
        .notify(RemoveShard {
          shard_id: self.shard_id.clone(),
        })
        .await
        .ok();
    } else {
      ctx.send_later(Gc, Gc::INTERVAL);
    }
  }
}

struct Connect {
  game_id: i32,
}

impl Message for Connect {
  type Result = Result<Option<(Bytes, mpsc::Receiver<Bytes>)>>;
}

#[async_trait]
impl Handler<Connect> for ShardConsumer {
  async fn handle(
    &mut self,
    _ctx: &mut Context<Self>,
    Connect { game_id }: Connect,
  ) -> Result<Option<(Bytes, mpsc::Receiver<Bytes>)>> {
    let entry = match self.games.get_mut(&game_id) {
      Some(entry) => entry,
      None => return Ok(None),
    };

    Ok(entry.connect_parts_channel().await?.into())
  }
}

struct Scanner {
  shard_id: String,
  cache: Persist,
  addr: Addr<ShardConsumer>,
  span: Span,
}

impl Scanner {
  const CHUNK_SIZE: i64 = 10000;
  const WAIT_RECORD_TIMEOUT: Duration = Duration::from_millis(1000);
  const GET_RECORDS_SLEEP: Duration = Duration::from_millis(250);

  async fn run(mut self) {
    use rusoto_kinesis::{GetRecordsError, GetRecordsInput};

    let mut backoff = backoff::ExponentialBackoff {
      max_elapsed_time: None,
      ..Default::default()
    };

    'main: loop {
      let mut iterator = 'get_iter: loop {
        match self.refresh_iterator().await {
          Ok(v) => {
            backoff.reset();
            break 'get_iter v;
          }
          Err(err) => {
            if let Some(v) = backoff.next_backoff() {
              self
                .span
                .in_scope(|| tracing::error!("refresh_iterator: {}", err));
              tokio::time::sleep(v).await;
            } else {
              self
                .span
                .in_scope(|| tracing::error!("refresh_iterator: max backoff reached"));
              return;
            }
          }
        }
      };

      loop {
        let t = Instant::now();
        let input = GetRecordsInput {
          limit: Some(Self::CHUNK_SIZE),
          shard_iterator: iterator.clone(),
        };

        match KINESIS_CLIENT.get_records(input).await {
          Ok(output) => {
            backoff.reset();
            let records = output.records;
            self.span.in_scope(|| {
              tracing::debug!("records: {}", records.len());
            });

            if !records.is_empty() {
              if let Err(err) = self.handle_chunk(&records).await {
                self
                  .span
                  .in_scope(|| tracing::error!("handle_chunk: {}", err));
                break 'main;
              }
            }

            iterator = if let Some(v) = output.next_shard_iterator {
              v
            } else {
              self.span.in_scope(|| tracing::warn!("shard closed"));
              break 'main;
            };
            if records.is_empty() {
              tokio::time::sleep(Self::WAIT_RECORD_TIMEOUT).await;
            }
          }
          Err(rusoto_core::RusotoError::Service(GetRecordsError::ExpiredIterator(_))) => {
            self.span.in_scope(|| tracing::warn!("iterator expired"));
            break;
          }
          Err(err) => {
            self.span.in_scope(|| {
              tracing::error!("get records: {}", err);
            });
            if let Some(duration) = backoff.next_backoff() {
              tokio::time::sleep(duration).await;
            } else {
              self
                .span
                .in_scope(|| tracing::error!("max backoff reached"));
              break 'main;
            }
          }
        }
        let d = Instant::now() - t;
        if let Some(d) = Self::GET_RECORDS_SLEEP.checked_sub(d) {
          tokio::time::sleep(d).await;
        }
      }
    }
    self.span.in_scope(|| {
      tracing::error!("scanner terminated");
    });
  }

  async fn refresh_iterator(&mut self) -> Result<String> {
    use rusoto_kinesis::GetShardIteratorInput;
    let starting_sequence_number = self.cache.get_shard_finished_seq(&self.shard_id).await?;
    self.span.in_scope(|| {
      tracing::info!("starting_sequence_number = {:?}", starting_sequence_number);
    });

    let input = GetShardIteratorInput {
      shard_id: self.shard_id.clone(),
      shard_iterator_type: if starting_sequence_number.is_some() {
        "AFTER_SEQUENCE_NUMBER".to_string()
      } else {
        "TRIM_HORIZON".to_string()
      },
      starting_sequence_number,
      stream_name: KINESIS_STREAM_NAME.clone(),
      ..Default::default()
    };

    let res = KINESIS_CLIENT.get_shard_iterator(input).await?;
    let iterator = res
      .shard_iterator
      .ok_or_else(|| Error::NoShardIterator(self.shard_id.clone()))?;

    self.span.in_scope(|| {
      tracing::info!("iterator = {}", iterator);
    });

    Ok(iterator)
  }

  async fn handle_chunk(&mut self, records: &Vec<rusoto_kinesis::Record>) -> Result<()> {
    let span = &self.span;
    let mut map = BTreeMap::new();
    let mut lost_games = BTreeSet::new();
    let max_sequence_number = records.last().map(|r| r.sequence_number.clone()).unwrap();

    for r in records {
      let bytes = r.data.clone();
      let source = KMSRecord::peek_source(bytes.as_ref())?;
      if source != crate::env::ENV.record_source {
        continue;
      }
      let record = KMSRecord::decode(bytes)?;
      for (seq_id, r) in record.records {
        let entry = map.entry(r.game_id).or_insert_with(|| GameRecords {
          min_seq_id: seq_id,
          max_seq_id: u32::MAX,
          records: vec![],
        });
        if entry.max_seq_id == u32::MAX || entry.max_seq_id == seq_id - 1 {
          entry.max_seq_id = seq_id;
          entry.records.push(r.data);
        } else {
          if lost_games.contains(&r.game_id) {
            continue;
          } else {
            span.in_scope(|| {
              tracing::warn!(
                game_id = r.game_id,
                "records discarded: non-continuous chunk seq id: {} -> {}",
                entry.max_seq_id,
                seq_id
              );
            });
            lost_games.insert(r.game_id);
          }
        }
      }
    }

    self
      .addr
      .send(ImportChunk {
        max_sequence_number,
        game_records: map,
      })
      .await??;

    Ok(())
  }
}

impl ShardConsumer {
  async fn write_records(
    &mut self,
    game_id: i32,
    GameRecords {
      min_seq_id,
      max_seq_id,
      records,
    }: GameRecords,
  ) -> Result<()> {
    let entry = match self.games.entry(game_id) {
      Entry::Vacant(entry) => {
        if min_seq_id != 0 {
          self.span.in_scope(|| {
            tracing::warn!(game_id, "game data lost: first chunk id is not 0");
          });
          return Err(Error::GameDataLost(game_id));
        }
        self.persist.add_game(game_id, &self.shard_id).await?;
        entry.insert(GameEntry {
          t: Instant::now(),
          writer: GameDataWriter::create_or_recover(game_id).await?,
          part_sender: None,
        })
      }
      Entry::Occupied(entry) => {
        let entry = entry.into_mut();
        if entry.writer.next_record_id() < min_seq_id {
          self.span.in_scope(|| {
            tracing::warn!(
              game_id,
              "game data lost: non-continuous chunk id: {} -> {}",
              entry.writer.next_record_id(),
              min_seq_id
            );
          });
          entry.part_sender.take();
          return Err(Error::GameDataLost(game_id));
        }
        entry.t = Instant::now();
        entry
      }
    };

    if min_seq_id + records.len() as u32 != max_seq_id + 1 {
      self.span.in_scope(|| {
        tracing::warn!(
          game_id,
          "game data lost: non-continuous record ids: {} + {} != {}",
          min_seq_id,
          records.len(),
          max_seq_id + 1
        );
      });
      entry.part_sender.take();
      return Err(Error::GameDataLost(game_id));
    }

    let mut max_discard_id = None;
    let mut start_write_id = None;
    let mut send_parts = entry.part_sender.as_mut().map(|s| (s, BytesMut::new()));
    for (idx, record) in records.into_iter().enumerate() {
      let record_id = min_seq_id + idx as u32;
      if entry.writer.next_record_id() == record_id {
        if start_write_id.is_none() {
          start_write_id.replace(record_id);
        }
        if let Some((_, buf)) = send_parts.as_mut() {
          record.encode(buf);
        }
        entry.writer.write_record(record).await?;
      } else {
        max_discard_id.replace(record_id);
      }
    }

    if let Some((s, buf)) = send_parts {
      let drop_sender = if !buf.is_empty() {
        s.send(buf.freeze().into()).await.is_err()
      } else {
        false
      };
      if drop_sender {
        entry.part_sender.take();
        self.span.in_scope(|| {
          tracing::warn!(game_id, "game part sender dropped");
        });
      }
    }

    if let Some(id) = max_discard_id {
      self.span.in_scope(|| {
        tracing::warn!(
          game_id,
          "discard records: {} - {}, start write id: {:?}",
          min_seq_id,
          id,
          start_write_id
        )
      });
    }

    Ok(())
  }

  async fn recover_entry(&mut self, game: PersistGameState) -> Result<()> {
    let writer = GameDataWriter::recover(game.id).await?;
    self
      .span
      .in_scope(|| tracing::info!("recovered: game_id = {}", game.id));
    self.games.insert(
      game.id,
      GameEntry {
        t: Instant::now(),
        writer,
        part_sender: None,
      },
    );
    Ok(())
  }
}

#[derive(Debug)]
struct GameEntry {
  t: Instant,
  writer: GameDataWriter,
  part_sender: Option<mpsc::Sender<Bytes>>,
}

impl GameEntry {
  async fn connect_parts_channel(&mut self) -> Result<(Bytes, mpsc::Receiver<Bytes>)> {
    let part = self.writer.build_initial_part().await?;
    let (tx, rx) = mpsc::channel(32);
    self.part_sender.replace(tx);
    Ok((part, rx))
  }
}

#[tokio::test]
async fn test_list_shards() {
  use flo_observer::KINESIS_CLIENT;
  use rusoto_kinesis::{Kinesis, ListShardsInput};

  dotenv::dotenv().unwrap();

  let shards = KINESIS_CLIENT
    .list_shards(ListShardsInput {
      stream_name: Some(flo_observer::KINESIS_STREAM_NAME.clone()),
      ..Default::default()
    })
    .await
    .unwrap();

  dbg!(shards);
}
