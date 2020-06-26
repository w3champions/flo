use super::{BinDecode, BinDecodeError, Buf};
use std::fmt::Debug;

pub trait BinBufExt {
  fn check_size(&mut self, size: usize) -> Result<(), BinDecodeError>;

  fn peek_u8(&mut self) -> Option<u8>;

  fn get_tag<T: AsRef<[u8]> + Debug>(&mut self, tag: T) -> Result<T, BinDecodeError>;

  fn advance_until<D: BinDelimiterMatcher>(&mut self, d: D) -> Result<usize, BinDecodeError>;

  fn advance_until_or_eof<D: BinDelimiterMatcher>(&mut self, d: D)
    -> Result<usize, BinDecodeError>;

  fn get_delimited_bytes<D: BinDelimiterMatcher>(
    &mut self,
    delim: D,
  ) -> Result<(Vec<u8>, u8), BinDecodeError>;

  fn get_delimited_string<D: BinDelimiterMatcher>(
    &mut self,
    delim: D,
  ) -> Result<(String, u8), BinDecodeError> {
    let (bytes, d) = self.get_delimited_bytes(delim)?;
    let s = String::from_utf8(bytes)
      .map_err(|e| BinDecodeError::failure(format!("can not parse bytes as utf8 string: {}", e)))?;
    Ok((s.to_string(), d))
  }

  fn get_delimited_from_str<D: BinDelimiterMatcher, T: std::str::FromStr>(
    &mut self,
    delim: D,
  ) -> Result<(T, u8), BinDecodeError>
  where
    <T as std::str::FromStr>::Err: std::fmt::Display,
  {
    self.get_delimited_string(delim).and_then(|(s, d)| {
      s.parse()
        .map(|v| (v, d))
        .map_err(|e| BinDecodeError::failure(format!("can not parse from str: {}", e)))
    })
  }

  fn get_repeated<T, I: From<Vec<T>>>(&mut self, len: usize) -> Result<I, BinDecodeError>
  where
    T: BinDecode;
}

impl<T> BinBufExt for T
where
  T: Buf,
{
  #[inline]
  fn check_size(&mut self, size: usize) -> Result<(), BinDecodeError> {
    if self.remaining() < size {
      return Err(BinDecodeError::incomplete());
    }
    Ok(())
  }

  #[inline]
  fn peek_u8(&mut self) -> Option<u8> {
    let bytes = self.bytes();
    assert!(bytes.len() > 0 || !self.has_remaining());
    bytes.get(0).cloned()
  }

  fn get_tag<TTag: AsRef<[u8]> + Debug>(&mut self, tag: TTag) -> Result<TTag, BinDecodeError> {
    let tag_slice = tag.as_ref();
    if self.remaining() < tag_slice.len() {
      return Err(BinDecodeError::incomplete());
    }

    for i in 0..(tag_slice.len()) {
      if self.get_u8() != tag_slice[i] {
        return Err(BinDecodeError::failure(format!(
          "bytes does not match tag `{:?}`",
          tag
        )));
      }
    }

    Ok(tag)
  }

  fn get_delimited_bytes<D: BinDelimiterMatcher>(
    &mut self,
    mut delim: D,
  ) -> Result<(Vec<u8>, u8), BinDecodeError> {
    let mut bytes = vec![];
    for _ in 0..(self.remaining()) {
      let b = self.get_u8();
      if delim.match_byte(b) {
        return Ok((bytes, b));
      }
      bytes.push(b);
    }
    Err(BinDecodeError::incomplete())
  }

  fn advance_until<D: BinDelimiterMatcher>(&mut self, mut d: D) -> Result<usize, BinDecodeError> {
    for i in 0..(self.remaining()) {
      let b = self.get_u8();
      if d.match_byte(b) {
        return Ok(i + 1);
      }
    }
    Err(BinDecodeError::incomplete())
  }

  fn advance_until_or_eof<D: BinDelimiterMatcher>(
    &mut self,
    mut d: D,
  ) -> Result<usize, BinDecodeError> {
    let len = self.remaining();
    for i in 0..len {
      let b = self.get_u8();
      if d.match_byte(b) {
        return Ok(i + 1);
      }
    }
    Ok(len)
  }

  fn get_repeated<TItem, I: From<Vec<TItem>>>(&mut self, len: usize) -> Result<I, BinDecodeError>
  where
    TItem: BinDecode,
  {
    self.check_size(TItem::MIN_SIZE * len)?;

    let mut items = Vec::with_capacity(len);
    for _ in 0..len {
      items.push(TItem::decode(self)?)
    }
    Ok(From::from(items))
  }
}

pub trait BinDelimiterMatcher {
  fn match_byte(&mut self, b: u8) -> bool;
}

impl BinDelimiterMatcher for u8 {
  fn match_byte(&mut self, b: u8) -> bool {
    *self == b
  }
}

impl<F> BinDelimiterMatcher for F
where
  F: FnMut(u8) -> bool,
{
  fn match_byte(&mut self, b: u8) -> bool {
    (*self)(b)
  }
}

pub trait BinDecodeErrorExt {
  fn context<T: std::fmt::Display>(self, ctx: T) -> Self;
}

impl<T> BinDecodeErrorExt for Result<T, BinDecodeError> {
  fn context<TContext: std::fmt::Display>(self, ctx: TContext) -> Self {
    self.map_err(|e| e.context(ctx))
  }
}
