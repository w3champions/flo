use dhost_util::binary::*;
use dhost_util::{BinDecode, BinEncode};
use dhost_w3gs::packets::GameSettings;
use std::time::SystemTime;

#[derive(Debug)]
pub struct GameInfo {
  pub name: CString,
  pub flags: u32,
  pub settings: GameSettings,
  pub secret: u32,
  pub create_time: SystemTime,
}

#[derive(Debug, BinEncode, BinDecode)]
struct GameData {
  name: CString,
  #[bin(eq = 0)]
  _unknown_byte: u8,
  settings: GameSettings,
  slots_total: u32,
  flags: u32,
  port: u16,
}

#[test]
fn test_decode_protobuf_gameinfo() {
  use super::proto;
  use prost::Message;
  let bytes = include_bytes!("../../samples/gameinfo_melee.bin") as &[u8];
  let v: proto::GameInfo = Message::decode(bytes).unwrap();
  println!("{:#?}", v);
}

#[test]
fn test_decode_gamedata() {
  let mut bytes = include_bytes!("../../samples/gameinfo_w3c_ffa.data.bin") as &[u8];
  let data = GameData::decode(&mut bytes).unwrap();
  println!("{:#?}", data);
}
