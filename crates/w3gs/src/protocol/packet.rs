use prost::Message;

use flo_util::binary::*;
use flo_util::binary::{BinDecode, BinEncode};
use flo_util::{BinDecode, BinEncode};

use crate::error::{Error, Result};
use crate::protocol::constants::{PacketTypeId, ProtoBufMessageTypeId};

pub trait PacketPayload: Sized {
  const PACKET_TYPE_ID: PacketTypeId;
}

impl<'a, T> PacketPayload for &'a T
where
  T: PacketPayload,
{
  const PACKET_TYPE_ID: PacketTypeId = T::PACKET_TYPE_ID;
}

/// Specialized version of `BinEncode`
/// `BinEncode` generic over `BufMut` but we need `BytesMut`
/// to reduce allocations/copies
pub trait PacketPayloadEncode {
  fn encode(&self, buf: &mut BytesMut);
  fn encode_len(&self) -> Option<usize> {
    None
  }
  fn encode_to_bytes(&self) -> Bytes {
    let mut buf = BytesMut::new();
    self.encode(&mut buf);
    buf.freeze()
  }
}

impl<'a, T> PacketPayloadEncode for &'a T
where
  T: PacketPayloadEncode,
{
  fn encode(&self, buf: &mut BytesMut) {
    <T as PacketPayloadEncode>::encode(*self, buf)
  }
  fn encode_len(&self) -> Option<usize> {
    <T as PacketPayloadEncode>::encode_len(*self)
  }
  fn encode_to_bytes(&self) -> Bytes {
    <T as PacketPayloadEncode>::encode_to_bytes(*self)
  }
}

/// Specialized version of `BinDecode`
/// `BinDecode` generic over `Buf` but we need `Bytes`
/// to reduce allocations/copies
pub trait PacketPayloadDecode: Sized {
  fn decode(buf: &mut Bytes) -> Result<Self>;
}

pub trait PacketProtoBufMessage: Message + Default {
  const MESSAGE_TYPE_ID: ProtoBufMessageTypeId;
}

#[derive(Debug)]
pub struct Packet {
  pub header: Header,
  pub payload: Bytes,
}

impl Packet {
  pub fn with_payload<T>(payload: T) -> Result<Packet>
  where
    T: PacketPayload + PacketPayloadEncode + std::fmt::Debug,
  {
    let mut buf = if let Some(len) = payload.encode_len() {
      BytesMut::with_capacity(len)
    } else {
      BytesMut::new()
    };

    // dbg!(&payload);

    payload.encode(&mut buf);

    if buf.len() > (std::u16::MAX - 4) as usize {
      return Err(Error::PayloadSizeOverflow);
    }

    Ok(Packet {
      header: Header::new(T::PACKET_TYPE_ID, (buf.len() as u16) + 4),
      payload: buf.freeze(),
    })
  }

  pub fn simple<T>(payload: T) -> Result<Packet>
  where
    T: PacketPayload + BinEncode + std::fmt::Debug,
  {
    Self::with_payload(SimplePayload(payload))
  }

  pub fn decode_header(buf: &mut BytesMut) -> Result<Header> {
    Header::decode(buf).map_err(Into::into)
  }

  pub fn decode(header: Header, buf: &mut BytesMut) -> Result<Self> {
    let payload_len = header.get_payload_len()?;
    buf.check_size(payload_len)?;
    Ok(Packet {
      header,
      payload: buf.split_to(payload_len).freeze(),
    })
  }

  pub fn decode_payload<T>(&self) -> Result<T>
  where
    T: PacketPayload + PacketPayloadDecode,
  {
    if self.header.type_id != T::PACKET_TYPE_ID {
      return Err(Error::PacketTypeIdMismatch {
        expected: T::PACKET_TYPE_ID,
        found: self.header.type_id,
      });
    }
    let mut buf = self.payload.clone();
    let payload = T::decode(&mut buf)?;
    if buf.has_remaining() {
      return Err(Error::ExtraPayloadBytes(buf.remaining()));
    }
    Ok(payload)
  }

  pub fn decode_simple_payload<T>(&self) -> Result<T>
  where
    T: PacketPayload + BinDecode,
  {
    self
      .decode_payload::<SimplePayload<T>>()
      .map(SimplePayload::into_inner)
  }

