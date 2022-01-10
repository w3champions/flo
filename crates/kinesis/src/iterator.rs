use std::{sync::Arc, time::{Duration, Instant, SystemTime}, collections::{BTreeMap, BTreeSet}, pin::Pin, task::{Context, Poll}};
use backoff::backoff::Backoff;
use rusoto_kinesis::{KinesisClient, Kinesis};
use tokio::sync::mpsc::{Sender, Receiver, channel};
use flo_observer::record::{KMSRecord, ObserverRecordSource, GameRecordData};
use tracing::Span;
use tokio_stream::Stream;
use crate::error::{Result, Error};

#[derive(Debug, Clone)]
pub enum ShardIteratorType {
  AtSequenceNumber(String),
  AfterSequenceNumber(String),
  AtTimestamp(f64),
  TrimHorizon,
  Latest,
}

impl ShardIteratorType {
  pub fn at_timestamp_backward(duration: Duration) -> ShardIteratorType {
    let t = SystemTime::now().checked_sub(duration)
      .unwrap_or_else(|| SystemTime::UNIX_EPOCH);
    let t = t.duration_since(SystemTime::UNIX_EPOCH)
      .map(|d| d.as_millis() as f64 / 1000.)
      .unwrap_or_else(|_| 0.);
    ShardIteratorType::AtTimestamp(t)
  }
}

#[derive(Debug)]
pub struct ShardIterator {
  rx: Receiver<Item>,
}

pub struct ShardIteratorConfig {
  pub iter_type: ShardIteratorType,
  pub source: ObserverRecordSource,
  pub client: Arc<KinesisClient>,
  pub stream_name: String,
  pub shard_id: String,
}

impl ShardIterator {
  pub fn new(ShardIteratorConfig {
    iter_type,
    source,
    client,
    stream_name,
    shard_id,
  }: ShardIteratorConfig) -> Self {
    let (tx, rx) = channel(32);

    let span = tracing::info_span!("shard_iterator", shard_id = shard_id.as_str());

    let scanner = Scanner {
      iter_type,
      source,
      client,
      stream_name,
      shard_id,
      tx,
      span,
    };

    tokio::spawn(scanner.run());

    Self {
      rx
    }
  }
}

impl Stream for ShardIterator {
  type Item = Chunk;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.rx.poll_recv(cx).map(|item| {
      item.and_then(|item| {
        match item {
          Item::Chunk(chunk) => Some(chunk),
          Item::Terminated => None,
        }
      })
    })
  }
}

struct Scanner {
  iter_type: ShardIteratorType,
  source: ObserverRecordSource,
  client: Arc<KinesisClient>,
  stream_name: String,
  shard_id: String,
  tx: Sender<Item>,
  span: Span,
}

impl Scanner {
  const CHUNK_SIZE: i64 = 10000;
  const WAIT_RECORD_TIMEOUT: Duration = Duration::from_millis(1000);
  const GET_RECORDS_SLEEP: Duration = Duration::from_millis(1000);

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

        match self.client.get_records(input).await {
          Ok(output) => {
            backoff.reset();
            let records = output.records;
            self.span.in_scope(|| {
              tracing::debug!("records: {}", records.len());
            });

            if !records.is_empty() {
              if let Err(err) = self.handle_chunk(output.millis_behind_latest, &records).await {
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
              self.span.in_scope(|| tracing::debug!("shard ex"));
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
    self.tx.send(Item::Terminated).await.ok();
  }

  async fn handle_chunk(&mut self, millis_behind_latest: Option<i64>, records: &Vec<rusoto_kinesis::Record>) -> Result<()> {
    let span = &self.span;
    let mut map = BTreeMap::new();
    let mut lost_games = BTreeSet::new();
    let max_sequence_number = records.last().map(|r| r.sequence_number.clone()).unwrap();

    for r in records {
      let approximate_arrival_timestamp = r.approximate_arrival_timestamp.clone().unwrap_or_default();
      let bytes = r.data.clone();
      let source = KMSRecord::peek_source(bytes.as_ref())?;
      if source != self.source {
        continue;
      }
      let record = KMSRecord::decode(bytes)?;
      for (seq_id, r) in record.records {
        let entry = map.entry(r.game_id).or_insert_with(|| GameChunk {
          approximate_arrival_timestamp,
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
      .tx
      .send(Item::Chunk(Chunk {
        max_sequence_number,
        millis_behind_latest,
        game_records: map,
      }))
      .await.map_err(|_| Error::Cancelled)?;

    Ok(())
  }

  async fn refresh_iterator(&mut self) -> Result<String> {
    use rusoto_kinesis::GetShardIteratorInput;

    let (shard_iterator_type, starting_sequence_number, timestamp) = match self.iter_type {
        ShardIteratorType::AtSequenceNumber(ref v) => (
          "AT_SEQUENCE_NUMBER",
          Some(v.clone()),
          None
        ),
        ShardIteratorType::AfterSequenceNumber(ref v) => (
          "AFTER_SEQUENCE_NUMBER",
          Some(v.clone()),
          None
        ),
        ShardIteratorType::AtTimestamp(v) => (
          "AT_TIMESTAMP",
          None,
          Some(v.clone())
        ),
        ShardIteratorType::TrimHorizon => (
          "TRIM_HORIZON",
          None,
          None
        ),
        ShardIteratorType::Latest => (
          "LATEST",
          None,
          None
        ),
    };

    let input = GetShardIteratorInput {
      shard_id: self.shard_id.clone(),
      shard_iterator_type: shard_iterator_type.to_string(),
      starting_sequence_number,
      timestamp,
      stream_name: self.stream_name.clone(),
    };

    let res = self.client.get_shard_iterator(input).await?;
    let iterator = res
      .shard_iterator
      .ok_or_else(|| Error::NoShardIterator(self.shard_id.clone()))?;

    self.span.in_scope(|| {
      tracing::info!("iterator = {}", iterator);
    });

    Ok(iterator)
  }
}

#[derive(Debug)]
pub enum Item {
  Chunk(Chunk),
  Terminated,
}

#[derive(Debug)]
pub struct Chunk {
  pub max_sequence_number: String,
  pub millis_behind_latest: Option<i64>,
  pub game_records: BTreeMap<i32, GameChunk>,
}

#[derive(Debug)]
pub struct GameChunk {
  pub approximate_arrival_timestamp: f64,
  pub min_seq_id: u32,
  pub max_seq_id: u32,
  pub records: Vec<GameRecordData>,
}