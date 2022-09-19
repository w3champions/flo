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
use flate2::write::ZlibEncoder;
use flate2::{Compression, Crc, CrcReader, CrcWriter};
use std::io::{Cursor, Read, Write};

use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};

use crate::constants::SUPPORTED_BLOCK_SIZE;
use crate::{error::*, Record};

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
  _total_size: usize,
  finished_block: usize,
}

impl<R> Blocks<R> {
  pub(crate) fn new(r: R, num_blocks: usize, total_size: usize) -> Self {
    Self {
      _total_size: total_size,
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
      _total_size: buf.remaining(),
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

    let mut buf: Vec<u8> = Vec::with_capacity(SUPPORTED_BLOCK_SIZE);
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
    let chain = std::io::Read::chain(
      Cursor::new(header_for_crc.encode_to_bytes()),
      self.r.by_ref(),
    );
    let mut r = CrcReader::new(chain);
    if let Err(e) = r.read_exact(&mut buf[0..BlockHeader::MIN_SIZE]) {
      return Some(Err(Error::ReadBlockHeader(e)));
    }

    let crc = r.crc().sum();
    let crc = (crc ^ (crc >> 16)) as u16;
    if crc != header.crc16_header {
      return Some(Err(Error::InvalidChecksum {
        subject: "header",
        expected: header.crc16_header,
        got: crc,
      }));
    }

    if header.decompressed_data_size != SUPPORTED_BLOCK_SIZE as u32 {
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
      return Some(Err(Error::InvalidChecksum {
        subject: "data",
        expected: header.crc16_compressed_data,
        got: crc,
      }));
    }

    self.finished_block = self.finished_block + 1;

    Some(Ok(Block {
      header,
      data: Bytes::from(buf),
    }))
  }
}

#[derive(Debug, BinEncode)]
pub struct Block {
  pub header: BlockHeader,
  pub data: Bytes,
}

pub struct BlocksEncoder<W> {
  current_data_len: usize,
  num_of_blocks: usize,
  num_of_uncompressed_bytes: usize,
  block_w: ZlibEncoder<CrcWriter<Vec<u8>>>,
  w: W,
}

impl<W> BlocksEncoder<W>
where
  W: Write,
{
  pub fn new(w: W) -> Self {
    Self {
      current_data_len: 0,
      num_of_blocks: 0,
      num_of_uncompressed_bytes: 0,
      block_w: Self::make_writer(),
      w,
    }
  }

  pub fn encode(&mut self, record: &Record) -> Result<usize> {
    let mut buf = record.encode_to_bytes().freeze();
    let len = buf.len();

    self.num_of_uncompressed_bytes += len;

    if self.current_data_len + len <= SUPPORTED_BLOCK_SIZE {
      self.block_w.write_all(&buf)?;
      self.current_data_len += len;

      if self.current_data_len == SUPPORTED_BLOCK_SIZE {
        self.finish_block()?;
      }
    } else {
      let current_remaining_len = SUPPORTED_BLOCK_SIZE - self.current_data_len;
      let slice = &buf[0..current_remaining_len];

      self.block_w.write_all(slice)?;
      self.current_data_len += slice.len();

      buf.advance(current_remaining_len);
      self.finish_block()?;

      while buf.has_remaining() {
        let slice_len = std::cmp::min(SUPPORTED_BLOCK_SIZE, buf.remaining());
        let slice = &buf[0..slice_len];

        self.block_w.write_all(slice)?;
        self.current_data_len += slice.len();

        buf.advance(slice_len);
        if self.current_data_len == SUPPORTED_BLOCK_SIZE {
          self.finish_block()?;
        }
      }
    }
    Ok(len)
  }

  pub fn finish(mut self) -> Result<Finished<W>> {
    if self.current_data_len < SUPPORTED_BLOCK_SIZE {
      let pad_len = SUPPORTED_BLOCK_SIZE - self.current_data_len;
      for _ in 0..pad_len {
        self.block_w.write_all(&[0])?;
      }
      self.current_data_len = SUPPORTED_BLOCK_SIZE;
    }
    self.finish_block()?;
    Ok(Finished {
      num_of_blocks: self.num_of_blocks,
      num_of_uncompressed_bytes: self.num_of_uncompressed_bytes,
      inner: self.w,
    })
  }

  fn finish_block(&mut self) -> Result<()> {
    let mut w = std::mem::replace(&mut self.block_w, Self::make_writer()).finish()?;
    let data_crc = w.crc().sum();
    let buf = w.into_inner();

    let mut header = BlockHeader {
      crc16_header: 0,
      crc16_compressed_data: 0,
      compressed_data_size: buf.len() as u32,
      decompressed_data_size: self.current_data_len as u32,
    };

    let mut header_crc = Crc::new();
    header_crc.update(&header.compressed_data_size.to_le_bytes());
    header_crc.update(&header.decompressed_data_size.to_le_bytes());
    header_crc.update(&[0, 0, 0, 0]); // crc16_header, crc16_compressed_data
    let header_crc = header_crc.sum();

    header.crc16_header = (header_crc ^ (header_crc >> 16)) as u16;
    header.crc16_compressed_data = (data_crc ^ (data_crc >> 16)) as u16;

    let mut header_buf = [0_u8; BlockHeader::MIN_SIZE];
    header.encode(&mut header_buf.as_mut_slice());
    self.w.write_all(&header_buf)?;
    self.w.write_all(&buf)?;

    self.current_data_len = 0;
    self.num_of_blocks += 1;
    Ok(())
  }

  fn make_writer() -> ZlibEncoder<CrcWriter<Vec<u8>>> {
    ZlibEncoder::new(
      CrcWriter::new(Vec::with_capacity(SUPPORTED_BLOCK_SIZE)),
      Compression::new(1),
    )
  }
}

#[derive(Debug)]
pub struct Finished<W> {
  pub num_of_blocks: usize,
  pub num_of_uncompressed_bytes: usize,
  pub inner: W,
}

#[test]
fn test_block() {
  use crate::header::Header;
  let bytes = flo_util::sample_bytes!("replay", "3506801.w3g");
  let mut buf = bytes.as_slice();
  let header = Header::decode(&mut buf).unwrap();
  dbg!(&header);

  let mut total_size = 0;
  let blocks = Blocks::from_buf(buf, header.num_blocks as usize);
  for block in blocks {
    let block = block.unwrap();
    total_size += block.data.len();
    dbg!(block.header);
  }
  dbg!(total_size);
}
