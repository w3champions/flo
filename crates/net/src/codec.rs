use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use flo_util::binary::{BinDecode, BinEncode};

use crate::error::Error;
use crate::packet::{Frame, Header};

const MAX_PAYLOAD_LEN: usize = 4096;

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
            Ok(Some(Frame {
              type_id: header.type_id,
              payload: src.split_to(payload_len).freeze(),
            }))
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
          let frame = Frame {
            type_id: header.type_id,
            payload: src.split_to(payload_len).freeze(),
          };
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

  fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
    dst.reserve(Header::MIN_SIZE + item.payload.len());
    item.type_id.encode(dst);
    (item.payload.len() as u16).encode(dst);
    dst.put(item.payload);
    Ok(())
  }
}
