use thiserror::Error;

use crate::error::*;
use crate::packet::{Frame, FramePayload, PacketTypeId};
use bitflags::bitflags;
use flo_util::binary::{BinBufExt, BinDecode, Buf};
use flo_util::{BinDecode, BinEncode};
use flo_w3gs::packet::Packet;
pub use flo_w3gs::packet::{Header as W3GSHeader, Packet as W3GSPacket};
pub use flo_w3gs::protocol::constants::PacketTypeId as W3GSPacketTypeId;
use std::collections::VecDeque;

#[derive(Error, Debug)]
pub enum ParseW3GSPacketError {
  #[error("unexpected end of buffer")]
  EOB,
  #[error("expect a W3GS frame")]
  NotW3GS,
}

pub trait W3GSFrameExt {
  fn try_into_w3gs(self) -> Result<(W3GSMetadata, W3GSPacket)>;
  fn from_w3gs(metadata: W3GSMetadata, packet: W3GSPacket) -> Self;
}

impl W3GSFrameExt for Frame {
  #[inline]
  fn try_into_w3gs(self) -> Result<(W3GSMetadata, Packet)> {
    frame_to_w3gs(self)
  }

  #[inline]
  fn from_w3gs(metadata: W3GSMetadata, packet: Packet) -> Self {
    w3gs_to_frame(metadata, packet)
  }
}

#[inline]
fn frame_to_w3gs(frame: Frame) -> Result<(W3GSMetadata, W3GSPacket)> {
  let (metadata, payload) = match frame.payload {
    FramePayload::Bytes(_) => {
      return Err(Error::ReadW3GSFrame(ParseW3GSPacketError::NotW3GS));
    }
    FramePayload::W3GS { metadata, payload } => (metadata, payload),
  };

  let header = W3GSHeader::new(
    W3GSPacketTypeId::from(metadata.type_id()),
    (payload.remaining() + 4) as u16,
  );

  Ok((metadata, W3GSPacket { header, payload }))
}

#[inline]
fn w3gs_to_frame(metadata: W3GSMetadata, packet: W3GSPacket) -> Frame {
  Frame {
    type_id: PacketTypeId::W3GS,
    payload: FramePayload::W3GS {
      metadata,
      payload: packet.payload,
    },
  }
}

