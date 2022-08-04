use flate2::Crc;
use flo_util::binary::{BinDecode, BinEncode};

use crate::{
  block::{Blocks, BlocksEncoder},
  error::Result,
  header::GameVersion,
  Header, Record, RecordIter,
};
use std::io::{Read, Seek, SeekFrom, Write};

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

  pub fn into_records(self) -> RecordIter<R> {
    RecordIter::new(self.blocks)
  }
}

pub struct ReplayEncoder<W> {
  header: Header,
  w: BlocksEncoder<W>,
}

impl<W: Write + Seek> ReplayEncoder<W> {
  pub fn new(game_version: GameVersion, flags: u16, mut w: W) -> Result<Self> {
    w.seek(SeekFrom::Start(Header::MIN_SIZE as u64))?;
    Ok(Self {
      header: Header::new(game_version, flags),
      w: BlocksEncoder::new(w),
    })
  }

  pub fn encode_records<'a, I>(&mut self, iter: I) -> Result<()>
  where
    I: IntoIterator<Item = &'a Record>,
  {
    for r in iter {
      match *r {
        Record::TimeSlotFragment(ref slot) => {
          self.header.duration_ms += slot.0.time_increment_ms as u32;
        }
        Record::TimeSlot(ref slot) => {
          self.header.duration_ms += slot.time_increment_ms as u32;
        }
        _ => {}
      }
      self.w.encode(r)?;
    }
    Ok(())
  }

  pub fn finish(mut self) -> Result<()> {
    let blocks = self.w.finish()?;

    let mut w = blocks.inner;

    self.header.size_file = w.stream_position()? as u32;

    w.seek(SeekFrom::Start(0))?;
    let mut buf = [0_u8; Header::MIN_SIZE];

    self.header.num_blocks = blocks.num_of_blocks as u32;
    self.header.size_blocks = blocks.num_of_uncompressed_bytes as u32;

    self.header.encode(&mut buf.as_mut_slice());

    let mut crc = Crc::new();
    crc.update(&buf);
    self.header.crc = crc.sum();

    (&mut buf[(Header::MIN_SIZE - 4)..]).copy_from_slice(&self.header.crc.to_le_bytes());

    w.write_all(&buf)?;

    w.flush()?;

    Ok(())
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

#[test]
fn test_encode() {
  let path = flo_util::sample_path!("replay", "grubby_happy.w3g");
  let out_path = "../../target/gen.w3g";
  let d = ReplayDecoder::new(std::fs::File::open(&path).unwrap()).unwrap();
  let mut e = ReplayEncoder::new(
    GameVersion {
      version: 10032,
      build_number: 6110,
      ..Default::default()
    },
    0,
    std::fs::File::create(out_path).unwrap(),
  )
  .unwrap();
  let records = d.into_records().collect::<Result<Vec<_>, _>>().unwrap();
  e.encode_records(&records).unwrap();
  e.finish().unwrap();

  let mut v = vec![];

  let mut crc = Crc::new();
  for (i, b) in ReplayDecoder::new(std::fs::File::open(&path).unwrap())
    .unwrap()
    .into_blocks()
    .enumerate()
  {
    let data = b.unwrap().data;
    crc.update(&data);
    if i == 0 {
      std::fs::write("../../target/1.bin", &data).unwrap();
      v.push(data);
    }
  }
  let crc1 = crc.sum();

  let mut crc = Crc::new();
  for (i, b) in ReplayDecoder::new(std::fs::File::open(out_path).unwrap())
    .unwrap()
    .into_blocks()
    .enumerate()
  {
    let data = b.unwrap().data;
    crc.update(&data);
    if i == 0 {
      std::fs::write("../../target/2.bin", &data).unwrap();
      v.push(data);
    }
  }
  let crc2 = crc.sum();

  assert_eq!(crc1, crc2);

  // use bytes::Buf;
  // struct RecordIter<'a>(&'a [u8]);
  // impl<'a> Iterator for RecordIter<'a> {
  //   type Item = Result<Record>;

  //   fn next(&mut self) -> Option<Self::Item> {
  //     if !self.0.has_remaining() {
  //       return None
  //     }
  //     Some(Record::decode(&mut self.0).map_err(Into::into))
  //   }
  // }

  // for (i, (a, b)) in RecordIter(&v[0]).zip(RecordIter(&v[1])).enumerate() {
  //   let a = a.ok();
  //   let b = b.ok();
  //   dbg!(i);
  //   assert_eq!(a, b);
  // }
}
