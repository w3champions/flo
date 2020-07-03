pub use crate::error::BinDecodeError;
pub use bytes::{Buf, BufMut, Bytes, BytesMut};
pub use std::ffi::CString;
pub use std::mem::size_of;

mod ext;
mod flags;
mod net;

pub use self::ext::*;
pub use self::flags::Flags;
pub use self::net::*;

pub trait BinEncode {
  fn encode<T: BufMut>(&self, buf: &mut T);
  fn encode_to_bytes(&self) -> BytesMut {
    let mut bytes = BytesMut::new();
    self.encode(&mut bytes);
    bytes
  }
}

pub trait BinDecode
where
  Self: Sized,
{
  const MIN_SIZE: usize = 0;
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError>;
}

macro_rules! impl_fixed {
  ($ty:ty, $put:ident, $get:ident) => {
    impl BinEncode for $ty {
      #[inline]
      fn encode<T: BufMut>(&self, buf: &mut T) {
        buf.$put(*self);
      }
    }
    impl BinDecode for $ty {
      const MIN_SIZE: usize = std::mem::size_of::<Self>();
      const FIXED_SIZE: bool = true;
      #[inline]
      fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
        Ok(buf.$get())
      }
    }
  };
}

impl_fixed!(u8, put_u8, get_u8);
impl_fixed!(u16, put_u16_le, get_u16_le);
impl_fixed!(i32, put_i32_le, get_i32_le);
impl_fixed!(u32, put_u32_le, get_u32_le);
impl_fixed!(f32, put_f32_le, get_f32_le);
impl_fixed!(f64, put_f64_le, get_f64_le);

impl BinEncode for bool {
  #[inline]
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_u8(if *self { 1 } else { 0 });
  }
}
impl BinDecode for bool {
  const MIN_SIZE: usize = 1;
  const FIXED_SIZE: bool = true;
  #[inline]
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    Ok(buf.get_u8() == 1)
  }
}

impl BinEncode for CString {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_slice(self.as_bytes_with_nul());
  }
}
impl BinDecode for CString {
  const MIN_SIZE: usize = 1;
  const FIXED_SIZE: bool = false;
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    fn get_cstring_slice(slice: &[u8]) -> Result<Option<&[u8]>, BinDecodeError> {
      if slice.is_empty() {
        return Err(BinDecodeError::incomplete());
      }
      let null_byte_pos = slice.iter().position(|b| *b == 0);
      match null_byte_pos {
        Some(pos) => Ok(Some(&slice[0..=pos])),
        None => Ok(None),
      }
    }

    let slice = buf.bytes();

    match get_cstring_slice(slice)? {
      // cstring found in current slice
      Some(s) => {
        let out = s[..(s.len() - 1)].to_vec();
        let len = s.len();
        buf.advance(len);
        Ok(CString::new(out).map_err(BinDecodeError::failure)?)
      }
      None => {
        let mut out = slice.to_vec();
        let len = slice.len();
        buf.advance(len);
        loop {
          if !buf.has_remaining() {
            return Err(BinDecodeError::incomplete());
          }

          let slice = buf.bytes();
          if let Some(s) = get_cstring_slice(slice)? {
            out.extend(&s[..(s.len() - 1)]);
            let len = s.len();
            buf.advance(len);
            return Ok(CString::new(out).map_err(BinDecodeError::failure)?);
          } else {
            out.extend(slice);
            let len = slice.len();
            buf.advance(len);
          }
        }
      }
    }
  }
}

impl<'a, T: BinEncode> BinEncode for &'a [T] {
  #[inline]
  fn encode<TBuf: BufMut>(&self, buf: &mut TBuf) {
    for v in self.iter() {
      v.encode(buf);
    }
  }
}

impl<T: BinEncode> BinEncode for Vec<T> {
  #[inline]
  fn encode<TBuf: BufMut>(&self, buf: &mut TBuf) {
    for v in self {
      v.encode(buf);
    }
  }
}

impl BinEncode for Bytes {
  #[inline]
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put(self.clone())
  }
}

