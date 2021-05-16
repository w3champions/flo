use crate::error::Result;
use backoff::backoff::Backoff;
use bytes::{BufMut, Bytes, BytesMut};
use flo_observer::{record::GameRecord, KINESIS_CLIENT};
use flo_w3gs::packet::Packet;
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::{sleep, Instant};
use tokio_util::sync::CancellationToken;

const BUFFER_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub struct ObserverPublisher {
  ct: CancellationToken,
  tx: Sender<Cmd>,
}

impl Drop for ObserverPublisher {
  fn drop(&mut self) {
    self.ct.cancel()
  }
}

impl ObserverPublisher {
  pub fn new() -> Self {
    let (tx, rx) = channel(crate::constants::OBS_CHANNEL_SIZE);
    let ct = CancellationToken::new();

    tokio::spawn(Worker::new(ct.clone(), rx).run());

    Self { ct, tx }
  }

  pub fn handle(&self) -> ObserverPublisherHandle {
    ObserverPublisherHandle {
      broken: Cell::new(false),
      tx: self.tx.clone(),
    }
  }
}

#[derive(Debug, Clone)]
pub struct ObserverPublisherHandle {
  broken: Cell<bool>,
  tx: Sender<Cmd>,
}

impl ObserverPublisherHandle {
  pub fn push_w3gs(&self, game_id: i32, packet: Packet) {
    self.push_record(GameRecord::new_w3gs(game_id, packet))
  }

  pub fn push_start_lag(&self, game_id: i32, player_ids: Vec<i32>) {
    self.push_record(GameRecord::new_start_lag(game_id, player_ids))
  }

  pub fn push_end_lag(&self, game_id: i32, player_id: i32) {
    self.push_record(GameRecord::new_end_lag(game_id, player_id))
  }

  pub fn push_game_end(&self, game_id: i32) {
    self.push_record(GameRecord::new_game_end(game_id))
  }

  pub fn push_tick_checksum(&self, game_id: i32, tick: u32, checksum: u32) {
    self.push_record(GameRecord::new_tick_checksum(game_id, tick, checksum))
  }

  fn push_record(&self, record: GameRecord) {
    if self.broken.get() {
      return;
    }
    self
      .tx
      .try_send(Cmd::AddRecord(record))
      .err()
      .map(|_| self.broken.set(true));
  }

  pub fn remove_game(&self, game_id: i32) {
    if self.broken.get() {
      return;
    }

    self
      .tx
      .try_send(Cmd::RemoveGame { game_id })
      .err()
      .map(|_| self.broken.set(true));
  }
}

enum Cmd {
  AddRecord(GameRecord),
  RemoveGame { game_id: i32 },
}

struct Worker {
  ct: CancellationToken,
  rx: Receiver<Cmd>,
  last_sequence_number: Option<String>,
  buffer_map: BTreeMap<i32, GameBuffer>,
}

impl Worker {
  fn new(ct: CancellationToken, rx: Receiver<Cmd>) -> Self {
    Self {
      ct,
      rx,
      last_sequence_number: None,
      buffer_map: BTreeMap::new(),
    }
  }

  async fn run(mut self) {
    use backoff::ExponentialBackoff;

    let mut active = false;
    let mut flush_backoff = ExponentialBackoff {
      max_elapsed_time: None,
      ..Default::default()
    };

    let flush_timeout = sleep(Duration::from_secs(0));
    tokio::pin!(flush_timeout);

    loop {
      tokio::select! {
        _ = self.ct.cancelled() => break,
        Some(cmd) = self.rx.recv() => {
          if !active {
            active = true;
            flush_timeout.as_mut().reset(Instant::now() + crate::constants::OBS_FLUSH_INTERVAL);
          }
          self.handle_cmd(cmd).await;
        },
        _ = &mut flush_timeout, if active => {
          let mut reset_backoff = false;
          loop {
            match self.flush().await {
              Ok(next) => {
                flush_timeout.as_mut().reset(next);
                break;
              }
              Err(err) => {
                tracing::error!("obs: flush: {}", err);
                reset_backoff = true;
                if let Some(duration) = flush_backoff.next_backoff() {
                  sleep(duration).await;
                } else {
                  sleep(flush_backoff.max_interval).await;
                }
              }
            }
          }
          if reset_backoff {
            flush_backoff.reset();
          }
        }
      }
    }
  }

