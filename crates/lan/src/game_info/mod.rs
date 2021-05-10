use std::path::Path;
use std::time::SystemTime;

use flo_util::binary::BinEncode;
use flo_util::binary::*;
use flo_util::{BinDecode, BinEncode};
use flo_w3gs::constants::GameFlags;
use flo_w3gs::protocol::game::GameSettings;
use flo_w3replay::W3Replay;

use crate::error::*;
use crate::proto;
use flo_w3gs::protocol::constants::GameSettingFlags;

#[derive(Debug, PartialEq, Clone)]
pub struct GameInfo {
  pub(crate) message_id: i32,
  pub game_id: String,
  pub create_time: SystemTime,
  pub secret: u32,
  pub name: CString,
  pub players_num: u8,
  pub players_max: u8,
  pub data: GameData,
}

impl GameInfo {
  /// Constructs a GameInfo using default settings
  pub fn new(
    id: i32,
    name: &str,
    map_path: &str,
    map_sha1: [u8; 20],
    map_checksum: u32,
  ) -> Result<Self> {
    let name = CString::new(name).map_err(|_| Error::NullByteInString)?;
    Ok(GameInfo {
      message_id: 0,
      game_id: id.to_string(),
      create_time: SystemTime::now(),
      secret: 0,
      name: name.clone(),
      players_num: 1,
      players_max: 24,
      data: GameData {
        name,
        _unknown_byte: 0,
        settings: GameSettings {
          game_setting_flags: GameSettingFlags::default(),
          unk_1: 0,
          map_width: 0,
          map_height: 0,
          map_checksum,
          map_path: CString::new(map_path).map_err(|_| Error::NullByteInString)?,
          host_name: CString::new("FLO").unwrap(),
          map_sha1,
        },
        slots_total: 24,
        flags: GameFlags::OBS_FULL,
        port: 16000,
      },
    })
  }

  pub fn from_replay<P: AsRef<Path>>(path: P) -> Result<Self> {
    use flo_w3replay::Record;
    for record in W3Replay::open(path)?.into_records() {
      match record? {
        Record::GameInfo(gameinfo) => {
          let name = CString::new("Replay").unwrap();
          let players_max = if gameinfo.player_count <= 24 {
            gameinfo.player_count as u8
          } else {
            return Err(Error::ReplayInvalidGameInfoRecord);
          };
          return Ok(Self {
            message_id: 0,
            game_id: "1".to_string(),
            create_time: SystemTime::now(),
            secret: 0xDDDDDDDD,
            name: name.clone(),
            players_num: 0,
            players_max,
            data: GameData {
              name,
              _unknown_byte: 0,
              settings: gameinfo.game_settings,
              slots_total: gameinfo.player_count,
              flags: gameinfo.game_flags,
              port: 16000,
            },
          });
        }
        _ => {}
      }
    }
    Err(Error::ReplayNoGameInfoRecord)
  }

  pub fn encode_to_bytes(&self) -> Result<Vec<u8>> {
    use prost::Message;

    let data = base64::encode(self.data.encode_to_bytes());
    let create_time = self
      .create_time
      .duration_since(SystemTime::UNIX_EPOCH)
      .map_err(|_| Error::InvalidGameInfo("encode: invalid create_time"))?
      .as_secs();
    let name_utf8 = String::from_utf8_lossy(self.name.as_bytes());
    let message = proto::GameInfo {
      name: name_utf8.to_string(),
      message_id: self.message_id,
      entries: vec![
        proto::GameInfoEntry {
          key: "players_num".to_string(),
          value: format!("{}", self.players_num),
        },
        proto::GameInfoEntry {
          key: "_name".to_string(),
          value: name_utf8.to_string(),
        },
        proto::GameInfoEntry {
          key: "players_max".to_string(),
          value: format!("{}", self.players_max),
        },
        proto::GameInfoEntry {
          key: "game_create_time".to_string(),
          value: format!("{}", create_time),
        },
        proto::GameInfoEntry {
          key: "_type".to_string(),
          value: format!("{}", 1),
        },
        proto::GameInfoEntry {
          key: "_subtype".to_string(),
          value: format!("{}", 0),
        },
        proto::GameInfoEntry {
          key: "game_secret".to_string(),
          value: format!("{}", self.secret),
        },
        proto::GameInfoEntry {
          key: "game_data".to_string(),
          value: format!("{}", data),
        },
        proto::GameInfoEntry {
          key: "game_id".to_string(),
          value: format!("{}", self.game_id),
        },
        proto::GameInfoEntry {
          key: "_flags".to_string(),
          value: format!("{}", 0),
        },
      ],
    };
    let len = message.encoded_len();
    let mut buf = Vec::with_capacity(len);
    message.encode(&mut buf)?;
    Ok(buf)
  }