bitflags! {
  struct W3GSMetadataFlags: u8 {
    const ACK = 0b00000001;
  }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct W3GSMetadata {
  #[bin(bitflags(u8))]
  flags: W3GSMetadataFlags,
  type_id: W3GSPacketTypeId,
  sid: u32,
  #[bin(condition = "flags.contains(W3GSMetadataFlags::ACK)")]
  ack_sid: Option<u32>,
}

impl W3GSMetadata {
  pub fn type_id(&self) -> W3GSPacketTypeId {
    self.type_id
  }

  pub fn sid(&self) -> u32 {
    self.sid
  }

  pub fn ack_sid(&self) -> Option<u32> {
    self.ack_sid.clone()
  }

  pub fn len(&self) -> usize {
    W3GSMetadata::MIN_SIZE + if self.ack_sid.is_some() { 4 } else { 0 }
  }

  pub fn new(type_id: W3GSPacketTypeId, sid: u32, ack_sid: Option<u32>) -> Self {
    Self {
      flags: if ack_sid.is_some() {
        W3GSMetadataFlags::ACK
      } else {
        W3GSMetadataFlags::empty()
      },
      type_id,
      sid,
      ack_sid,
    }
  }
}

#[derive(Debug)]
pub struct W3GSAckQueue {
  tx_next_sid: u32,
  tx_ack_sid: Option<u32>,
  tx_pending_ack_q: VecDeque<(W3GSMetadata, W3GSPacket)>,
  rx_ack_sid: Option<u32>,
  last_rx_ack_sid: Option<u32>,
}

impl W3GSAckQueue {
  const WINDOW_SIZE: u32 = u32::MAX / 2;

  pub fn new() -> Self {
    Self {
      tx_next_sid: 0,
      tx_ack_sid: None,
      tx_pending_ack_q: VecDeque::new(),
      rx_ack_sid: None,
      last_rx_ack_sid: None,
    }
  }

  pub fn gen_next_send_sid(&mut self) -> u32 {
    self.tx_next_sid = self.tx_next_sid.wrapping_add(1);
    self.tx_next_sid
  }

  pub fn push_send(&mut self, meta: W3GSMetadata, packet: W3GSPacket) {
    self.tx_pending_ack_q.push_back((meta, packet));
  }

  pub fn ack_sent(&mut self, ack_sid: u32) {
    // tracing::debug!("ack_sent: {}", ack_sid);
    self.tx_ack_sid.replace(ack_sid);
    let mut found = false;
    while let Some((meta, _)) = self.tx_pending_ack_q.pop_front() {
      if meta.sid == ack_sid {
        found = true;
        break;
      }
    }
    if !found {
      tracing::warn!("ack_sid not found: {}", ack_sid);
    }
  }

  #[must_use]
  pub fn ack_received(&mut self, sid: u32) -> bool {
    // tracing::debug!("ack_received: {} {:?}", sid, self.last_rx_ack_sid);
    let should_discard = self
      .last_rx_ack_sid
      .clone()
      .map(|last_sid| {
        if last_sid > sid && ((u32::MAX - last_sid).saturating_add(sid)) < Self::WINDOW_SIZE {
          // wrapped
          return false;
        }

        if sid > last_sid {
          return false;
        }

        // discard
        return true;
      })
      .unwrap_or(false);
    if should_discard {
      return false;
    }

    self.rx_ack_sid.replace(sid);
    self.last_rx_ack_sid.replace(sid);
    true
  }

  pub fn take_ack_received(&mut self) -> Option<u32> {
    self.rx_ack_sid.take()
  }

  pub fn last_ack_received(&self) -> Option<u32> {
    self.last_rx_ack_sid.clone()
  }

  pub fn pending_ack_len(&self) -> usize {
    self.tx_pending_ack_q.len()
  }

  pub fn pending_ack_queue(&self) -> &VecDeque<(W3GSMetadata, W3GSPacket)> {
    &self.tx_pending_ack_q
  }
}

#[test]
fn test_frame_to_w3gs() {
  use flo_util::binary::*;
  let frame = Frame {
    type_id: PacketTypeId::W3GS,
    payload: FramePayload::W3GS {
      metadata: W3GSMetadata::new(W3GSPacketTypeId::GameOver, 1234, None),
      payload: Bytes::from_static(&[1, 2, 3, 4, 5]),
    },
  };
  let mut buf = BytesMut::new();
  frame.encode(&mut buf);

  assert_eq!(u8::from(W3GSPacketTypeId::GameOver), 0x14);

  let len_bytes = 11_u16.to_le_bytes();
  let sid_bytes = 1234_u32.to_le_bytes();

  assert_eq!(
    buf.chunk(),
    &[
      0xF7_u8,
      len_bytes[0],
      len_bytes[1],
      0,
      0x14,
      sid_bytes[0],
      sid_bytes[1],
      sid_bytes[2],
      sid_bytes[3],
      1,
      2,
      3,
      4,
      5
    ] as &[_]
  );

  let frame = Frame {
    type_id: PacketTypeId::W3GS,
    payload: FramePayload::W3GS {
      metadata: W3GSMetadata::new(W3GSPacketTypeId::GameOver, 1234, 5678.into()),
      payload: Bytes::from_static(&[1, 2, 3, 4, 5]),
    },
  };
  let mut buf = BytesMut::new();
  frame.encode(&mut buf);

  let len_bytes = 15_u16.to_le_bytes();
  let ack_sid_bytes = 5678_u32.to_le_bytes();

  assert_eq!(
    buf.chunk(),
    &[
      0xF7_u8,
      len_bytes[0],
      len_bytes[1],
      0b00000001,
      0x14,
      sid_bytes[0],
      sid_bytes[1],
      sid_bytes[2],
      sid_bytes[3],
      ack_sid_bytes[0],
      ack_sid_bytes[1],
      ack_sid_bytes[2],
      ack_sid_bytes[3],
      1,
      2,
      3,
      4,
      5
    ] as &[_]
  );
}
