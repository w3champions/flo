use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use flo_util::binary::BinDecode;

use crate::constants::MAX_PAYLOAD_LEN;
use crate::error::Error;
use crate::packet::{Frame, FramePayload, Header, PacketTypeId};
use crate::w3gs::W3GSMetadata;

#[derive(Debug)]
pub struct FloFrameCodec {
  decode_state: DecoderState,
}

impl FloFrameCodec {
  pub fn new() -> Self {
    Self {
      decode_state: DecoderState::DecodingHeader,
    }
  }
}

impl Decoder for FloFrameCodec {
  type Item = Frame;
  type Error = Error;

  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    match self.decode_state {
      DecoderState::DecodingHeader => {
        if src.remaining() >= Header::MIN_SIZE {
          let header = Header::decode(src)?;
          let payload_len = header.payload_len as usize;

          if payload_len > MAX_PAYLOAD_LEN {
            return Err(Error::PayloadTooLarge);
          }

          if src.remaining() >= payload_len {
            // payload received
            Ok(Some(Self::frame(
              header.type_id,
              src.split_to(payload_len).freeze(),
            )?))
          } else {
            // wait payload
            src.reserve(payload_len);
            self.decode_state = DecoderState::DecodingPayload {
              header: Some(header),
              payload_len,
            };
            Ok(None)
          }
        } else {
          // wait header
          Ok(None)
        }
      }
      DecoderState::DecodingPayload {
        ref mut header,
        payload_len,
      } => {
        if src.remaining() >= payload_len {
          let header = header.take().expect("header");
          let payload = src.split_to(payload_len);
          let frame = Self::frame(header.type_id, payload.freeze())?;
          self.decode_state = DecoderState::DecodingHeader;
          Ok(Some(frame))
        } else {
          Ok(None)
        }
      }
    }
  }
}

#[derive(Debug)]
enum DecoderState {
  DecodingHeader,
  DecodingPayload {
    header: Option<Header>,
    payload_len: usize,
  },
}

impl Encoder<Frame> for FloFrameCodec {
  type Error = Error;

  #[inline]
  fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
    item.encode(dst);
    Ok(())
  }
}

impl FloFrameCodec {
  #[inline]
  fn frame(type_id: PacketTypeId, mut payload: Bytes) -> Result<Frame, Error> {
    Ok(Frame {
      type_id,
      payload: if type_id == PacketTypeId::W3GS {
        let metadata = W3GSMetadata::decode(&mut payload)?;
        FramePayload::W3GS { metadata, payload }
      } else {
        FramePayload::Bytes(payload)
      },
    })
  }
}