  pub fn decode_bytes(bytes: &[u8]) -> Result<Self> {
    use prost::Message;
    use std::collections::HashMap;
    let message: proto::GameInfo = Message::decode(bytes)?;
    let entries: HashMap<&str, &str> = message
      .entries
      .iter()
      .map(|e| (e.key.as_ref(), e.value.as_ref()))
      .collect();
    let data_b64 = entries
      .get(&"game_data")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `game_data` entry"))?;
    let data_bytes = base64::decode(data_b64)?;
    let game_data = GameData::decode(&mut data_bytes.as_ref())?;
    let game_id = entries
      .get(&"game_id")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `game_id` entry"))?;
    let game_secret = entries
      .get(&"game_secret")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `game_secret` entry"))?;
    let secret = game_secret
      .parse()
      .map_err(|_| Error::InvalidGameInfo("invalid game_secret"))?;
    let game_create_time: u64 = entries
      .get(&"game_create_time")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `game_create_time` entry"))?
      .parse()
      .map_err(|_| Error::InvalidGameInfo("invalid game create timestamp: invalid format"))?;
    let create_time = SystemTime::UNIX_EPOCH
      .checked_add(std::time::Duration::from_secs(game_create_time))
      .ok_or_else(|| Error::InvalidGameInfo("invalid game create timestamp: overflow"))?;
    let name =
      CString::new(message.name).map_err(|_| Error::InvalidGameInfo("name contains null byte"))?;
    let players_num = entries
      .get(&"players_num")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `players_num` entry"))?
      .parse()
      .map_err(|_| Error::InvalidGameInfo("invalid `players_num`"))?;
    let players_max = entries
      .get(&"players_max")
      .cloned()
      .ok_or_else(|| Error::InvalidGameInfo("no `players_max` entry"))?
      .parse()
      .map_err(|_| Error::InvalidGameInfo("invalid `players_max`"))?;
    Ok(Self {
      message_id: message.message_id,
      game_id: game_id.to_string(),
      name,
      players_num,
      players_max,
      secret,
      create_time,
      data: game_data,
    })
  }

  pub fn set_port(&mut self, port: u16) {
    self.data.port = port;
  }
}

#[derive(Debug, BinEncode, BinDecode, PartialEq, Clone)]
pub struct GameData {
  pub name: CString,
  #[bin(eq = 0)]
  _unknown_byte: u8,
  pub settings: GameSettings,
  pub slots_total: u32,
  #[bin(bitflags(u32))]
  pub flags: GameFlags,
  pub port: u16,
}

#[test]
fn test_decode_protobuf_gameinfo() {
  use super::proto;
  use prost::Message;
  let bytes = include_bytes!("../../../../deps/wc3-samples/lan/gameinfo_melee.bin") as &[u8];
  let v: proto::GameInfo = Message::decode(bytes).unwrap();
  println!("{:#?}", v);
}

#[test]
fn test_decode_gameinfo_check() {
  use super::proto;
  use prost::Message;
  let bytes = include_bytes!("../../../../deps/wc3-samples/lan/gameinfo_check.bin") as &[u8];
  let v: proto::GameInfo = Message::decode(bytes).unwrap();
  println!("{:#?}", v);
}

#[test]
fn test_encode_gameinfo() {
  let bytes = include_bytes!("../../../../deps/wc3-samples/lan/gameinfo_melee.bin") as &[u8];
  let v = GameInfo::decode_bytes(&bytes).unwrap();
  println!("{:#?}", v);
  let encoded = v.encode_to_bytes().unwrap();
  std::fs::write(
    flo_util::sample_path!("lan", "gameinfo_encode.bin"),
    &encoded,
  )
  .unwrap();
  assert_eq!(GameInfo::decode_bytes(&encoded).unwrap(), v);
}

#[test]
fn test_decode_gamedata() {
  let mut bytes =
    include_bytes!("../../../../deps/wc3-samples/lan/gameinfo_w3c_ffa.data.bin") as &[u8];
  let data = GameData::decode(&mut bytes).unwrap();
  println!("{:#?}", data);
}

#[test]
fn test_decode_gamedata_2() {
  let bytes = base64::decode("YidiJ2InYgAAAQNJBwEBoQHxSQFXMYt5TZthcXMvKTMprWNvb3V5Y2G7eS93M20BMScxMQEByeVvKddX/4+NjWFvjTkDbz8b+wMLHcMAAgAAAAnAQgCk7g==").unwrap();
  let data = GameData::decode(&mut bytes.as_slice()).unwrap();
  println!("{:#?}", data);
}
