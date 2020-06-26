use super::*;
use enumflags2::{BitFlags, RawBitFlags};

#[derive(Debug, Copy, Clone)]
pub struct Flags<T>
where
  T: RawBitFlags,
{
  flags: BitFlags<T>,
  raw_bits: T::Type,
}

impl<T> BinEncode for Flags<T>
where
  T: RawBitFlags,
  T::Type: BinEncode,
{
  fn encode<TBuf: BufMut>(&self, buf: &mut TBuf) {
    self.raw_bits.encode(buf)
  }
}
impl<T> BinDecode for Flags<T>
where
  T: RawBitFlags,
  T::Type: BinDecode,
{
  const MIN_SIZE: usize = std::mem::size_of::<T::Type>();
  const FIXED_SIZE: bool = true;
  fn decode<TBuf: Buf>(buf: &mut TBuf) -> Result<Self, BinDecodeError> {
    let raw_bits = BinDecode::decode(buf)?;
    Ok(Self {
      flags: BitFlags::from_bits_truncate(raw_bits),
      raw_bits,
    })
  }
}
