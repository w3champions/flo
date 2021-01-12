use bytes::buf::ext::Reader;
use bytes::Buf;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;

mod block;
mod constants;
mod header;
mod records;

pub mod error;
use block::Blocks;
pub use constants::*;
use error::*;
pub use header::Header;
pub use records::*;

#[derive(Debug)]
pub struct W3Replay<R> {
  header: Header,
  blocks: Blocks<R>,
}

impl W3Replay<BufReader<File>> {
  pub fn open<P: AsRef<Path>>(path: P) -> Result<W3Replay<BufReader<File>>> {
    use flo_util::binary::BinDecode;

    let f = File::open(path)?;
    let len = f.metadata()?.len() as usize;
    let mut r = BufReader::new(f);
    let mut buf: [u8; Header::MIN_SIZE] = [0; Header::MIN_SIZE];
    r.read_exact(&mut buf).map_err(Error::ReadHeader)?;
    let mut buf_slice = &buf[..];
    let header = Header::decode(&mut buf_slice).map_err(|e| e.context("header"))?;
    Ok(W3Replay {
      blocks: Blocks::new(r, header.num_blocks as usize, len - Header::MIN_SIZE),
      header,
    })
  }

  pub fn inspect<P: AsRef<Path>>(path: P) -> Result<(ReplayInfo, RecordIter<BufReader<File>>)> {
    let replay = Self::open(path)?;
    let mut game = None;
    let mut players = vec![];
    let mut slots = None;
    let mut iter = replay.into_records();
    while let Some(record) = iter.next() {
      match record? {
        Record::GameInfo(info) => game = Some(info),
        Record::SlotInfo(info) => slots = Some(info),
        Record::PlayerInfo(info) => players.push(info.player_info),
        Record::GameStart(_) => break,
        _ => {}
      }
    }
    Ok((
      ReplayInfo {
        game: game.ok_or_else(|| Error::NoGameInfoRecord)?,
        players,
        slots: slots.ok_or_else(|| Error::NoSlotInfoRecord)?,
      },
      iter,
    ))
  }
}

impl<B> W3Replay<Reader<B>>
where
  B: Buf,
{
  pub fn from_buf(mut buf: B) -> Result<W3Replay<Reader<B>>> {
    use flo_util::binary::BinDecode;
    let header = Header::decode(&mut buf).map_err(|e| e.context("header"))?;
    Ok(W3Replay {
      blocks: Blocks::from_buf(buf, header.num_blocks as usize),
      header,
    })
  }
}

impl<R> W3Replay<R> {
  pub fn into_records(self) -> RecordIter<R> {
    RecordIter::new(self.blocks)
  }
}

#[derive(Debug)]
pub struct ReplayInfo {
  pub game: GameInfo,
  pub players: Vec<PlayerInfo>,
  pub slots: SlotInfo,
}

#[test]
fn test_open() {
  let path = flo_util::sample_path!("replay", "16k.w3g");
  for r in W3Replay::open(&path).unwrap().into_records() {
    let r = r.unwrap();
    if r.type_id() == RecordTypeId::GameInfo {
      break;
    }
  }
}

#[test]
fn test_inspect() {
  let path = flo_util::sample_path!("replay", "grubby_happy.w3g");
  let (info, _) = W3Replay::inspect(&path).unwrap();
  dbg!(info);
}

#[test]
fn test_inspect2() {
  let path = flo_util::sample_path!("replay", "bn.w3g");
  let (info, _) = W3Replay::inspect(&path).unwrap();
  dbg!(info);
}

#[test]
fn test_inspect_computers() {
  let path = flo_util::sample_path!("replay", "computers.w3g");
  let (info, _) = W3Replay::inspect(&path).unwrap();
  dbg!(info);
}

#[test]
fn test_inspect_time() {
  use std::collections::BTreeMap;
  // let path = flo_util::sample_path!("replay", "spike.w3g");
  // let path = flo_util::sample_path!("replay", "Hippo_vs_Bido.w3g");
  // let path = r#"C:\Users\fluxx\Downloads\Replay_2020_12_17_0011.w3g"#;
  // let path = flo_util::sample_path!("replay", "fps_drop.w3g");
  // let path = flo_util::sample_path!("replay", "201227_ag3nt_spike.w3g");
  // let path = flo_util::sample_path!("replay", "grubby_happy.w3g");
  // let path = r#"C:\Users\fluxx\OneDrive\Documents\Warcraft III\BattleNet\298266\Replays\Autosaved\Multiplayer\Replay_2020_06_18_2318.w3g"#;
  let path = r#"C:\Users\fluxx\OneDrive\Documents\Warcraft III\BattleNet\298266\Replays\11111.w3g"#;
  let mut n = 0;
  let mut t = 0;
  let mut map = BTreeMap::new();
  for r in W3Replay::open(&path).unwrap().into_records() {
    let r = r.unwrap();
    if let Record::TimeSlot(r) = r {
      n += 1;
      (*map.entry(r.time_increment_ms).or_insert_with(|| 0)) += 1;
      t += r.time_increment_ms as u32;
    }
  }
  println!("t = {}, n = {}, map = {:?}", t, n, map);
}
