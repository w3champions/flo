use bytes::BufMut;
use flo_w3gs::protocol::packet::Packet;

#[derive(Debug)]
pub struct DataRecord {
  pub game_records: Vec<GameRecord>,
}

#[derive(Debug)]
pub struct GameRecord {
  pub game_id: i32,
  pub data: GameRecordData,
}

#[derive(Debug)]
pub enum GameRecordData {
  W3GS(Packet),
}

#[derive(Debug)]
#[repr(u8)]
pub enum DataTypeId {
  W3GS = 1,
}

impl GameRecordData {
  fn type_id(&self) -> DataTypeId {
    match *self {
      GameRecordData::W3GS(_) => DataTypeId::W3GS,
    }
  }

  fn len(&self) -> usize {
    match *self {
      GameRecordData::W3GS(ref pkt) => 1 + pkt.payload.len(),
    }
  }

  fn encode<T: BufMut>(&self, mut buf: T) {
    buf.put_u8(self.type_id() as u8);
    buf.put_u16(self.len() as u16);
    match *self {
      GameRecordData::W3GS(ref pkt) => buf.put(pkt.payload.as_ref()),
    }
  }
}

impl GameRecord {
  pub fn new_w3gs(game_id: i32, pkt: Packet) -> Self {
    Self {
      game_id,
      data: GameRecordData::W3GS(pkt),
    }
  }

  pub fn encode_len(&self) -> usize {
    4 + 4 + 2 + self.data.len()
  }

  pub fn encode<T: BufMut>(&self, mut buf: T) {
    buf.put_i32(self.game_id);
    self.data.encode(&mut buf);
  }
}
