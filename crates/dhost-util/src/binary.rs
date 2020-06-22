pub use crate::error::BinDecodeError;
pub use bytes::{Buf, BufMut, BytesMut};
pub use std::ffi::CString;
pub use std::mem::size_of;

pub trait BinEncode {
  fn encode<T: BufMut>(&self, buf: &mut T);
}

pub trait BinDecode
where
  Self: Sized,
{
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError>;
}

pub trait BinDecodeExt {
  /// Decodes a null terminated string
  fn decode_cstring(&mut self) -> Result<CString, BinDecodeError>;
  /// Decodes and a 0 byte
  fn decode_zero_byte(&mut self) -> Result<(), BinDecodeError>;
}

impl<T> BinDecodeExt for T
where
  T: Buf,
{
  fn decode_cstring(&mut self) -> Result<CString, BinDecodeError> {
    fn get_cstring_slice(slice: &[u8]) -> Result<Option<&[u8]>, BinDecodeError> {
      if slice.is_empty() {
        return Err(BinDecodeError::Incomplete);
      }
      let null_byte_pos = slice.iter().position(|b| *b == 0);
      match null_byte_pos {
        Some(pos) => Ok(Some(&slice[0..=pos])),
        None => Ok(None),
      }
    }

    let slice = self.bytes();

    match get_cstring_slice(slice)? {
      // cstring found in current slice
      Some(s) => {
        let out = s[..(s.len() - 1)].to_vec();
        let len = s.len();
        self.advance(len);
        Ok(CString::new(out).map_err(BinDecodeError::failure)?)
      }
      None => {
        let mut out = slice.to_vec();
        let len = slice.len();
        self.advance(len);
        loop {
          if !self.has_remaining() {
            return Err(BinDecodeError::Incomplete);
          }

          let slice = self.bytes();
          if let Some(s) = get_cstring_slice(slice)? {
            out.extend(&s[..(s.len() - 1)]);
            let len = s.len();
            self.advance(len);
            return Ok(CString::new(out).map_err(BinDecodeError::failure)?);
          } else {
            out.extend(slice);
            let len = slice.len();
            self.advance(len);
          }
        }
      }
    }
  }

  fn decode_zero_byte(&mut self) -> Result<(), BinDecodeError> {
    if !self.has_remaining() {
      return Err(BinDecodeError::Incomplete);
    }

    if self.get_u8() != 0 {
      return Err(BinDecodeError::failure("zero byte expected"));
    }

    Ok(())
  }
}

#[test]
fn test_ext_decode_cstring() {
  use bytes::buf::BufExt;

  let cstr = "1234567890".as_bytes();
  // continuous buffer
  let mut buf = "1234567890\0z".as_bytes();
  assert_eq!(buf.decode_cstring().unwrap().as_bytes(), cstr);
  assert_eq!(buf.remaining(), 1);

  // non-continuous buffer
  let mut buf = (&b"12"[..])
    .chain(&b"34"[..])
    .chain(&b"56"[..])
    .chain(&b"78"[..])
    .chain(&b"90"[..])
    .chain(&b"\0z"[..]);

  assert_eq!(buf.decode_cstring().unwrap().as_bytes(), cstr);
  assert_eq!(buf.remaining(), 1);
}
