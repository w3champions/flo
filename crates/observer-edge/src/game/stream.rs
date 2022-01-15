use bytes::{Bytes, BytesMut};
use flo_kinesis::iterator::GameChunk;
use flo_observer::record::GameRecordData;
use std::collections::BTreeMap;

use crate::broadcast::{BroadcastSender, BroadcastReceiver};

pub struct GameStreamMap {
  map: BTreeMap<i32, GameStream>,
}

impl GameStreamMap {
  pub fn new() -> Self {
    GameStreamMap {
      map: BTreeMap::new(),
    }
  }

  pub fn subscribe(&mut self, game_id: i32, initial_records: &[GameRecordData]) -> (GameStreamDataSnapshot, BroadcastReceiver<GameStreamEvent>) {
    use std::collections::btree_map::Entry;
    match self.map.entry(game_id) {
        Entry::Vacant(e) => {
          let (stream, rx) = GameStream::new(game_id, initial_records);
          let snapshot = stream.make_data_snapshot();
          e.insert(stream);
          (snapshot, rx)
        },
        Entry::Occupied(e) => {
          let stream = e.get();
          let snapshot = stream.make_data_snapshot();
          (snapshot, stream.tx.subscribe())
        },
    }
  }

  pub fn dispatch_game_records(&mut self, game_id: i32, chunk: &GameChunk) {
    let mut should_remove = false;
    if let Some(stream) = self.map.get_mut(&game_id) {
      should_remove = !stream.dispatch_records(&chunk.records);
    }
    if should_remove {
      tracing::debug!(game_id, "sender closed");
      self.map.remove(&game_id);
    }
  }

  // Needs to be called regularly to clean up streams that have expired, that is streams have no active receiver.
  pub fn remove_all_disconnected(&mut self) {
    let map = std::mem::replace(&mut self.map, BTreeMap::new());
    for (k, v) in map {
      if !v.is_closed() {
        self.map.insert(k, v);
      }
    }
  }
}

pub struct GameStream {
  game_id: i32,
  buf: BytesMut,
  tx: BroadcastSender<GameStreamEvent>,
}

impl GameStream {
  pub fn new(game_id: i32, initial_records: &[GameRecordData]) -> (Self, BroadcastReceiver<GameStreamEvent>) {
    let mut buf = BytesMut::new();
    for record in initial_records {
      record.encode(&mut buf);
    }
    let (tx, rx) = BroadcastSender::channel();
    (Self { game_id, buf, tx }, rx)
  }

  fn is_closed(&self) -> bool {
    self.tx.is_closed()
  }

  fn make_data_snapshot(&self) -> GameStreamDataSnapshot {
    GameStreamDataSnapshot {
      data: self.buf.clone().freeze(),
    }
  }

  fn dispatch_records(&mut self, records: &[GameRecordData]) -> bool {
    if records.is_empty() {
      return self.tx.is_closed()
    };
    let pos = self.buf.len();
    for record in records {
      record.encode(&mut self.buf);
    }
    let len = self.buf.len() - pos;
    if len > 0 {
      let data = self.buf.clone().split_off(len).freeze();
      self.tx.send(GameStreamEvent::Chunk {
        data
      })
    } else {
      !self.tx.is_closed()
    }
  }
}

#[derive(Debug, Clone)]
pub enum GameStreamEvent {
  Chunk {
    data: Bytes,
  },
}

pub struct GameStreamDataSnapshot {
  pub data: Bytes,
}