  async fn handle_cmd(&mut self, cmd: Cmd) {
    match cmd {
      Cmd::AddRecord(record) => {
        self
          .buffer_map
          .entry(record.game_id)
          .or_insert_with(|| GameBuffer::new())
          .push(record);
      }
      Cmd::RemoveGame { game_id } => {
        if let Some(buf) = self.buffer_map.get_mut(&game_id) {
          buf.should_remove = true;
        }
      }
    }
  }

  // returns next flush instant
  async fn flush(&mut self) -> Result<Instant> {
    use rusoto_core::RusotoError;
    use rusoto_kinesis::{Kinesis, PutRecordError, PutRecordInput};

    let mut remove_ids = None;
    let mut expired_ids = None;
    let start = Instant::now();
    let items: Vec<_> = self
      .buffer_map
      .iter_mut()
      .filter_map(|(game_id, buf)| {
        if buf.should_remove {
          remove_ids.get_or_insert_with(|| vec![]).push(*game_id);
        }

        if start.saturating_duration_since(buf.last_update) > BUFFER_TIMEOUT {
          expired_ids.get_or_insert_with(|| vec![]).push(*game_id);
          return None;
        }

        if buf.is_empty() {
          return None;
        }

        Some((*game_id, buf.split_chunk()))
      })
      .collect();

    if let Some(ids) = remove_ids {
      for id in ids {
        self.buffer_map.remove(&id);
      }
    }

    if let Some(ids) = expired_ids {
      for id in ids {
        self.buffer_map.remove(&id);
        tracing::warn!("obs: game buffer expired: {}", id);
      }
    }

    if items.is_empty() {
      return Ok(start + crate::constants::OBS_FLUSH_INTERVAL);
    }

    for (game_id, data) in items {
      let input = PutRecordInput {
        data,
        explicit_hash_key: None,
        partition_key: game_id.to_string(),
        sequence_number_for_ordering: self.last_sequence_number.clone(),
        stream_name: flo_observer::KINESIS_STREAM_NAME.clone(),
      };

      loop {
        match KINESIS_CLIENT.put_record(input.clone()).await {
          Ok(output) => {
            self.last_sequence_number.replace(output.sequence_number);
            break;
          }
          Err(RusotoError::Service(err)) => match err {
            PutRecordError::KMSThrottling(msg) => {
              tracing::error!("obs: KMSThrottling: {}", msg);
            }
            PutRecordError::ProvisionedThroughputExceeded(msg) => {
              tracing::error!("obs: ProvisionedThroughputExceeded: {}", msg);
            }
            _ => return Err(RusotoError::Service(err).into()),
          },
          Err(err) => match err {
            RusotoError::Credentials(_)
            | RusotoError::Validation(_)
            | RusotoError::ParseError(_) => {
              return Err(err.into());
            }
            other => {
              tracing::error!("obs: {}", other);
            }
          },
        }

        sleep(Duration::from_secs(1)).await;
      }
    }

    let now = Instant::now();
    Ok(
      now
        + crate::constants::OBS_FLUSH_INTERVAL
          .checked_sub(now - start)
          .unwrap_or(Duration::from_secs(0)),
    )
  }
}

struct GameBuffer {
  seq_id: u32,
  data: BytesMut,
  split_chunks: VecDeque<Bytes>,
  last_update: Instant,
  should_remove: bool,
}

impl GameBuffer {
  pub fn new() -> Self {
    GameBuffer {
      seq_id: 0,
      data: BytesMut::new(),
      split_chunks: VecDeque::new(),
      last_update: Instant::now(),
      should_remove: false,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.data.is_empty()
  }

  // [source: u32] [[seq_id: u32] [data]]
  pub fn push(&mut self, record: GameRecord) {
    let next_len = self.data.len() + record.encode_len();
    if next_len > crate::constants::OBS_MAX_CHUNK_SIZE {
      self.split_chunks.push_back(self.data.split().freeze());
    }

    if self.data.is_empty() {
      self.data.put_u32(*crate::constants::OBS_SOURCE as u32);
    }

    self.data.put_u32(self.seq_id);
    record.encode(&mut self.data);
    self.seq_id = self.seq_id.saturating_add(1);

    assert!(self.data.len() <= crate::constants::OBS_MAX_CHUNK_SIZE);
    self.last_update = Instant::now();
  }

  pub fn split_chunk(&mut self) -> Bytes {
    if let Some(chunk) = self.split_chunks.pop_front() {
      return chunk;
    }
    self.data.split().freeze()
  }
}
