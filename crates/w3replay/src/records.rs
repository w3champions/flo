use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};
pub use flo_w3gs::action::PlayerAction;
use flo_w3gs::constants::{LeaveReason, RacePref};
pub use flo_w3gs::desync::Desync;
pub use flo_w3gs::packet::ProtoBufPayload;
use flo_w3gs::protocol::chat::ChatMessage;
pub use flo_w3gs::slot::SlotInfo;

use crate::constants::RecordTypeId;

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
    ChatMessage(ChatMessage),
    TimeSlotAck(TimeSlotAck),
    Desync(Desync),
    EndTimer(EndTimer),
    ProtoBuf(ProtoBufPayload)
  }
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
pub struct GameInfo {
  pub num_of_host_records: u32,
  pub host_player_info: PlayerInfo,
  pub game_name: CString,
  #[bin(eq = 0)]
  pub _unk_1: u8,
  pub encoded_string: CString,
  pub player_count: u32,
  pub game_type: u32,
  pub language_id: u32,
}

#[derive(Debug, BinEncode, BinDecode, PartialEq)]
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
pub struct TimeSlotFragment(TimeSlot);

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
  use crate::header::Header;
  use bytes::buf::ext::Chain;
  let bytes = flo_util::sample_bytes!("replay", "16k.w3g");
  let mut buf = bytes.as_slice();
  buf.get_tag(crate::constants::SIGNATURE).unwrap();
  let header = crate::header::Header::decode(&mut buf).unwrap();
  dbg!(&header);

  let mut rec_count = 0;
  let mut blocks = crate::block::Blocks::from_buf(buf, header.num_blocks as usize);
  let empty = Bytes::new();
  let mut tail = empty.clone();
  for (i, block) in blocks.enumerate() {
    dbg!(i);

    let block = block.unwrap();
    let mut buf = Chain::new(tail, block.data);
    let buf_size = buf.remaining();
    loop {
      let pos = buf.remaining();

      dbg!(buf_size - pos);

      let r = crate::records::Record::decode(&mut buf).map_err(|e| {
        // flo_util::dump_hex(buf);
        e
      });
      match r {
        Ok(rec) => {
          rec_count = rec_count + 1;
          dbg!(rec_count);
        }
        Err(e) => {
          if e.is_incomplete() {
            let offset = pos - buf.remaining();
            let last_len = buf.last_ref().len();
            tail = buf.last_mut().split_off(last_len - offset);
            flo_util::dump_hex(tail.as_ref());
            break;
          } else {
            Err(e).unwrap()
          }
        }
      }

      if !buf.has_remaining() {
        tail = empty.clone();
        break;
      }
    }
  }

  if tail.len() > 0 {
    panic!("extra bytes = {}", tail.len());
  }
}