#[test]
fn test_ext_decode_cstring() {
  use bytes::buf::BufExt;

  let cstr = "1234567890".as_bytes();
  // continuous buffer
  let mut buf = "1234567890\0z".as_bytes();
  assert_eq!(CString::decode(&mut buf).unwrap().as_bytes(), cstr);
  assert_eq!(buf.remaining(), 1);

  // non-continuous buffer
  let mut buf = (&b"12"[..])
    .chain(&b"34"[..])
    .chain(&b"56"[..])
    .chain(&b"78"[..])
    .chain(&b"90"[..])
    .chain(&b"\0z"[..]);

  assert_eq!(CString::decode(&mut buf).unwrap().as_bytes(), cstr);
  assert_eq!(buf.remaining(), 1);
}

#[test]
fn test_derive_decode_fixed_size() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: u32,
  }

  assert_eq!(T::MIN_SIZE, size_of::<u32>());
  assert!(T::FIXED_SIZE);

  let mut buf = BytesMut::new();
  buf.put_u32_le(1);

  let t = T::decode(&mut buf).unwrap();
  assert_eq!(t, T { _1: 1 })
}

#[test]
fn test_derive_decode_fixed_size_multi() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: u32,
    _2: u16,
    _3: u8,
  }

  assert_eq!(
    T::MIN_SIZE,
    size_of::<u32>() + size_of::<u16>() + size_of::<u8>()
  );
  assert!(T::FIXED_SIZE);

  let mut buf = BytesMut::new();
  buf.put_u32_le(1);
  buf.put_u16_le(2);
  buf.put_u8(3);

  let t = T::decode(&mut buf).unwrap();
  assert_eq!(
    t,
    T {
      _1: 1,
      _2: 2,
      _3: 3,
    }
  );
  assert!(!buf.has_remaining());
}

#[test]
fn test_derive_decode_variable_size() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: CString,
  }

  assert_eq!(T::MIN_SIZE, CString::MIN_SIZE);
  assert!(!T::FIXED_SIZE);

  let mut buf = BytesMut::new();
  buf.put_slice(b"123456\0");

  let t = T::decode(&mut buf).unwrap();
  assert_eq!(
    t,
    T {
      _1: CString::new("123456").unwrap()
    }
  );
  assert!(!buf.has_remaining());
}

#[test]
fn test_derive_decode_variable_size_muti() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: CString,
    _2: u32,
    _3: CString,
    _4: CString,
    _5: u16,
    _6: CString,
  }

  assert_eq!(
    T::MIN_SIZE,
    CString::MIN_SIZE * 4 + u32::MIN_SIZE + u16::MIN_SIZE
  );
  assert!(!T::FIXED_SIZE);

  let mut buf = BytesMut::new();
  buf.put_slice(b"1\0");
  buf.put_u32_le(2);
  buf.put_slice(b"3\0");
  buf.put_slice(b"4\0");
  buf.put_u16_le(5);
  buf.put_slice(b"6\0");

  let t = T::decode(&mut buf).unwrap();
  assert_eq!(
    t,
    T {
      _1: CString::new("1").unwrap(),
      _2: 2,
      _3: CString::new("3").unwrap(),
      _4: CString::new("4").unwrap(),
      _5: 5,
      _6: CString::new("6").unwrap(),
    }
  );
  assert!(!buf.has_remaining());
}

#[test]
#[should_panic(expected = "unexpected value for field `_1`, expected `2`, got `1`")]
fn test_derive_decode_eq() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    #[bin(eq = 2)]
    _1: u32,
  }

  let mut buf = BytesMut::new();
  buf.put_u32_le(1);

  T::decode(&mut buf).unwrap();
}

#[test]
fn test_derive_decode_slice() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: [u8; 5],
    _2: u32,
    _3: [CString; 2],
  }

  let mut buf = BytesMut::new();
  buf.put_slice(&[1, 2, 3, 4, 5]);
  buf.put_u32_le(2);
  buf.put_slice(b"1\0");
  buf.put_slice(b"2\0");

  assert_eq!(
    T::decode(&mut buf).unwrap(),
    T {
      _1: [1, 2, 3, 4, 5],
      _2: 2,
      _3: [CString::new("1").unwrap(), CString::new("2").unwrap()],
    }
  );
}

