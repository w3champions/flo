use bytes::buf::Chain;
use std::io::prelude::*;

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};
pub use flo_w3gs::action::PlayerAction;
pub use flo_w3gs::constants::{GameFlags, LeaveReason, RacePref};
pub use flo_w3gs::desync::Desync;
pub use flo_w3gs::game::GameSettings;
pub use flo_w3gs::packet::ProtoBufPayload;
pub use flo_w3gs::protocol::chat::ChatMessage;
pub use flo_w3gs::slot::SlotInfo;

use crate::block::{Block, Blocks};
use crate::constants::RecordTypeId;

#[derive(Debug)]
pub struct RecordIter<R> {
  blocks: Blocks<R>,
  empty: Bytes,
  state: State,
}

impl<R> RecordIter<R> {
  pub(crate) fn new(blocks: Blocks<R>) -> Self {
    Self {
      blocks,
      empty: Bytes::new(),
      state: State::Initial,
    }
  }
}

#[derive(Debug)]
enum State {
  Initial,
  DecodingBlock(Block, Chain<Bytes, Bytes>),
  BlockDone,
  Done,
}

impl<R> Iterator for RecordIter<R>
where
  R: Read,
{
  type Item = Result<Record, BinDecodeError>;

  fn next(&mut self) -> Option<Self::Item> {
    let (item, next_state) = match std::mem::replace(&mut self.state, State::Done) {
      State::Initial => extract_next_block_first_record(self.empty.clone(), &mut self.blocks),
      State::DecodingBlock(mut block, mut buf) => match buf.peek_u8() {
        Some(n) if n != 0 => match extract_next_record(&mut block, &mut buf) {
          Ok(NextRecord::Record(rec)) => (Some(Ok(rec)), State::DecodingBlock(block, buf)),
          Ok(NextRecord::Partial(tail)) => extract_next_block_first_record(tail, &mut self.blocks),
          Err(err) => (Some(Err(err)), State::Done),
        },
        _ => extract_next_block_first_record(self.empty.clone(), &mut self.blocks),
      },
      State::BlockDone => extract_next_block_first_record(self.empty.clone(), &mut self.blocks),
      State::Done => (None, State::Done),
    };

    self.state = next_state;

    item
  }
}

fn extract_next_block_first_record<R>(
  tail: Bytes,
  blocks: &mut Blocks<R>,
) -> (Option<Result<Record, BinDecodeError>>, State)
where
  R: Read,
{
  if let Some(block) = blocks.next() {
    match block {
      Ok(mut block) => {
        let mut buf = tail.chain(block.data.clone());
        match extract_next_record(&mut block, &mut buf) {
          Ok(NextRecord::Record(rec)) => {
            match buf.peek_u8() {
              Some(n) if n != 0 => (Some(Ok(rec)), State::DecodingBlock(block, buf)),
              // end of block or 0 padding reached
              _ => (Some(Ok(rec)), State::BlockDone),
            }
          }
          Ok(NextRecord::Partial(_tail)) => (
            Some(Err(BinDecodeError::failure("record larger than the block"))),
            State::Done,
          ),
          Err(e) => (Some(Err(e)), State::Done),
        }
      }
      Err(e) => (
        Some(Err(BinDecodeError::failure(format!("read block: {}", e)))),
        State::Done,
      ),
    }
  } else {
    if tail.is_empty(/* first block, or last block has no partial record bytes at the end */) {
      (None, State::Done)
    } else {
      (Some(Err(BinDecodeError::incomplete())), State::Done)
    }
  }
}

fn extract_next_record(
  block: &mut Block,
  buf: &mut Chain<Bytes, Bytes>,
) -> Result<NextRecord, BinDecodeError> {
  let pos = buf.remaining();

  let r = crate::records::Record::decode(buf);
  match r {
    Ok(rec) => Ok(NextRecord::Record(rec)),
    Err(e) => {
      if e.is_incomplete() {
        let tail = block.data.split_off(block.data.len() - pos);
        Ok(NextRecord::Partial(tail))
      } else {
        Err(e)
      }
    }
  }
}

enum NextRecord {
  Record(Record),
  Partial(Bytes),
}

