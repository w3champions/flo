use crate::broadcast::{BroadcastReceiver, BroadcastSender};
use bytes::{Bytes, BytesMut};
use flo_kinesis::iterator::GameChunk;
use flo_observer::record::GameRecordData;
use std::collections::BTreeMap;

pub const MAX_STREAM_FRAME_SIZE: usize = 8 * 1024;

pub struct GameStreamMap {
  map: BTreeMap<i32, GameStream>,
}

impl GameStreamMap {
  pub fn new() -> Self {
    GameStreamMap {
      map: BTreeMap::new(),
    }
  }

  pub fn subscribe(
    &mut self,
    game_id: i32,
    initial_records: &[GameRecordData],
  ) -> (GameStreamDataSnapshot, BroadcastReceiver<GameStreamEvent>) {
    use std::collections::btree_map::Entry;
    match self.map.entry(game_id) {
      Entry::Vacant(e) => {
        let (stream, rx) = GameStream::new(game_id, initial_records);
        let snapshot = stream.make_data_snapshot();
        e.insert(stream);
        (snapshot, rx)
      }
      Entry::Occupied(e) => {
        let stream = e.get();
        let snapshot = stream.make_data_snapshot();
        (snapshot, stream.tx.subscribe())
      }
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
  frames: Vec<Bytes>,
  tx: BroadcastSender<GameStreamEvent>,
  ended: bool,
}

impl GameStream {
  pub fn new(
    game_id: i32,
    initial_records: &[GameRecordData],
  ) -> (Self, BroadcastReceiver<GameStreamEvent>) {
    let (tx, rx) = BroadcastSender::channel();
    let mut stream = Self {
      game_id,
      frames: vec![],
      tx,
      ended: false,
    };
    if stream.encode_records(&initial_records).has_game_end {
      stream.ended = true;
    }
    (stream, rx)
  }

  fn is_closed(&self) -> bool {
    self.tx.is_closed()
  }

  fn make_data_snapshot(&self) -> GameStreamDataSnapshot {
    GameStreamDataSnapshot {
      frames: self.frames.clone(),
      ended: self.ended,
    }
  }

  fn dispatch_records(&mut self, records: &[GameRecordData]) -> bool {
    if records.is_empty() {
      return self.tx.is_closed();
    };
    let encoded = self.encode_records(records);
    if !encoded.is_empty() {
      let event = GameStreamEvent::Chunk {
        frames: encoded.frames.to_vec(),
        ended: encoded.has_game_end,
      };
      self.tx.send(event)
    } else {
      !self.tx.is_closed()
    }
  }

  fn encode_records(&mut self, records: &[GameRecordData]) -> EncodedRecords {
    let mut buf = BytesMut::with_capacity(MAX_STREAM_FRAME_SIZE);
    let start_frames_len = self.frames.len();
    for record in records {
      if let GameRecordData::W3GS(p) = record {
        if p.type_id() == flo_w3gs::protocol::constants::PacketTypeId::ChatFromHost {
          continue;
        }
      }

      if buf.len() + record.encode_len() > MAX_STREAM_FRAME_SIZE {
        let frame = buf.freeze();
        self.frames.push(frame);
        buf = BytesMut::with_capacity(MAX_STREAM_FRAME_SIZE);
      }

      record.encode(&mut buf);

      if buf.len() > MAX_STREAM_FRAME_SIZE {
        tracing::warn!(
          game_id = self.game_id,
          "oversized record: {:?} {}",
          record.type_id(),
          buf.len()
        );
      }
    }

    if buf.len() > 0 {
      self.frames.push(buf.freeze());
    }

    let has_game_end = if let Some(GameRecordData::GameEnd) = records.last() {
      true
    } else {
      false
    };

    let encoded = if self.frames.len() != start_frames_len {
      EncodedRecords {
        frames: &self.frames[start_frames_len..],
        has_game_end,
      }
    } else {
      EncodedRecords {
        frames: &[],
        has_game_end,
      }
    };

    encoded
  }
}

struct EncodedRecords<'a> {
  frames: &'a [Bytes],
  has_game_end: bool,
}

impl<'a> EncodedRecords<'a> {
  fn is_empty(&self) -> bool {
    self.frames.len() == 0
  }
}

#[derive(Debug, Clone)]
pub enum GameStreamEvent {
  Chunk { frames: Vec<Bytes>, ended: bool },
}

pub struct GameStreamDataSnapshot {
  pub frames: Vec<Bytes>,
  pub ended: bool,
}

#[tokio::test]
async fn test_game_stream() {
  dotenv::dotenv().unwrap();
  flo_log_subscriber::init();

  use bytes::Buf;
  use tokio_stream::StreamExt;
  let initial: Vec<_> = (0..1000_u32)
    .map(|v| GameRecordData::TickChecksum {
      tick: v,
      checksum: v,
    })
    .collect();
  let append_chunks: Vec<Vec<_>> = (1..10)
    .map(|i| {
      (0..8000)
        .map(|v| GameRecordData::TickChecksum {
          tick: i * 1000 + v,
          checksum: i * 1000 + v,
        })
        .collect()
    })
    .collect();
  let all: Vec<_> = initial
    .clone()
    .into_iter()
    .chain(append_chunks.clone().into_iter().flatten())
    .collect();
  let (mut stream, rx) = GameStream::new(1, &initial);
  let snapshot = stream.make_data_snapshot();

  tracing::debug!("initial = {}, all = {}", initial.len(), all.len());

  fn parse_records(mut buf: &[u8], items: &mut Vec<GameRecordData>) {
    while buf.remaining() > 0 {
      items.push(GameRecordData::decode(&mut buf).unwrap());
    }
  }

  let send = tokio::spawn(async move {
    for item in append_chunks {
      assert!(stream.dispatch_records(&item));
    }
  });

  let recv = tokio::spawn(async move {
    let mut records = vec![];
    for frame in snapshot.frames {
      parse_records(&frame, &mut records);
    }
    let events: Vec<_> = rx.into_stream().collect().await;
    for event in events {
      let GameStreamEvent::Chunk { frames, .. } = event;
      for frame in frames {
        parse_records(&frame, &mut records);
      }
    }
    records
  });

  send.await.unwrap();

  let got = recv.await.unwrap();

  assert_eq!(got.len(), all.len());
  for (i, a) in got.into_iter().enumerate() {
    let a = if let GameRecordData::TickChecksum { tick, .. } = a {
      tick
    } else {
      unreachable!()
    };
    let b = if let GameRecordData::TickChecksum { tick, .. } = &all[i] {
      *tick
    } else {
      unreachable!()
    };
    assert_eq!(a, b)
  }
}
