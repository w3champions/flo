//! Replay Header
//!
//! [Section 2.0](http://w3g.deepnode.de/files/w3g_format.txt)
//!
//! offset | size/type | Description
//! -------+-----------+-----------------------------------------------------------
//! 0x0000 | 28 chars  | zero terminated string "Warcraft III recorded game\0x1A\0"
//! 0x001c |  1 dword  | fileoffset of first compressed data block (header size)
//!        |           |  0x40 for WarCraft III with patch <= v1.06
//!        |           |  0x44 for WarCraft III patch >= 1.07 and TFT replays
//! 0x0020 |  1 dword  | overall size of compressed file
//! 0x0024 |  1 dword  | replay header version:
//!        |           |  0x00 for WarCraft III with patch <= 1.06
//!        |           |  0x01 for WarCraft III patch >= 1.07 and TFT replays
//! 0x0028 |  1 dword  | overall size of decompressed data (excluding header)
//! 0x002c |  1 dword  | number of compressed data blocks in file
//! 0x0030 |  n bytes  | SubHeader (see section 2.1 and 2.2)
//!
//! - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//! 2.2 SubHeader for header version 1
//! - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//!
//! This header is used for all replays saved with WarCraft III patch version
//! v1.07 and above.
//!
//! offset | size/type | Description
//! -------+-----------+-----------------------------------------------------------
//! 0x0000 |  1 dword  | version identifier string reading:
//!        |           |  'WAR3' for WarCraft III Classic
//!        |           |  'W3XP' for WarCraft III Expansion Set 'The Frozen Throne'
//!        |           | (note that this string is saved in little endian format
//!        |           |  in the replay file)
//! 0x0004 |  1 dword  | version number (corresponds to patch 1.xx so far)
//! 0x0008 |  1  word  | build number (see section 2.3)
//! 0x000A |  1  word  | flags
//!        |           |   0x0000 for single player games
//!        |           |   0x8000 for multiplayer games (LAN or Battle.net)
//! 0x000C |  1 dword  | replay length in msec
//! 0x0010 |  1 dword  | CRC32 checksum for the header
//!        |           | (the checksum is calculated for the complete header
//!        |           |  including this field which is set to zero)
//!
//! Overall header size for version 1 is 0x44 bytes.

use flo_util::binary::*;
use flo_util::dword_string::DwordString;
use flo_util::{BinDecode, BinEncode};

#[derive(Debug, BinEncode, BinDecode)]
pub struct Header {
  #[bin(eq = SIGNATURE)]
  _sig: [u8; 28],
  #[bin(eq = 68)]
  pub size_header: u32,
  pub size_file: u32,
  #[bin(eq = 1)] // only version 1 is supported
  pub header_version: u32,
  pub size_blocks: u32,
  pub num_blocks: u32,
  pub game_version: GameVersion,
  pub flags: u16,
  pub duration_ms: u32,
  pub crc: u32,
}

#[derive(Debug, BinEncode, BinDecode)]
pub struct GameVersion {
  #[bin(eq = b"W3XP")]
  pub product: DwordString,
  pub version: u32,
  pub build_number: u16,
}

#[test]
fn test_header() {
  let bytes = flo_util::sample_bytes!("replay", "1.w3g");
  let mut buf = bytes.as_slice();
  let header = Header::decode(&mut buf).unwrap();
  dbg!(&header);
}
