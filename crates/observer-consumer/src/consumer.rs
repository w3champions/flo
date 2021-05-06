use crate::cache::{Cache, CacheGameState};
use crate::error::{Error, Result};
use crate::fs::GameDataWriter;
use backoff::backoff::Backoff;
use flo_observer::record::{GameRecord, GameRecordData};
use flo_observer::{KINESIS_CLIENT, KINESIS_STREAM_NAME};
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message, Owner};
use rusoto_core::RusotoError;
use rusoto_kinesis::{GetRecordsError, GetRecordsOutput, Kinesis};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::Span;

#[derive(Debug)]
pub struct ShardConsumer {
  shard_id: String,
  cache: Cache,
  games: BTreeMap<i32, GameEntry>,
  span: Span,
}

impl ShardConsumer {
  pub fn new(shard_id: String, cache: Cache) -> Self {
    let span = tracing::info_span!("shard_consumer", shard_id = shard_id.as_str());
    Self {
      shard_id,
      cache,
      games: BTreeMap::new(),
      span,
    }
  }
}

#[async_trait]
impl Actor for ShardConsumer {
  async fn started(&mut self, ctx: &mut Context<Self>) {}
}

pub struct StartShardConsumer {
  pub recovered_games: Vec<CacheGameState>,
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
    use rusoto_kinesis::GetShardIteratorInput;

    self.span.in_scope(|| {
      tracing::info!("starting: recovered_games = {}", recovered_games.len());
    });

    for game in recovered_games {
      self.recover_entry(game).await?;
    }

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

    ctx.spawn(
      Scanner {
        shard_id: self.shard_id.clone(),
        iterator,
        addr: ctx.addr(),
      }
      .run(),
    );

    Ok(())
  }
}

struct Scanner {
  shard_id: String,
  iterator: String,
  addr: Addr<ShardConsumer>,
}

impl Scanner {
  const CHUNK_SIZE: i64 = 1000;
  const WAIT_RECORD_TIMEOUT: Duration = Duration::from_millis(100);

  async fn run(mut self) {
    use rusoto_kinesis::GetRecordsInput;

    let span = tracing::info_span!("scanner", shard_id = self.shard_id.as_str());
    let mut backoff = backoff::ExponentialBackoff {
      max_elapsed_time: None,
      ..Default::default()
    };

    loop {
      let input = GetRecordsInput {
        limit: Some(Self::CHUNK_SIZE),
        shard_iterator: self.iterator.clone(),
      };

      match KINESIS_CLIENT.get_records(input).await {
        Ok(output) => {
          backoff.reset();
          let records = output.records;
          span.in_scope(|| {
            tracing::debug!("records: {}", records.len());
          });
          self.iterator = if let Some(v) = output.next_shard_iterator {
            v
          } else {
            span.in_scope(|| tracing::warn!("shard closed"));
            break;
          };
          if records.is_empty() {
            tokio::time::sleep(Self::WAIT_RECORD_TIMEOUT).await;
          }
        }
        Err(err) => {
          span.in_scope(|| {
            tracing::error!("get records: {}", err);
          });
          if let Some(duration) = backoff.next_backoff() {
            tokio::time::sleep(duration).await;
          } else {
            span.in_scope(|| tracing::error!("max backoff reached"));
            break;
          }
        }
      }
    }
  }
}

impl ShardConsumer {
  async fn write_records(
    &mut self,
    game_id: i32,
    seq_id: u32,
    records: Vec<GameRecordData>,
  ) -> Result<()> {
    let entry = match self.games.entry(game_id) {
      Entry::Vacant(entry) => entry.insert(GameEntry {
        finished_seq_id: None,
        writer: GameDataWriter::create(game_id).await?,
      }),
      Entry::Occupied(entry) => entry.into_mut(),
    };

    if entry.finished_seq_id.map(|v| v >= seq_id) == Some(true) {
      return Ok(());
    }

    for record in records {
      entry.writer.write_record(record).await?;
    }

    Ok(())
  }

  async fn recover_entry(&mut self, game: CacheGameState) -> Result<()> {
    let writer = GameDataWriter::recover(game.id).await?;
    self.games.insert(
      game.id,
      GameEntry {
        finished_seq_id: Some(game.finished_seq_id),
        writer,
      },
    );
    Ok(())
  }
}

#[derive(Debug)]
struct GameEntry {
  finished_seq_id: Option<u32>,
  writer: GameDataWriter,
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
