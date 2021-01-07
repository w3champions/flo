//! 3.0 [Data block header](http://w3g.deepnode.de/files/w3g_format.txt)
//!
//! Each compressed data block consists of a header followed by compressed data.
//! The first data block starts at the address denoted in the replay file header.
//! All following addresses are relative to the start of the data block header.
//! The decompressed data blocks append to a single continueous data stream
//! (disregarding the block headers). The content of this stream (see section 4) is
//! completely independent of the original block boundaries.
//!
//! offset | size/type | Description
//! -------+-----------+-----------------------------------------------------------
//! 0x0000 |  1  word  | size n of compressed data block (excluding header)
//! 0x0002 |  1  word  | size of decompressed data block (currently 8k)
//! 0x0004 |  1 dword  | unknown (probably checksum)
//! 0x0008 |  n bytes  | compressed data (decompress using zlib)
//!
//! To decompress one block with zlib:
//!  1. call 'inflate_init'
//!  2. call 'inflate' with Z_SYNC_FLUSH for the block
//!
//! The last block is padded with 0 bytes up to the 8K border. These bytes can
//! be disregarded.

use flate2::read::ZlibDecoder;
use flate2::CrcReader;
use std::io::{Cursor, Read};

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::error::*;

#[derive(Debug, BinDecode, BinEncode, Clone)]
pub struct BlockHeader {
  pub compressed_data_size: u32, // u16 -> u32 since version 10032
  pub decompressed_data_size: u32,
  pub crc16_header: u16,
  pub crc16_compressed_data: u16,
}

#[derive(Debug)]
pub struct Blocks<R> {
  r: R,
  num_blocks: usize,
  total_size: usize,
  finished_block: usize,
}

impl<R> Blocks<R> {
  pub(crate) fn new(r: R, num_blocks: usize, total_size: usize) -> Self {
    Self {
      total_size,
      r,
      num_blocks,
      finished_block: 0,
    }
  }
}

impl<B> Blocks<bytes::buf::Reader<B>>
where
  B: Buf,
{
  pub fn from_buf(buf: B, num_blocks: usize) -> Self {
    Self {
      total_size: buf.remaining(),
      r: buf.reader(),
      num_blocks,
      finished_block: 0,
    }
  }
}

impl<R> Iterator for Blocks<R>
where
  R: Read,
{
  type Item = Result<Block>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.finished_block == self.num_blocks {
      return None;
    }

    let mut buf: Vec<u8> = Vec::with_capacity(crate::constants::SUPPORTED_BLOCK_SIZE);
    buf.resize(buf.capacity(), 0);
    if let Err(err) = self.r.read_exact(&mut buf[0..BlockHeader::MIN_SIZE]) {
      return Some(Err(Error::ReadBlockHeader(err)));
    }

    let header = match BlockHeader::decode(&mut buf.as_slice()) {
      Ok(header) => header,
      Err(err) => return Some(Err(err.into())),
    };

    let header_for_crc = BlockHeader {
      crc16_header: 0,
      crc16_compressed_data: 0,
      ..header.clone()
    };
    let chain =
      std::io::Read::chain(Cursor::new(header_for_crc.encode_to_bytes()), self.r.by_ref());
    let mut r = CrcReader::new(chain);
    if let Err(e) = r.read_exact(&mut buf[0..BlockHeader::MIN_SIZE]) {
      return Some(Err(Error::ReadBlockHeader(e)));
    }

    let crc = r.crc().sum();
    let crc = (crc ^ (crc >> 16)) as u16;
    if crc != header.crc16_header {
      return Some(Err(Error::InvalidChecksum));
    }

    if header.decompressed_data_size != crate::constants::SUPPORTED_BLOCK_SIZE as u32 {
      return Some(Err(Error::UnsupportedBlockSize(
        header.decompressed_data_size as usize,
      )));
    }

    let r = CrcReader::new(self.r.by_ref().take(header.compressed_data_size as u64));
    let mut d = ZlibDecoder::new(r);
    if let Err(err) = d.read_exact(&mut buf) {
      return Some(Err(Error::ReadBlockHeader(err)));
    }

    let crc = d.get_ref().crc().sum();
    let crc = (crc ^ (crc >> 16)) as u16;
    if crc != header.crc16_compressed_data {
      return Some(Err(Error::InvalidChecksum));
    }

    self.finished_block = self.finished_block + 1;

    Some(Ok(Block {
      header,
      data: Bytes::from(buf),
    }))
  }
}

#[derive(Debug)]
pub struct Block {
  pub header: BlockHeader,
  pub data: Bytes,
}

#[test]
fn test_block() {
  use crate::header::Header;
  let bytes = flo_util::sample_bytes!("replay", "16k.w3g");
  let mut buf = bytes.as_slice();
  let header = Header::decode(&mut buf).unwrap();
  dbg!(&header);

  let blocks = Blocks::from_buf(buf, header.num_blocks as usize);
  for block in blocks {
    let _block = block.unwrap();
  }
}
