use crate::error::Result;
use backoff::backoff::Backoff;
use bytes::{BufMut, Bytes, BytesMut};
use flo_observer::{record::GameRecord, record::RTTStats, KINESIS_CLIENT};
use flo_w3gs::packet::Packet;
use parking_lot::Mutex;
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
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
    let bm = BufferMap::new();

    tokio::spawn(Handler::new(ct.clone(), rx, bm.clone()).run());
    tokio::spawn(Pusher::new(ct.clone(), bm.clone()).run());

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

  pub fn push_rtt_stat(&self, game_id: i32, stats: RTTStats) {
    self.push_record(GameRecord::new_rtt_stats(game_id, stats))
  }

  fn push_record(&self, record: GameRecord) {
    if self.broken.get() {
      return;
    }
    self
      .tx
      .try_send(Cmd::AddRecord(record))
      .err()
      .map(|_| {
        tracing::error!("observer pushing disabled.");
        self.broken.set(true)
      });
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

#[derive(Clone)]
struct BufferMap {
  map: Arc<Mutex<BTreeMap<i32, GameBuffer>>>,
  notify: Arc<Notify>,
}

impl BufferMap {
  fn new() -> Self {
    Self {
      map: Arc::new(Mutex::new(BTreeMap::new())),
      notify: Arc::new(Notify::new()),
    }
  }

  fn add_record(&self, record: GameRecord) {
    self
      .map
      .lock()
      .entry(record.game_id)
      .or_insert_with(|| GameBuffer::new())
      .push(record);
    self.notify.notify_one();
  }

  fn mark_remove(&self, game_id: i32) {
    let mut g = self.map.lock();
    if let Some(buf) = g.get_mut(&game_id) {
      buf.should_remove = true;
    }
  }

  fn split_chunks(&self, time: Instant) -> Vec<(i32, Bytes)> {
    let mut map = self.map.lock();
    let mut remove_ids = None;
    let mut expired_ids = None;
    let items = map
      .iter_mut()
      .filter_map(|(game_id, buf)| {
        if buf.should_remove {
          remove_ids.get_or_insert_with(|| vec![]).push(*game_id);
        }

        if time.saturating_duration_since(buf.last_update) > BUFFER_TIMEOUT {
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
        map.remove(&id);
      }
    }

    if let Some(ids) = expired_ids {
      for id in ids {
        map.remove(&id);
        tracing::warn!("obs: game buffer expired: {}", id);
      }
    }

    items
  }
}

struct Handler {
  ct: CancellationToken,
  rx: Receiver<Cmd>,
  buffer_map: BufferMap,
}

impl Handler {
  fn new(ct: CancellationToken, rx: Receiver<Cmd>, buffer_map: BufferMap) -> Self {
    Self { ct, rx, buffer_map }
  }

  async fn run(mut self) {
    loop {
      tokio::select! {
        _ = self.ct.cancelled() => break,
        Some(cmd) = self.rx.recv() => {
          self.handle_cmd(cmd).await;
        },
      }
    }
  }

  async fn handle_cmd(&mut self, cmd: Cmd) {
    match cmd {
      Cmd::AddRecord(record) => {
        self.buffer_map.add_record(record);
      }
      Cmd::RemoveGame { game_id } => {
        self.buffer_map.mark_remove(game_id);
      }
    }
  }
}

struct Pusher {
  ct: CancellationToken,
  last_sequence_number: Option<String>,
  buffer_map: BufferMap,
}

impl Pusher {
  fn new(ct: CancellationToken, buffer_map: BufferMap) -> Self {
    Self {
      ct,
      last_sequence_number: None,
      buffer_map,
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
        _ = &mut flush_timeout, if active => {
          let mut reset_backoff = false;
          loop {
            match self.flush().await {
              Ok(next) => {
                if let Some(next) = next {
                  flush_timeout.as_mut().reset(next);
                } else {
                  active = false;
                }
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
        },
        _ = self.buffer_map.notify.notified(), if !active => {
          active = true;
          flush_timeout.as_mut().reset(Instant::now());
        }
      }
    }
  }

  // returns next flush instant
  async fn flush(&mut self) -> Result<Option<Instant>> {
    use rusoto_core::RusotoError;
    use rusoto_kinesis::{Kinesis, PutRecordError, PutRecordInput};

    let start = Instant::now();
    let items: Vec<_> = self
      .buffer_map
      .split_chunks(start);

    if items.is_empty() {
      return Ok(None);
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
    Ok(Some(
      now
        + crate::constants::OBS_FLUSH_INTERVAL
          .checked_sub(now - start)
          .unwrap_or(Duration::from_secs(0)),
    ))
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
    self.split_chunks.is_empty() && self.data.is_empty()
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
