



use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::error::*;
use crate::protocol::constants::{PacketTypeId};
use crate::protocol::packet::{
  PacketPayload, PacketPayloadDecode, PacketPayloadEncode,
};


#[derive(Debug, PartialEq)]
pub struct OutgoingAction {
  pub crc32: u32,
  pub data: Bytes,
}

impl PacketPayloadEncode for OutgoingAction {
  fn encode(&self, buf: &mut BytesMut) {
    buf.reserve(size_of::<u32>() + self.data.len());
    buf.put_u32_le(self.crc32);
    buf.put(self.data.clone());
  }
}

impl PacketPayloadDecode for OutgoingAction {
  fn decode(buf: &mut Bytes) -> Result<Self> {
    if buf.remaining() < 4 {
      return Err(Error::InvalidPayloadLength(buf.remaining()));
    }

    let checksum = u32::decode(buf)?;

    if !buf.has_remaining() {
      return Err(
        BinDecodeError::incomplete()
          .context("OutgoingAction data")
          .into(),
      );
    }

    let data = buf.split_to(buf.remaining());

    let mut crc32 = crc32fast::Hasher::new();
    crc32.update(data.as_ref());
    let crc32 = crc32.finalize();

    if checksum != crc32 {
      return Err(Error::InvalidChecksum);
    }

    Ok(Self { crc32, data })
  }
}

impl PacketPayload for OutgoingAction {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::OutgoingAction;
}

#[derive(Debug, PartialEq)]
pub struct IncomingAction(pub TimeSlot);

impl PacketPayload for IncomingAction {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::IncomingAction;
}

impl PacketPayloadEncode for IncomingAction {
  fn encode(&self, buf: &mut BytesMut) {
    self.0.encode(buf)
  }
}

impl PacketPayloadDecode for IncomingAction {
  fn decode(buf: &mut Bytes) -> Result<Self, Error> {
    Ok(Self(PacketPayloadDecode::decode(buf)?))
  }
}

#[derive(Debug, PartialEq)]
pub struct IncomingAction2(pub TimeSlot);

impl PacketPayload for IncomingAction2 {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::IncomingAction2;
}

impl PacketPayloadEncode for IncomingAction2 {
  fn encode(&self, buf: &mut BytesMut) {
    self.0.encode(buf)
  }
}

impl PacketPayloadDecode for IncomingAction2 {
  fn decode(buf: &mut Bytes) -> Result<Self, Error> {
    Ok(Self(PacketPayloadDecode::decode(buf)?))
  }
}

#[derive(Debug, PartialEq)]
pub struct TimeSlot {
  pub time_increment_ms: u16,
  pub actions: Vec<PlayerAction>,
}

enum IncomingAction2Chunks {
  Empty,
  One(TimeSlot),
  Many(Vec<TimeSlot>),
}

pub struct IncomingAction2ChunksIter(IncomingAction2Chunks);

impl Iterator for IncomingAction2ChunksIter {
  type Item = TimeSlot;

  fn next(&mut self) -> Option<Self::Item> {
    let (item, next) = match std::mem::replace(&mut self.0, IncomingAction2Chunks::Empty) {
      IncomingAction2Chunks::Empty => (None, IncomingAction2Chunks::Empty),
      IncomingAction2Chunks::One(item) => (Some(item), IncomingAction2Chunks::Empty),
      IncomingAction2Chunks::Many(mut items) => {
        if items.is_empty() {
          (None, IncomingAction2Chunks::Empty)
        } else {
          (Some(items.remove(0)), IncomingAction2Chunks::Many(items))
        }
      }
    };
    self.0 = next;
    item
  }
}

impl TimeSlot {
  // https://github.com/Josko/aura-bot/blob/1e5df425fd325e9b0e6aa8fa5eed35f0c61f3114/src/game.cpp#L963
  const MAX_ACTION_DATA_LEN: usize = 1452;