macro_rules! record_enum {
  (
    pub enum Record {
      $($type_id:ident($payload_ty:ty)),*
    }
  ) => {
    #[derive(Debug, PartialEq)]
    pub enum Record {
      $(
        $type_id($payload_ty),
      )*
    }

    impl Record {
      pub fn type_id(&self) -> RecordTypeId {
        match *self {
          $(
            Record::$type_id(_) => {
              RecordTypeId::$type_id
            }
          )*,
        }
      }
    }

    impl BinDecode for Record {
      const MIN_SIZE: usize = 1;
      const FIXED_SIZE: bool = false;

      fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
        buf.check_size(1)?;
        let type_id = RecordTypeId::decode(buf)?;
        match type_id {
          $(
            RecordTypeId::$type_id => {
              Ok(Record::$type_id(<$payload_ty>::decode(buf)?))
            },
          )*
          RecordTypeId::UnknownValue(v) => Err(BinDecodeError::failure(format!("unknown record type id: {}", v)))
        }
      }
    }

    impl BinEncode for Record {
      fn encode<T: BufMut>(&self, buf: &mut T) {
        match *self {
          $(
            Record::$type_id(ref payload) => {
              RecordTypeId::$type_id.encode(buf);
              payload.encode(buf);
            }
          )*,
        }
      }
    }
  };
}

record_enum! {
  pub enum Record {
    GameInfo(GameInfo),
    PlayerInfo(PlayerInfoRecord),
    PlayerLeft(PlayerLeft),
    SlotInfo(SlotInfo),
    CountDownStart(CountDownStart),
    CountDownEnd(CountDownEnd),
    GameStart(GameStart),
    TimeSlotFragment(TimeSlotFragment),
    TimeSlot(TimeSlot),
    ChatMessage(PlayerChatMessage),
    TimeSlotAck(TimeSlotAck),
    Desync(Desync),
    EndTimer(EndTimer),
    ProtoBuf(ProtoBufPayload)
  }
}

#[derive(Debug, BinEncode, BinDecode, PartialEq, Clone)]
pub struct GameInfo {
  pub num_of_host_records: u32,
  pub host_player_info: PlayerInfo,
  pub game_name: CString,
  #[bin(eq = 0)]
  pub _unk_1: u8,
  pub game_settings: GameSettings,
  pub player_count: u32,
  #[bin(bitflags(u32))]
  pub game_flags: GameFlags,
  pub language_id: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq, Clone)]
