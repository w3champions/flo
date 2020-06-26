use flo_util::binary::*;
use std::ffi::CString;

use crate::constants::GameSettingFlags;

/// GameSettings stores the settings of a created game.
///
/// Flags:
///
///   Speed: (mask 0x00000003) cannot be combined
///     0x00000000 - Slow game speed
///     0x00000001 - Normal game speed
///     0x00000002 - Fast game speed
///   Visibility: (mask 0x00000F00) cannot be combined
///     0x00000100 - Hide terrain
///     0x00000200 - Map explored
///     0x00000400 - Always visible (no fog of war)
///     0x00000800 - Default
///   Observers/Referees: (mask 0x40003000) cannot be combined
///     0x00000000 - No Observers
///     0x00002000 - Observers on Defeat
///     0x00003000 - Additional players as observer allowed
///     0x40000000 - Referees
///   Teams/Units/Hero/Race: (mask 0x07064000) can be combined
///     0x00004000 - Teams Together (team members are placed at neighbored starting locations)
///     0x00060000 - Fixed teams
///     0x01000000 - Unit share
///     0x02000000 - Random hero
///     0x04000000 - Random races
///
/// Format:
///
///    (UINT32)     Flags
///    (UINT16)     Map width
///    (UINT16)     Map height
///    (UINT32)     Map xoro
///    (STRING)     Map path
///    (STRING)     Host name
///     (UINT8)[20] Map Sha1 hash
///
/// Encoded as a null terminated string where every even byte-value was
/// incremented by 1. So all encoded bytes are odd. A control-byte stores
/// the transformations for the next 7 bytes.
///
#[derive(Debug)]
pub struct GameSettings {
  pub game_setting_flags: GameSettingFlags,
  pub map_width: u16,
  pub map_height: u16,
  pub map_xoro: u32,
  pub map_path: CString,
  pub host_name: CString,
  pub map_sha1: [u8; 20],
}

impl GameSettings {
  fn get_encode_size(&self) -> usize {
    size_of::<u32>() /* Flags */
    + 1 /* 0x0 */
    + size_of::<u16>() /* Map width */
    + size_of::<u16>() /* Map height */
    + size_of::<u32>() /* Map xoro */
    + self.map_path.as_bytes().len() + 1 /* Map path */
    + self.host_name.as_bytes().len() + 1 /* Host name */
    + 1 /* 0x0 */
    + 20 /* Map Sha1 hash */
  }
}

impl BinEncode for GameSettings {
  fn encode<T: BufMut>(&self, buf: &mut T) {
    let len = self.get_encode_size();
    let mut stat_string_buf = Vec::<u8>::with_capacity(len);
    stat_string_buf.put_u32_le(self.game_setting_flags.bits());
    stat_string_buf.put_u8(0);
    stat_string_buf.put_u16_le(self.map_width);
    stat_string_buf.put_u16_le(self.map_height);
    stat_string_buf.put_u32_le(self.map_xoro);
    stat_string_buf.put(self.map_path.as_bytes());
    stat_string_buf.put_u8(0);
    stat_string_buf.put(self.host_name.as_bytes());
    stat_string_buf.put_u8(0);
    stat_string_buf.put_u8(0);
    stat_string_buf.put(&self.map_sha1 as &[u8]);
    let encoded = flo_util::stat_string::encode(&stat_string_buf);
    buf.put_slice(&encoded);
    buf.put_u8(0);
  }
}

impl BinDecode for GameSettings {
  fn decode<T: Buf>(buf: &mut T) -> Result<Self, BinDecodeError> {
    let min_len = size_of::<u32>() /* Flags */
      + 1 /* 0x0 */
      + size_of::<u16>() /* Map width */
      + size_of::<u16>() /* Map height */
      + size_of::<u32>() /* Map xoro */
      + "x.w3m".len() + 1 /* Map path */
      + "h".len() + 1 /* Host name */
      + 1 /* 0x0 */
      + 20 /* Map Sha1 hash */;

    if buf.remaining() < flo_util::stat_string::encoded_len(min_len) {
      return Err(BinDecodeError::incomplete());
    }

    let cstr = CString::decode(buf)?;
    let data = flo_util::stat_string::decode(cstr.as_bytes());

    let mut buf = &data[..];

    let game_setting_flags = buf.get_u32_le();
    let game_setting_flags = GameSettingFlags::from_bits(game_setting_flags).ok_or_else(|| {
      BinDecodeError::failure(format!(
        "unknown game flags value: 0x{:x}",
        game_setting_flags
      ))
    })?;

    buf.get_tag(b"\0")?;

    let map_width = buf.get_u16_le();
    let map_height = buf.get_u16_le();
    let map_xoro = buf.get_u32_le();
    let map_path = CString::decode(&mut buf)?;
    let host_name = CString::decode(&mut buf)?;

    if buf.remaining() != 1 + 20 {
      return Err(BinDecodeError::incomplete());
    }

    let zero = buf.get_u8();

    if zero != 0 {
      return Err(BinDecodeError::failure("zero byte expected"));
    }

    let mut map_sha1 = [0; 20];
    buf.copy_to_slice(&mut map_sha1);

    Ok(GameSettings {
      game_setting_flags,
      map_width,
      map_height,
      map_xoro,
      map_path,
      host_name,
      map_sha1,
    })
  }
}