  /// Splits self to a iterator which yields items with
  /// action data length < `Self::MAX_ACTION_DATA_LEN`
  /// `time_increment_ms` will only be set on the last item
  /// FIXME: refactor, add real time increments value
  pub fn split_chunks(self) -> IncomingAction2ChunksIter {
    if self
      .actions
      .iter()
      .map(PlayerAction::byte_len)
      .sum::<usize>()
      < Self::MAX_ACTION_DATA_LEN
    {
      IncomingAction2ChunksIter(IncomingAction2Chunks::One(self))
    } else {
      let mut payloads = vec![];
      let mut data_len = 0;
      let mut actions = vec![];
      for action in self.actions {
        let action_len = action.byte_len();
        if data_len + action_len > Self::MAX_ACTION_DATA_LEN {
          data_len = 0;
          payloads.push(TimeSlot {
            time_increment_ms: 0,
            actions: std::mem::replace(&mut actions, vec![action]),
          });
        } else {
          data_len = data_len + action_len;
          actions.push(action)
        }
      }

      if !actions.is_empty() {
        payloads.push(TimeSlot {
          time_increment_ms: self.time_increment_ms,
          actions,
        })
      } else {
        payloads
          .last_mut()
          .expect("single action data too large")
          .time_increment_ms = self.time_increment_ms;
      }

      IncomingAction2ChunksIter(IncomingAction2Chunks::Many(payloads))
    }
  }
}

impl PacketPayloadEncode for TimeSlot {
  fn encode(&self, buf: &mut BytesMut) {
    let actions_len: usize = self.actions.iter().map(PlayerAction::byte_len).sum();
    let header_len = size_of::<u16>() + // send_interval
      size_of::<u16>(); // crc16
    buf.reserve(header_len + actions_len);

    if self.actions.is_empty() {
      buf.put_u16_le(self.time_increment_ms);
    } else {
      buf.put_u16_le(self.time_increment_ms);
      let mut actions_buf = buf.split_off(header_len);

      for action in &self.actions {
        action.encode(&mut actions_buf);
      }

      buf.put_u16_le(crc16(actions_buf.as_ref()));

      buf.unsplit(actions_buf);
    }
  }
}

impl PacketPayloadDecode for TimeSlot {
  fn decode(buf: &mut Bytes) -> Result<Self, Error> {
    if buf.remaining() < size_of::<u16>() {
      return Err(
        BinDecodeError::incomplete()
          .context("IncomingAction2 time_increment_ms")
          .into(),
      );
    }

    let time_increment_ms = buf.get_u16_le();

    if !buf.has_remaining() {
      return Ok(Self {
        time_increment_ms,
        actions: vec![],
      });
    }

    if buf.remaining() < size_of::<u16>() {
      return Err(
        BinDecodeError::incomplete()
          .context("IncomingAction2 crc16")
          .into(),
      );
    }

    let crc16_value = buf.get_u16_le();
    let expected_crc16 = crc16(buf.as_ref());

    if crc16_value != expected_crc16 {
      return Err(Error::InvalidChecksum);
    }

    let mut actions = vec![];
    while buf.has_remaining() {
      actions.push(PlayerAction::decode(buf)?)
    }

    Ok(Self {
      time_increment_ms,
      actions,
    })
  }
}

#[derive(Debug, PartialEq, Clone)]
pub struct PlayerAction {
  pub player_id: u8,
  pub data: Bytes,
}

impl PlayerAction {
  pub fn byte_len(&self) -> usize {
    size_of::<u8>(/* player_id */) + size_of::<u16>(/* data_len */) + self.data.len()
  }

  fn encode(&self, buf: &mut BytesMut) {
    buf.put_u8(self.player_id);
    buf.put_u16_le(self.data.len() as u16);
    buf.put(self.data.as_ref());
  }

