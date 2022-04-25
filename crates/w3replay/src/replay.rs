use flo_util::binary::BinDecode;

use crate::{
  block::{self, Blocks},
  error::Result,
  Header,
};
use std::io::Read;

pub struct ReplayDecoder<R> {
  header: Header,
  blocks: Blocks<R>,
}

impl<R: Read> ReplayDecoder<R> {
  pub fn new(mut r: R) -> Result<Self> {
    let mut header = [0_u8; Header::MIN_SIZE];
    r.read_exact(header.as_mut_slice())?;
    let header = Header::decode(&mut header.as_slice()).unwrap();
    let blocks = Blocks::new(
      r,
      header.num_blocks as _,
      (header.size_header + header.size_blocks) as _,
    );
    Ok(Self { header, blocks })
  }

  pub fn header(&self) -> &Header {
    &self.header
  }

  pub fn into_blocks(self) -> Blocks<R> {
    self.blocks
  }
}

#[test]
fn test_decode() {
  let r =
    ReplayDecoder::new(std::fs::File::open(flo_util::sample_path!("replay", "16k.w3g")).unwrap())
      .unwrap();

  let total_size: usize = r.into_blocks().map(|b| b.unwrap().data.len()).sum();
  assert_eq!(total_size, 40960)
}
