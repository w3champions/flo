use thiserror::Error;

use crate::packet::{Frame, FramePayload, PacketTypeId};
use flo_util::binary::Buf;
pub use flo_w3gs::packet::{Header as W3GSHeader, Packet as W3GSPacket};
use flo_w3gs::protocol::constants::PacketTypeId as W3GSPacketTypeId;

use crate::error::*;

#[derive(Error, Debug)]
pub enum ParseW3GSPacketError {
  #[error("unexpected end of buffer")]
  EOB,
  #[error("expect a frame with subtype")]
  Subtype,
}

#[inline]
pub fn frame_to_w3gs(frame: Frame) -> Result<W3GSPacket> {
  let (subtype, payload) = frame
    .payload
    .into_subtype()
    .ok_or_else(|| Error::ReadW3GSFrame(ParseW3GSPacketError::Subtype))?;

  let header = W3GSHeader::new(
    W3GSPacketTypeId::from(subtype),
    (payload.remaining() + 4) as u16,
  );

  Ok(W3GSPacket { header, payload })
}

#[inline]
pub fn w3gs_to_frame(packet: W3GSPacket) -> Frame {
  Frame {
    type_id: PacketTypeId::W3GS,
    payload: FramePayload::SubTypeBytes(packet.header.type_id.into(), packet.payload),
  }
}

#[test]
fn test_frame_to_w3gs() {
  use flo_util::binary::*;
  let frame = Frame {
    type_id: PacketTypeId::W3GS,
    payload: FramePayload::SubTypeBytes(
      W3GSPacketTypeId::GameOver.into(),
      Bytes::from_static(&[1, 2, 3, 4, 5]),
    ),
  };
  let mut buf = BytesMut::new();
  frame.encode(&mut buf);

  assert_eq!(u8::from(W3GSPacketTypeId::GameOver), 0x14);

  let len_bytes = 6_u16.to_le_bytes();

  assert_eq!(
    buf.bytes(),
    &[0xF7_u8, len_bytes[0], len_bytes[1], 0x14, 1, 2, 3, 4, 5] as &[_]
  )
}
