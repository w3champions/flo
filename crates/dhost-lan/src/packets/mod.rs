use dhost_util::binary::*;
use dhost_w3gs::{constants::GameFlags, packets::GameSettings};
use std::time::SystemTime;

#[derive(Debug)]
pub struct GameInfo {
  pub name: CString,
  pub flags: GameFlags,
  pub settings: GameSettings,
  pub secret: u32,
  pub create_time: SystemTime,
}

#[derive(Debug)]
struct GameData {
  name: CString,
  settings: GameSettings,
  slots_total: u32,
  flags: GameFlags,
  port: u16,
}

impl BinEncode for GameData {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    buf.put_slice(self.name.as_bytes_with_nul());
    buf.put_u8(0);
    self.settings.encode(buf);
    buf.put_u32_le(self.slots_total);
    buf.put_u32_le(self.flags.bits());
    buf.put_u16_le(self.port);
  }
}

impl BinDecode for GameData {
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    let name = buf.decode_cstring()?;
    buf.decode_zero_byte()?;

    let settings = GameSettings::decode(buf)?;
    let size = size_of::<u32>() /* slots_total */
      + size_of::<u32>()  /* flags */
      + size_of::<u16>()  /* port */;
    if buf.remaining() < size {
      return Err(BinDecodeError::Incomplete);
    }

    let slots_total = buf.get_u32_le();
    let flags = buf.get_u32_le();
    let flags = GameFlags::from_bits(flags)
      .ok_or_else(|| BinDecodeError::failure(format!("unknown game flags value: 0x{:x}", flags)))?;
    let port = buf.get_u16_le();
    Ok(Self {
      name,
      settings,
      slots_total,
      flags,
      port,
    })
  }
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
