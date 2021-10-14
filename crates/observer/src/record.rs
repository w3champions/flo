use bytes::{Buf, BufMut};
use flo_util::binary::{BinDecode, BinEncode};
use flo_util::{BinDecode, BinEncode};
use flo_w3gs::protocol::packet::{Header as W3GSHeader, Packet};
use std::convert::TryFrom;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecordError {
  #[error("unknown record source: {0}")]
  UnknownRecordSourceU32(u32),
  #[error("unknown record source: {0}")]
  UnknownRecordSource(String),
  #[error("unknown data type id: {0}")]
  UnknownDataTypeId(u8),
  #[error("unexpected end of buffer")]
  UnexpectedEndOfBuffer,
  #[error("decode game info: {0}")]
  DecodeGameInfo(prost::DecodeError),
  #[error("decode w3gs header: {0}")]
  DecodeW3GSHeader(flo_util::error::BinDecodeError),
  #[error("decode rtt stats record: {0}")]
  DecodeRTTStatsRecord(flo_util::error::BinDecodeError),
  #[error("decode w3gs: {0}")]
  DecodeW3GS(flo_w3gs::error::Error),
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u32)]
pub enum ObserverRecordSource {
  Test = 0xDA881C,
  PTR = 0xEBCC6D,
  Production = 0x153EA0,
}

impl TryFrom<u32> for ObserverRecordSource {
  type Error = RecordError;

  fn try_from(value: u32) -> Result<Self, Self::Error> {
    Ok(match value {
      0xDA881C => Self::Test,
      0xEBCC6D => Self::PTR,
      0x153EA0 => Self::Production,
      other => return Err(RecordError::UnknownRecordSourceU32(other)),
    })
  }
}

impl FromStr for ObserverRecordSource {
  type Err = RecordError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let v = match s {
      "TEST" => ObserverRecordSource::Test,
      "PTR" => ObserverRecordSource::PTR,
      "PROD" => ObserverRecordSource::Production,
      other => return Err(RecordError::UnknownRecordSource(other.to_string())),
    };
    Ok(v)
  }
}

#[derive(Debug, Clone)]
pub struct KMSRecord {
  pub source: ObserverRecordSource,
  pub records: Vec<(u32, GameRecord)>,
}

impl KMSRecord {
  pub fn peek_source(mut bytes: &[u8]) -> Result<ObserverRecordSource, RecordError> {
    if bytes.len() < 4 {
      return Err(RecordError::UnexpectedEndOfBuffer);
    }
    ObserverRecordSource::try_from(bytes.get_u32())
  }

  pub fn decode<T: Buf>(mut buf: T) -> Result<Self, RecordError> {
    if buf.remaining() < 4 {
      return Err(RecordError::UnexpectedEndOfBuffer);
    }
    let source = ObserverRecordSource::try_from(buf.get_u32())?;
    let mut records = vec![];
    while buf.has_remaining() {
      if buf.remaining() <= 4 {
        return Err(RecordError::UnexpectedEndOfBuffer);
      }
      let seq_id = buf.get_u32();
      let record = GameRecord::decode(&mut buf)?;
      records.push((seq_id, record));
    }
    Ok(Self { source, records })
  }
}

#[derive(Debug, Clone)]
pub struct GameRecord {
  pub game_id: i32,
  pub data: GameRecordData,
}

