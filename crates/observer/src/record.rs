use bytes::{Buf, BufMut};
use flo_net::proto::flo_node::Game;
use flo_util::binary::{BinDecode, BinEncode};
use flo_w3gs::protocol::packet::{Header as W3GSHeader, Packet};
use prost::Message;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecordError {
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
pub struct GameRecord {
  pub game_id: i32,
  pub data: GameRecordData,
}

#[derive(Debug, Clone)]
pub enum GameRecordData {
  GameInfo(Game),
  W3GS(Packet),
  StartLag(Vec<i32>),
  StopLag(i32),
  GameEnd,
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum DataTypeId {
  GameInfo = 0,
  W3GS = 1,
  StartLag = 2,
  StopLag = 3,
  GameEnd = 4,
}

impl GameRecordData {
  fn type_id(&self) -> DataTypeId {
    match *self {
      GameRecordData::GameInfo(_) => DataTypeId::GameInfo,
      GameRecordData::W3GS(_) => DataTypeId::W3GS,
      GameRecordData::StartLag(_) => DataTypeId::StartLag,
      GameRecordData::StopLag(_) => DataTypeId::StopLag,
      GameRecordData::GameEnd => DataTypeId::GameEnd,
    }
  }

  fn data_encode_len(&self) -> usize {
    match *self {
      GameRecordData::GameInfo(ref msg) => 2 + msg.encoded_len(),
      GameRecordData::W3GS(ref pkt) => 1 + pkt.payload.len(),
      GameRecordData::StartLag(ref ids) => 1 + 4 * ids.len(),
      GameRecordData::StopLag(_) => 4,
      GameRecordData::GameEnd => 0,
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
      GameRecordData::GameInfo(ref msg) => {
        let len = msg.encoded_len();
        assert!(len <= u16::MAX as usize);
        buf.put_u16(len as u16);
        msg.encode(&mut buf).expect("unbounded buffer")
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
    }
  }

  pub fn decode<T: Buf>(mut buf: T) -> Result<Self, RecordError> {
    if buf.remaining() <= 1 + 2 {
      return Err(RecordError::UnexpectedEndOfBuffer);
    }
    let data_type = match buf.get_u8() {
      0 => DataTypeId::GameInfo,
      1 => DataTypeId::W3GS,
      2 => DataTypeId::StartLag,
      3 => DataTypeId::StopLag,
      4 => DataTypeId::GameEnd,
      other => return Err(RecordError::UnknownDataTypeId(other)),
    };
    Ok(match data_type {
      DataTypeId::GameInfo => {
        if buf.remaining() <= 2 {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let size = buf.get_u16() as usize;
        if buf.remaining() < size {
          return Err(RecordError::UnexpectedEndOfBuffer);
        }
        let mut sub = buf.take(size);
        let game = Game::decode(sub.get_mut()).map_err(RecordError::DecodeGameInfo)?;
        Self::GameInfo(game)
      }
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
    })
  }
}

impl GameRecord {
  pub fn new_game_info(msg: Game) -> Self {
    Self {
      game_id: msg.id,
      data: GameRecordData::GameInfo(msg),
    }
  }

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

  let record = encode_then_decode(&GameRecord::new_game_info(Game {
    id: 1234,
    ..Default::default()
  }));
  assert_eq!(record.game_id, 1234);
  assert_eq!(record.data.type_id(), DataTypeId::GameInfo);
  let inner = match record.data {
    GameRecordData::GameInfo(msg) => msg,
    _ => unreachable!(),
  };
  assert_eq!(inner.id, 1234);

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
}