  pub fn decode_protobuf<T>(&self) -> Result<T>
  where
    T: PacketProtoBufMessage,
  {
    let payload: ProtoBufPayload = self.decode_simple_payload()?;
    payload.decode_message()
  }

  pub fn get_encode_len(&self) -> usize {
    4 + self.payload.len()
  }

  pub fn encode(&self, buf: &mut BytesMut) {
    buf.reserve(self.get_encode_len());
    self.header.encode(buf);
    buf.put_slice(&self.payload);
  }

  pub fn type_id(&self) -> PacketTypeId {
    self.header.type_id
  }

  pub fn len(&self) -> u16 {
    self.header.len
  }

  pub fn payload_len(&self) -> usize {
    self.payload.len()
  }
}

#[derive(Debug, BinDecode, BinEncode)]
pub struct Header {
  #[bin(eq = 0xF7)]
  _sig: u8,
  pub type_id: PacketTypeId,
  pub len: u16,
}

impl Header {
  fn new(type_id: PacketTypeId, len: u16) -> Self {
    Header {
      _sig: 0xF7,
      type_id,
      len,
    }
  }

  pub fn get_payload_len(&self) -> Result<usize> {
    let payload_len = self
      .len
      .checked_sub(4)
      .ok_or_else(|| Error::InvalidPacketLength(self.len))? as usize;
    Ok(payload_len)
  }
}

#[derive(Debug, BinDecode, BinEncode, PartialEq)]
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

  pub fn message_type_id(&self) -> ProtoBufMessageTypeId {
    self.type_id
  }
}

/// Adapter to bridge `Bin(En|De)code` with `PacketPayload(En|De)code`
#[derive(Debug, PartialEq)]
pub struct SimplePayload<T>(T);

impl<T> SimplePayload<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T> PacketPayload for SimplePayload<T>
where
  T: PacketPayload,
{
  const PACKET_TYPE_ID: PacketTypeId = T::PACKET_TYPE_ID;
}

impl<T> PacketPayloadEncode for SimplePayload<T>
where
  T: BinEncode,
{
  fn encode(&self, buf: &mut BytesMut) {
    BinEncode::encode(&self.0, buf)
  }
}

impl<T> PacketPayloadDecode for SimplePayload<T>
where
  T: BinDecode,
{
  fn decode(buf: &mut Bytes) -> Result<Self> {
    Ok(SimplePayload(BinDecode::decode(buf)?))
  }
}

#[cfg(test)]
pub(crate) fn test_payload_type<T>(filename: &str, expecting: &T)
where
  T: PacketPayload
    + PacketPayloadEncode
    + PacketPayloadDecode
    + std::cmp::PartialEq
    + std::fmt::Debug,
{
  let mut bytes = BytesMut::from(flo_util::sample_bytes!("packet", filename).as_slice());
  let header = Packet::decode_header(&mut bytes).unwrap();
  dbg!(&header);

  let packet = Packet::decode(header, &mut bytes).unwrap();
  assert!(!bytes.has_remaining());

  let payload: T = packet
    .decode_payload()
    .map_err(|e| {
      if let Error::ExtraPayloadBytes(len) = e {
        let extra = &packet.payload[(packet.payload.len() - len)..];
        println!("{:?}", extra);
        flo_util::dump_hex(extra);
      }
      e
    })
    .unwrap();

  assert_eq!(&payload, expecting);

  assert_eq!(payload.encode_to_bytes(), packet.payload);
}

#[cfg(test)]
pub(crate) fn test_simple_payload_type<T>(filename: &str, expecting: &T)
where
  T: PacketPayload + BinEncode + BinDecode + std::cmp::PartialEq + std::fmt::Debug,
{
  let mut bytes = BytesMut::from(flo_util::sample_bytes!("packet", filename).as_slice());
  let header = Packet::decode_header(&mut bytes).unwrap();
  dbg!(&header);

  let packet = Packet::decode(header, &mut bytes).unwrap();

  assert!(!bytes.has_remaining());

  let payload: T = packet
    .decode_simple_payload()
    .map_err(|e| {
      if let Error::ExtraPayloadBytes(len) = e {
        let extra = &packet.payload[(packet.payload.len() - len)..];
        println!("{:?}", extra);
        flo_util::dump_hex(extra);
      }
      e
    })
    .unwrap();
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

  let payload: ProtoBufPayload = packet.decode_simple_payload().unwrap();
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