#[derive(Debug, Clone)]
pub enum GameRecordData {
  W3GS(Packet),
  StartLag(Vec<i32>),
  StopLag(i32),
  GameEnd,
  TickChecksum { tick: u32, checksum: u32 },
  RTTStats(RTTStats),
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RTTStats {
  pub time: u32,
  items_len: u8,
  #[bin(repeat = "items_len")]
  pub items: Vec<RTTStatsItem>,
}

impl RTTStats {
  pub fn new(time: u32, items: impl Iterator<Item = RTTStatsItem>) -> Self {
    let items: Vec<_> = items.into_iter().take(u8::MAX as usize).collect();
    Self {
      time,
      items_len: items.len() as _,
      items,
    }
  }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RTTStatsItem {
  pub player_id: i32,
  pub ticks: u16,
  pub min: u16,
  pub max: u16,
  pub avg: f32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum DataTypeId {
  W3GS = 1,
  StartLag = 2,
  StopLag = 3,
  GameEnd = 4,
  TickChecksum = 5,
  RTTStat = 6,
}

impl GameRecordData {
  pub fn type_id(&self) -> DataTypeId {
    match *self {
      GameRecordData::W3GS(_) => DataTypeId::W3GS,
      GameRecordData::StartLag(_) => DataTypeId::StartLag,
      GameRecordData::StopLag(_) => DataTypeId::StopLag,
      GameRecordData::GameEnd => DataTypeId::GameEnd,
      GameRecordData::TickChecksum { .. } => DataTypeId::TickChecksum,
      GameRecordData::RTTStats { .. } => DataTypeId::RTTStat,
    }
  }

  fn data_encode_len(&self) -> usize {
    match *self {
      GameRecordData::W3GS(ref pkt) => 1 + pkt.payload.len(),
      GameRecordData::StartLag(ref ids) => 1 + 4 * ids.len(),
      GameRecordData::StopLag(_) => 4,
      GameRecordData::GameEnd => 0,
      GameRecordData::TickChecksum { .. } => 4 + 4,
      GameRecordData::RTTStats(ref data) => 4 + 1 + (data.items.len() * RTTStatsItem::MIN_SIZE),
    }
  }

  pub fn encode_len(&self) -> usize {
    1 + self.data_encode_len()
  }

  pub fn encode<T: BufMut>(&self, mut buf: T) {
    buf.put_u8(self.type_id() as u8);
    match *self {
      GameRecordData::W3GS(ref pkt) => {
        pkt.header.encode(&mut buf);
        buf.put(pkt.payload.as_ref())
      }
      GameRecordData::StartLag(ref ids) => {
        assert!(ids.len() < u8::MAX as usize);
        buf.put_u8(ids.len() as u8);
        for id in ids {
          buf.put_i32(*id);
        }
      }
      GameRecordData::StopLag(id) => {
        buf.put_i32(id);
      }
      GameRecordData::GameEnd => {}
      GameRecordData::TickChecksum { tick, checksum } => {
        buf.put_u32(tick);
        buf.put_u32(checksum);
      }
      GameRecordData::RTTStats(ref data) => {
        data.encode(&mut buf);
      }
    }
  }

  pub fn decode<T: Buf>(mut buf: T) -> Result<Self, RecordError> {
    if buf.remaining() < 1 {
      return Err(RecordError::UnexpectedEndOfBuffer);
    }
    let data_type = match buf.get_u8() {
      1 => DataTypeId::W3GS,
      2 => DataTypeId::StartLag,
      3 => DataTypeId::StopLag,
      4 => DataTypeId::GameEnd,
      5 => DataTypeId::TickChecksum,
      6 => DataTypeId::RTTStat,
      other => return Err(RecordError::UnknownDataTypeId(other)),
    };
    Ok(match data_type {
      DataTypeId::W3GS => {
        if buf.remaining() < 1 {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let header = W3GSHeader::decode(&mut buf).map_err(RecordError::DecodeW3GSHeader)?;
        let payload_len = header.get_payload_len().map_err(RecordError::DecodeW3GS)?;
        if buf.remaining() < payload_len {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let payload = buf.copy_to_bytes(payload_len);
        Self::W3GS(Packet { header, payload })
      }
      DataTypeId::StartLag => {
        if buf.remaining() < 1 {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let len = buf.get_u8() as usize;
        if buf.remaining() < 4 * len {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
          items.push(buf.get_i32());
        }
        Self::StartLag(items)
      }
      DataTypeId::StopLag => {
        if buf.remaining() < 4 {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        Self::StopLag(buf.get_i32())
      }
      DataTypeId::GameEnd => Self::GameEnd,
      DataTypeId::TickChecksum => {
        if buf.remaining() < 4 + 4 {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        Self::TickChecksum {
          tick: buf.get_u32(),
          checksum: buf.get_u32(),
        }
      }
      DataTypeId::RTTStat => {
        Self::RTTStats(RTTStats::decode(&mut buf).map_err(RecordError::DecodeRTTStatsRecord)?)
      }
    })
  }
}

impl GameRecord {
  pub fn new_w3gs(game_id: i32, pkt: Packet) -> Self {
    Self {
      game_id,
      data: GameRecordData::W3GS(pkt),
    }
  }

  pub fn new_start_lag(game_id: i32, player_ids: Vec<i32>) -> Self {
    Self {
      game_id,
      data: GameRecordData::StartLag(player_ids),
    }
  }

  pub fn new_end_lag(game_id: i32, player_id: i32) -> Self {
    Self {
      game_id,
      data: GameRecordData::StopLag(player_id),
    }
  }

  pub fn new_game_end(game_id: i32) -> Self {
    Self {
      game_id,
      data: GameRecordData::GameEnd,
    }
  }

  pub fn new_tick_checksum(game_id: i32, tick: u32, checksum: u32) -> Self {
    Self {
      game_id,
      data: GameRecordData::TickChecksum { tick, checksum },
    }
  }

  pub fn new_rtt_stats(game_id: i32, stats: RTTStats) -> Self {
    Self {
      game_id,
      data: GameRecordData::RTTStats(stats),
    }
  }

  pub fn encode_len(&self) -> usize {
    4 + self.data.encode_len()
  }

  pub fn encode<T: BufMut>(&self, mut buf: T) {
    buf.put_i32(self.game_id);
    self.data.encode(&mut buf);
  }

  pub fn decode<T: Buf>(mut buf: T) -> Result<Self, RecordError> {
    if buf.remaining() < 4 {
      return Err(RecordError::UnexpectedEndOfBuffer);
    }
    let game_id = buf.get_i32();
    Ok(Self {
      game_id,
      data: GameRecordData::decode(buf)?,
    })
  }
}

#[test]
fn test_encode_decode() {
  use bytes::BytesMut;
  use flo_w3gs::protocol::constants::PacketTypeId;
  use flo_w3gs::protocol::leave::{LeaveAck, LeaveReason, LeaveReq};

  fn encode_then_decode(record: &GameRecord) -> GameRecord {
    let mut buf = BytesMut::new();
    record.encode(&mut buf);
    let mut buf = buf.freeze();
    let r = GameRecord::decode(&mut buf).unwrap();
    assert!(!buf.has_remaining());
    r
  }

  let record = encode_then_decode(&GameRecord::new_w3gs(
    1234,
    Packet::simple(LeaveReq::new(LeaveReason::LeaveDisconnect)).unwrap(),
  ));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::W3GS);
  let inner = match record.data {
    GameRecordData::W3GS(pkt) => pkt,
    _ => unreachable!(),
  };
  assert_eq!(inner.type_id(), PacketTypeId::LeaveReq);
  let payload: LeaveReq = inner.decode_simple().unwrap();
  assert_eq!(payload.reason(), LeaveReason::LeaveDisconnect);

  let record = encode_then_decode(&GameRecord::new_w3gs(
    1234,
    Packet::simple(LeaveAck).unwrap(),
  ));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::W3GS);
  let inner = match record.data {
    GameRecordData::W3GS(pkt) => pkt,
    _ => unreachable!(),
  };
  assert_eq!(inner.type_id(), PacketTypeId::LeaveAck);

  let record = encode_then_decode(&GameRecord::new_start_lag(1234, vec![1, 2, 3, 4]));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::StartLag);
  let inner = match record.data {
    GameRecordData::StartLag(inner) => inner,
    _ => unreachable!(),
  };
  assert_eq!(inner, vec![1, 2, 3, 4]);

  let record = encode_then_decode(&GameRecord::new_end_lag(1234, 5678));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::StopLag);
  let inner = match record.data {
    GameRecordData::StopLag(inner) => inner,
    _ => unreachable!(),
  };
  assert_eq!(inner, 5678);

  let record = encode_then_decode(&GameRecord::new_rtt_stats(
    1234,
    RTTStats {
      time: 3333,
      items_len: 3,
      items: vec![
        RTTStatsItem {
          player_id: 1,
          ticks: 1,
          min: 1,
          max: 1,
          avg: 1.0,
        },
        RTTStatsItem {
          player_id: 2,
          ticks: 2,
          min: 2,
          max: 2,
          avg: 2.0,
        },
        RTTStatsItem {
          player_id: 3,
          ticks: 3,
          min: 3,
          max: 3,
          avg: 3.0,
        },
      ],
    },
  ));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::RTTStat);
  let inner = match record.data {
    GameRecordData::RTTStats(inner) => inner,
    _ => unreachable!(),
  };
  assert_eq!(inner.items_len, 3);
  for i in 1..=3 {
    let RTTStatsItem {
      player_id,
      ticks,
      min,
      max,
      avg,
    } = inner.items[i - 1];
    assert_eq!(player_id, i as i32);
    assert_eq!(ticks, i as u16);
    assert_eq!(min, i as u16);
    assert_eq!(max, i as u16);
    assert_eq!(avg, i as f32);
  }
}
