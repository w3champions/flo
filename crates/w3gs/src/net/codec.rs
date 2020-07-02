use flo_util::binary::*;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::Error;
use crate::protocol::packet::{Header, Packet};

#[derive(Debug)]
pub struct W3GSCodec {
  decode_state: DecoderState,
}

impl W3GSCodec {
  pub fn new() -> Self {
    Self {
      decode_state: DecoderState::DecodingHeader,
    }
  }
}

impl Decoder for W3GSCodec {
  type Item = Packet;
  type Error = Error;

  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    match self.decode_state {
      DecoderState::DecodingHeader => {
        if src.remaining() >= Header::MIN_SIZE {
          let header = Header::decode(src)?;
          let payload_len = header.get_payload_len()?;
          if src.remaining() >= payload_len {
            // payload received
            let packet = Packet::decode(header, src)?;
            Ok(Some(packet))
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
          let packet = Packet::decode(
            header.take().ok_or_else(|| Error::InvalidStateNoHeader)?,
            src,
          )?;
          self.decode_state = DecoderState::DecodingHeader;
          Ok(Some(packet))
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

impl Encoder<Packet> for W3GSCodec {
  type Error = Error;

  fn encode(&mut self, item: Packet, dst: &mut BytesMut) -> Result<(), Self::Error> {
    item.encode(dst);
    Ok(())
  }
}