pub struct PlayerInfo {
  pub id: u8,
  pub name: CString,
  pub _size_of_additional_data: u8,
  #[bin(repeat = "_size_of_additional_data")]
  pub additional_data: Vec<u8>,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct PlayerInfoRecord {
  pub player_info: PlayerInfo,
  pub unknown: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct PlayerLeft {
  pub reason: LeaveReason,
  pub player_id: u8,
  pub result: u32,
  pub unknown: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct GameStart {
  #[bin(eq = 1)]
  pub unknown: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct CountDownStart(GameStart);

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct CountDownEnd(GameStart);

#[derive(Debug, PartialEq)]
pub struct TimeSlot {
  pub time_increment_ms: u16,
  pub actions: Vec<PlayerAction>,
}

impl BinDecode for TimeSlot {
  const MIN_SIZE: usize = 4;
  const FIXED_SIZE: bool = false;

  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(4)?;
    let len = buf.get_u16_le();
    if buf.remaining() < len as usize {
      return Err(BinDecodeError::incomplete().context("Action data").into());
    }

    let end_remaining = buf
      .remaining()
      .checked_sub(len as usize)
      .ok_or_else(|| BinDecodeError::failure("invalid action data length"))?;

    let time_increment_ms: u16 = BinDecode::decode(buf)?;

    if buf.remaining() == end_remaining {
      return Ok(TimeSlot {
        time_increment_ms,
        actions: vec![],
      });
    }

    let mut actions = vec![];

    loop {
      if buf.remaining() < size_of::<u8>(/* player_id */) + size_of::<u16>(/* data_len */) {
        return Err(
          BinDecodeError::incomplete()
            .context("PlayerAction header")
            .into(),
        );
      }

      let player_id = buf.get_u8();
      let data_len = buf.get_u16_le() as usize;

      if buf
        .remaining()
        .checked_sub(data_len)
        .ok_or_else(|| BinDecodeError::failure("invalid action data length"))?
        < end_remaining
      {
        return Err(BinDecodeError::failure("invalid action data length"));
      }

      let mut data = BytesMut::with_capacity(data_len);
      data.resize(data.capacity(), 0);
      buf.copy_to_slice(&mut data);

      let action = PlayerAction {
        player_id,
        data: data.freeze(),
      };

      actions.push(action);

      if buf.remaining() == end_remaining {
        break;
      }
    }

    Ok(TimeSlot {
      time_increment_ms,
      actions,
    })
  }
}

impl BinEncode for TimeSlot {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    let len: usize = size_of::<u16>(/* time_increment_ms */)
      + self
        .actions
        .iter()
        .map(PlayerAction::byte_len)
        .sum::<usize>();
    buf.put_u16_le(len as u16);
    buf.put_u16(self.time_increment_ms);
    for action in &self.actions {
      buf.put_u8(action.player_id);
      buf.put_u16_le(action.data.len() as u16);
      buf.put(action.data.as_ref());
    }
  }
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct TimeSlotFragment(pub TimeSlot);

#[derive(Debug, PartialEq)]
pub struct PlayerChatMessage {
  pub player_id: u8,
  pub message: ChatMessage,
}

impl BinDecode for PlayerChatMessage {
  const MIN_SIZE: usize = 1 + 2 + ChatMessage::MIN_SIZE;
  const FIXED_SIZE: bool = false;

  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    buf.check_size(Self::MIN_SIZE)?;
    let player_id = buf.get_u8();
    let len = buf.get_u16_le() as usize;
    buf.check_size(len)?;
    let expected_remaining = buf.remaining() - len;
    let message = ChatMessage::decode(buf)?;
    if buf.remaining() != expected_remaining {
      return Err(BinDecodeError::failure("unexpected chat message length"));
    }
    Ok(Self { player_id, message })
  }
}

impl BinEncode for PlayerChatMessage {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_u8(self.player_id);
    buf.put_u16_le(self.message.encode_len() as u16 + 1);
    self.message.encode(buf)
  }
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct TimeSlotAck {
  #[bin(eq = 4)]
  pub _size_checksum: u8,
  pub checksum: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct EndTimer {
  pub over: bool,
  pub countdown_sec: u32,
}

#[test]
fn test_record() {
  let bytes = flo_util::sample_bytes!("replay", "16k.w3g");
  let mut buf = bytes.as_slice();
  let header = crate::header::Header::decode(&mut buf).unwrap();

  let mut rec_count = 0;
  let blocks = crate::block::Blocks::from_buf(buf, header.num_blocks as usize);
  let empty = Bytes::new();
  let mut tail = empty.clone();
  for (_i, block) in blocks.enumerate() {
    let mut block = block.unwrap();
    let mut buf = tail.chain(block.data.clone());
    loop {
      let pos = buf.remaining();

      let r = crate::records::Record::decode(&mut buf).map_err(|e| {
        // flo_util::dump_hex(buf);
        e
      });
      match r {
        Ok(rec) => {
          rec_count = rec_count + 1;
          if let Record::GameInfo(gameinfo) = rec {
            dbg!(gameinfo);
          }
        }
        Err(e) => {
          if e.is_incomplete() {
            tail = block.data.split_off(block.data.len() - pos);
            // flo_util::dump_hex(tail.as_ref());
            break;
          } else {
            Err(e).unwrap()
          }
        }
      }

      match buf.peek_u8() {
        Some(n) if n != 0 => {}
        // end of block or 0 padding reached
        _ => {
          tail = empty.clone();
          break;
        }
      }
    }
  }

  if tail.len() > 0 {
    panic!("extra bytes = {}", tail.len());
  }

  dbg!(rec_count);
}

#[test]
fn test_record_iter() {
  let bytes = flo_util::sample_bytes!("replay", "grubby_happy.w3g");
  let mut buf = bytes.as_slice();
  let header = crate::header::Header::decode(&mut buf).unwrap();

  let blocks = crate::block::Blocks::from_buf(buf, header.num_blocks as usize);
  let iter = RecordIter::new(blocks);
  let mut n = 0;
  for record in iter {
    let _record = record.unwrap();
    n = n + 1;
  }
  dbg!(n);
}