  fn decode(buf: &mut Bytes) -> Result<Self> {
    if buf.remaining() < size_of::<u8>(/* player_id */) + size_of::<u16>(/* data_len */) {
      return Err(
        BinDecodeError::incomplete()
          .context("PlayerAction header")
          .into(),
      );
    }

    let player_id = buf.get_u8();
    let data_len = buf.get_u16_le() as usize;

    if buf.remaining() < data_len {
      return Err(
        BinDecodeError::incomplete()
          .context("PlayerAction bytes")
          .into(),
      );
    }

    Ok(Self {
      player_id,
      data: { buf.split_to(data_len) },
    })
  }
}

#[derive(Debug, PartialEq, BinDecode, BinEncode)]
pub struct OutgoingKeepAlive {
  pub unknown: u8,
  pub checksum: u32,
}

impl PacketPayload for OutgoingKeepAlive {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::OutgoingKeepAlive;
}

fn crc16(data: &[u8]) -> u16 {
  let mut crc32 = crc32fast::Hasher::new();
  crc32.update(data);
  (crc32.finalize() & 0x0000FFFF) as u16
}

#[test]
fn test_outgoing_action() {
  let mut buf = Bytes::from(flo_util::sample_bytes!("packet", "outgoing_action.bin")).split_off(4);
  let _p = OutgoingAction::decode(&mut buf).unwrap();

  crate::packet::test_payload_type(
    "outgoing_action.bin",
    &OutgoingAction {
      crc32: 2869691446,
      data: Bytes::from(vec![
        22, 1, 4, 0, 116, 51, 0, 0, 116, 51, 0, 0, 139, 51, 0, 0, 139, 51, 0, 0, 185, 51, 0, 0,
        185, 51, 0, 0, 208, 51, 0, 0, 208, 51, 0, 0, 26, 25, 97, 101, 112, 104, 116, 51, 0, 0, 116,
        51, 0, 0,
      ]),
    },
  )
}

#[test]
fn test_incoming_action() {
  crate::packet::test_payload_type(
    "incoming_action.bin",
    &IncomingAction(TimeSlot {
      time_increment_ms: 100,
      actions: vec![PlayerAction {
        player_id: 2,
        data: Bytes::from(vec![
          22, 1, 4, 0, 116, 51, 0, 0, 116, 51, 0, 0, 139, 51, 0, 0, 139, 51, 0, 0, 185, 51, 0, 0,
          185, 51, 0, 0, 208, 51, 0, 0, 208, 51, 0, 0, 26, 25, 97, 101, 112, 104, 116, 51, 0, 0,
          116, 51, 0, 0,
        ]),
      }],
    }),
  )
}

#[test]
fn test_incoming_action2_split_chunk_1() {
  let payload = TimeSlot {
    time_increment_ms: 100,
    actions: vec![PlayerAction {
      player_id: 1,
      data: Bytes::from((0..100).collect::<Vec<u8>>()),
    }],
  };

  let splitted = payload.split_chunks().collect::<Vec<_>>();
  assert_eq!(
    splitted,
    vec![TimeSlot {
      time_increment_ms: 100,
      actions: vec![PlayerAction {
        player_id: 1,
        data: Bytes::from((0..100).collect::<Vec<u8>>()),
      }],
    }]
  )
}

#[test]
fn test_incoming_action2_split_chunk_30() {
  let mut payload = TimeSlot {
    time_increment_ms: 100,
    actions: vec![],
  };

  for _ in 0..30 {
    payload.actions.push(PlayerAction {
      player_id: 1,
      data: Bytes::from((0..100).collect::<Vec<u8>>()),
    })
  }

  let splitted = payload.split_chunks().collect::<Vec<_>>();
  assert_eq!(splitted.len(), 3000 / TimeSlot::MAX_ACTION_DATA_LEN + 1);

  for i in splitted.iter().rev().skip(1) {
    assert_eq!(i.time_increment_ms, 0);
  }
  assert_eq!(splitted.last().unwrap().time_increment_ms, 100);
  assert_eq!(
    splitted.iter().map(|p| p.actions.len()).sum::<usize>(),
    30_usize
  )
}