#[test]
fn test_derive_enum_decode() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  #[bin(enum_repr(u8))]
  enum V {
    #[bin(value = 1)]
    A,
    #[bin(value = 2)]
    B,
    UnknownValue(u8),
  }

  let mut buf = BytesMut::new();
  buf.put_slice(&[1, 2, 3]);

  assert_eq!(V::decode(&mut buf).unwrap(), V::A);
  assert_eq!(V::decode(&mut buf).unwrap(), V::B);
  assert_eq!(V::decode(&mut buf).unwrap(), V::UnknownValue(3));
}

#[test]
fn test_derive_decode_option() {
  use flo_codegen::BinDecode;
  #[derive(Debug, BinDecode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: [u8; 5],
    _2: u32,
    #[bin(condition = "_2 > 0")]
    _3: Option<[CString; 2]>,
  }

  let mut buf = BytesMut::new();
  buf.put_slice(&[1, 2, 3, 4, 5]);
  buf.put_u32_le(2);
  buf.put_slice(b"1\0");
  buf.put_slice(b"2\0");

  assert_eq!(
    T::decode(&mut buf).unwrap(),
    T {
      _1: [1, 2, 3, 4, 5],
      _2: 2,
      _3: Some([CString::new("1").unwrap(), CString::new("2").unwrap()]),
    }
  );

  let mut buf = BytesMut::new();
  buf.put_slice(&[1, 2, 3, 4, 5]);
  buf.put_u32_le(0);
  buf.put_slice(b"1\0");
  buf.put_slice(b"2\0");

  assert_eq!(
    T::decode(&mut buf).unwrap(),
    T {
      _1: [1, 2, 3, 4, 5],
      _2: 0,
      _3: None,
    }
  );
}

#[test]
fn test_derive_encode() {
  use flo_codegen::BinEncode;
  #[derive(Debug, BinEncode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: CString,
    _2: u32,
    _3: CString,
    _4: CString,
    _5: u16,
    _6: CString,
  }

  let mut bytes: Vec<u8> = vec![];
  T {
    _1: CString::new("1").unwrap(),
    _2: 2,
    _3: CString::new("3").unwrap(),
    _4: CString::new("4").unwrap(),
    _5: 5,
    _6: CString::new("6").unwrap(),
  }
  .encode(&mut bytes);

  let mut buf: Vec<u8> = vec![];
  buf.put_slice(b"1\0");
  buf.put_u32_le(2);
  buf.put_slice(b"3\0");
  buf.put_slice(b"4\0");
  buf.put_u16_le(5);
  buf.put_slice(b"6\0");

  assert_eq!(bytes, buf);
}

#[test]
fn test_derive_encode_slice() {
  use flo_codegen::BinEncode;
  #[derive(Debug, BinEncode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  struct T {
    _1: [u8; 5],
    _2: u32,
    _3: [CString; 2],
  }

  let mut bytes: Vec<u8> = vec![];
  T {
    _1: [1, 2, 3, 4, 5],
    _2: 2,
    _3: [CString::new("1").unwrap(), CString::new("2").unwrap()],
  }
  .encode(&mut bytes);

  let mut buf: Vec<u8> = vec![];
  buf.put_slice(&[1, 2, 3, 4, 5]);
  buf.put_u32_le(2);
  buf.put_slice(b"1\0");
  buf.put_slice(b"2\0");

  assert_eq!(bytes, buf);
}

#[test]
fn test_derive_enum_encode() {
  use flo_codegen::BinEncode;
  #[derive(Debug, Copy, Clone, BinEncode, PartialEq)]
  #[bin(mod_path = "crate::binary")]
  #[bin(enum_repr(u8))]
  enum V {
    #[bin(value = 1)]
    A,
    #[bin(value = 2)]
    B,
  }

  let mut bytes: Vec<u8> = vec![];
  V::A.encode(&mut bytes);
  V::B.encode(&mut bytes);

  let mut buf = BytesMut::new();
  buf.put_slice(&[1, 2]);

  assert_eq!(bytes, buf);
}
