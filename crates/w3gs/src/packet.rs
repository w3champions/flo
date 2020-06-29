use prost::Message;

use flo_util::binary::*;
use flo_util::binary::{BinDecode, BinEncode};
use flo_util::{BinDecode, BinEncode};

use crate::constants::{PacketTypeId, ProtoBufMessageTypeId};
use crate::error::{Error, Result};

pub trait PacketPayload: BinDecode + BinEncode {
  const PACKET_TYPE_ID: PacketTypeId;
}

pub trait PacketProtoBufMessage: Message + Default {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId;
}

#[derive(Debug)]
pub struct Packet {
  pub header: Header,
  pub payload: Bytes,
}

// no generic buffer type because we mostly decode packets from tokio's buffer
// which is BytesMut.
impl Packet {
  pub fn decode_header(buf: &mut BytesMut) -> Result<Header> {
    Header::decode(buf).map_err(Into::into)
  }

  pub fn decode(header: Header, buf: &mut BytesMut) -> Result<Self> {
    let payload_len = header
      .len
      .checked_sub(4)
      .ok_or_else(|| BinDecodeError::failure(format!("invalid packet length: {}", header.len)))?
      as usize;
    buf.check_size(payload_len)?;
    Ok(Packet {
      header,
      payload: buf.split_to(payload_len).freeze(),
    })
  }

  pub fn decode_payload<T>(&self) -> Result<T>
  where
    T: PacketPayload,
  {
    if self.header.type_id != T::PACKET_TYPE_ID {
      return Err(Error::PacketTypeIdMismatch {
        expected: T::PACKET_TYPE_ID,
        found: self.header.type_id,
      });
    }
    let mut buf = self.payload.as_ref();
    let payload = T::decode(&mut buf)?;
    if buf.has_remaining() {
      return Err(Error::ExtraPayloadBytes(buf.remaining()));
    }
    Ok(payload)
  }

  pub fn decode_protobuf<T>(&self) -> Result<T>
  where
    T: PacketProtoBufMessage,
  {
    let payload: ProtoBufPayload = self.decode_payload()?;
    payload.decode_message()
  }
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct Header {
  #[bin(eq = 0xF7)]
  _sig: u8,
  pub type_id: PacketTypeId,
  pub len: u16,
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct ProtoBufPayload {
  pub type_id: ProtoBufMessageTypeId,
  pub len: u32,
  #[bin(repeat = "len")]
  pub data: Vec<u8>,
}

impl PacketPayload for ProtoBufPayload {
  const PACKET_TYPE_ID: PacketTypeId = PacketTypeId::ProtoBuf;
}

impl ProtoBufPayload {
  pub fn new<T>(message: T) -> Self
  where
    T: PacketProtoBufMessage,
  {
    let len = message.encoded_len();
    let mut data = Vec::with_capacity(len);
    message
      .encode(&mut data)
      .expect("should not error because the buffer does have sufficient capacity.");
    Self {
      type_id: T::MESSAGE_TYPE_ID,
      len: len as u32,
      data,
    }
  }

  pub fn decode_message<T>(&self) -> Result<T>
  where
    T: PacketProtoBufMessage,
  {
    let message = T::decode(self.data.as_slice())?;
    Ok(message)
  }
}

#[cfg(test)]
pub(crate) fn test_payload_type<T: PacketPayload + std::cmp::PartialEq + std::fmt::Debug>(
  filename: &str,
  expecting: &T,
) {
  let mut bytes = BytesMut::from(flo_util::sample_bytes!("packet", filename).as_slice());
  let header = Packet::decode_header(&mut bytes).unwrap();
  dbg!(&header);

  let packet = Packet::decode(header, &mut bytes).unwrap();
  flo_util::dump_hex(&packet.payload);

  assert!(!bytes.has_remaining());

  let payload: T = packet.decode_payload().unwrap();
  dbg!(&payload);

  assert_eq!(&payload, expecting);

  assert_eq!(payload.encode_to_bytes(), packet.payload);
}

#[cfg(test)]
pub(crate) fn test_protobuf_payload_type<
  T: PacketProtoBufMessage + std::cmp::PartialEq + std::fmt::Debug,
>(
  filename: &str,
  expecting: &T,
) {
  let mut bytes = BytesMut::from(flo_util::sample_bytes!("packet", filename).as_slice());
  let header = Packet::decode_header(&mut bytes).unwrap();
  dbg!(&header);

  let packet = Packet::decode(header, &mut bytes).unwrap();
  flo_util::dump_hex(&packet.payload);

  assert!(!bytes.has_remaining());

  let payload: ProtoBufPayload = packet.decode_payload().unwrap();
  dbg!(&payload);

  let message: T = payload.decode_message().unwrap();

  assert_eq!(&message, expecting);

  let new = ProtoBufPayload::new(message);
  assert_eq!(new.encode_to_bytes(), packet.payload);
}

#[test]
fn test_packet() {
  use flo_util::binary::*;
  let mut buf = BytesMut::from(flo_util::sample_bytes!("packet", "dual.bin").as_slice());
  let h1 = Packet::decode_header(&mut buf).unwrap();
  let p1 = Packet::decode(h1, &mut buf).unwrap();
  let h2 = Packet::decode_header(&mut buf).unwrap();
  let p2 = Packet::decode(h2, &mut buf).unwrap();
  assert!(!buf.has_remaining());

  dbg!(p1);
  dbg!(p2);
}
