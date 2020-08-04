use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::str::FromStr;

use flo_net::proto::flo_connect::{
  GameInfo, PacketGamePlayerEnter, PacketGamePlayerLeave, PacketGameSlotUpdate,
  PacketGameSlotUpdateRequest,
};

use crate::error::{Error, Result};

pub use crate::net::lobby::{DisconnectReason, PlayerSession, PlayerSessionUpdate, RejectReason};
use crate::state::FloState;
use crate::state::PlatformStateError;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
  ReloadClientInfo,
  Connect(Connect),
  ListMaps,
  GetMapDetail(MapPath),
  GameSlotUpdateRequest(PacketGameSlotUpdateRequest),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
  ClientInfo(ClientInfo),
  ReloadClientInfoError(ErrorMessage),
  PlayerSession(PlayerSession),
  ConnectRejected(ErrorMessage),
  Disconnect(Disconnect),
  ListMaps(MapList),
  ListMapsError(ErrorMessage),
  GetMapDetail(MapDetail),
  GetMapDetailError(ErrorMessage),
  CurrentGameInfo(GameInfo),
  GamePlayerEnter(PacketGamePlayerEnter),
  GamePlayerLeave(PacketGamePlayerLeave),
  GameSlotUpdate(PacketGameSlotUpdate),
  PlayerSessionUpdate(PlayerSessionUpdate),
}

impl FromStr for IncomingMessage {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    serde_json::from_str(s).map_err(Into::into)
  }
}

impl OutgoingMessage {
  pub fn serialize(&self) -> Result<String> {
    serde_json::to_string(self).map_err(Into::into)
  }
}

#[derive(Debug, Serialize)]
pub struct ClientInfo {
  pub version: Cow<'static, str>,
  pub war3_info: War3Info,
}

#[derive(Debug, Serialize)]
pub struct War3Info {
  pub located: bool,
  pub version: Option<String>,
  pub error: Option<PlatformStateError>,
}

#[derive(Debug, Serialize)]
pub struct ErrorMessage {
  pub message: String,
}

impl ErrorMessage {
  pub fn new<T: ToString>(v: T) -> Self {
    ErrorMessage {
      message: v.to_string(),
    }
  }
}

impl FloState {
  pub fn get_war3_info(&self) -> War3Info {
    let info = self.platform.map(|info| War3Info {
      located: true,
      version: info.version.clone().into(),
      error: None,
    });
    match info {
      Ok(info) => info,
      Err(e) => War3Info {
        located: false,
        version: None,
        error: Some(e),
      },
    }
  }
}

#[derive(Debug, Deserialize)]
pub struct Connect {
  pub token: String,
}

#[derive(Debug, Serialize)]
pub struct Disconnect {
  pub reason: DisconnectReason,
  pub message: String,
}

#[derive(Debug, Serialize)]
pub struct MapList {
  pub data: Value,
}

#[derive(Debug, Deserialize)]
pub struct MapPath {
  pub path: String,
}

#[derive(Debug, Serialize)]
pub struct MapDetail {
  pub path: String,
  pub sha1: String,
  pub crc32: u32,
  pub name: String,
  pub author: String,
  pub description: String,
  pub width: u32,
  pub height: u32,
  pub preview_jpeg_base64: String,
  pub suggested_players: String,
  pub num_players: usize,
  pub players: Vec<MapPlayerOwned>,
  pub forces: Vec<MapForceOwned>,
}

#[derive(Debug, Serialize)]
pub struct MapPlayerOwned {
  pub name: String,
  pub r#type: u32,
  pub race: u32,
  pub flags: u32,
}

#[derive(Debug, Serialize)]
pub struct MapForceOwned {
  pub name: String,
  pub flags: u32,
  pub player_set: u32,
}
