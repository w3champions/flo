use super::*;

pub trait FlagsImpl {
  type Repr: BinEncode + BinDecode + Copy + Clone;

  fn from_bits_truncate(bits: Self::Repr) -> Self;
}

#[derive(Debug, Copy, Clone)]
pub struct Flags<T>
where
  T: FlagsImpl,
{
  flags: T,
  raw_bits: T::Repr,
}

impl<T> BinEncode for Flags<T>
where
  T: FlagsImpl,
{
  fn encode<TBuf: BufMut>(&self, buf: &mut TBuf) {
    self.raw_bits.encode(buf)
  }
}
impl<T> BinDecode for Flags<T>
where
  T: FlagsImpl,
{
  const MIN_SIZE: usize = std::mem::size_of::<T::Repr>();
  const FIXED_SIZE: bool = true;
  fn decode<TBuf: Buf>(buf: &mut TBuf) -> Result<Self, BinDecodeError> {
    let raw_bits = BinDecode::decode(buf)?;
    Ok(Self {
      flags: T::from_bits_truncate(raw_bits),
      raw_bits,
    })
  }
}
