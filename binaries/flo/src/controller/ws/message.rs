use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::str::FromStr;

use flo_net::proto::flo_connect::{
  PacketGamePlayerLeave, PacketGamePlayerPingMapSnapshot, PacketGamePlayerPingMapSnapshotRequest,
  PacketGamePlayerPingMapUpdate, PacketGameSelectNode, PacketGameSelectNodeRequest,
  PacketGameStartReject, PacketGameStartRequest, PacketGameStarting,
};

use crate::error::{Error, Result};
use crate::node::PingUpdate;
use crate::platform::PlatformStateError;
pub use crate::types::{DisconnectReason, PlayerSession, PlayerSessionUpdate, RejectReason};
use crate::types::{GameInfo, GameStatusUpdate, Slot, SlotSettings};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
  ReloadClientInfo,
  Connect(Connect),
  ListMaps,
  GetMapDetail(MapPath),
  GameSlotUpdateRequest(GameSlotUpdate),
  GameSelectNodeRequest(PacketGameSelectNodeRequest),
  GamePlayerPingMapSnapshotRequest(PacketGamePlayerPingMapSnapshotRequest),
  ListNodesRequest,
  GameStartRequest(PacketGameStartRequest),
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
  GamePlayerEnter(GamePlayerEnter),
  GamePlayerLeave(PacketGamePlayerLeave),
  GameSlotUpdate(GameSlotUpdate),
  PlayerSessionUpdate(PlayerSessionUpdate),
  ListNodes(NodeList),
  PingUpdate(PingUpdate),
  GameSelectNode(PacketGameSelectNode),
  GamePlayerPingMapUpdate(PacketGamePlayerPingMapUpdate),
  GamePlayerPingMapSnapshot(PacketGamePlayerPingMapSnapshot),
  GameStartReject(PacketGameStartReject),
  GameStarting(PacketGameStarting),
  GameStarted(GameStarted),
  GameSlotClientStatusUpdate(ClientUpdateSlotClientStatus),
  GameStatusUpdate(GameStatusUpdate),
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

#[derive(Debug, Serialize)]
pub struct NodeList {
  pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize)]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub country_id: String,
  pub ping: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct GameStarted {
  pub game_id: i32,
  pub lan_game_name: String,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, S2ProtoUnpack)]
#[s2_grpc(message_type(
  flo_net::proto::flo_connect::PacketGameSlotUpdateRequest,
  flo_net::proto::flo_connect::PacketGameSlotUpdate
))]
pub struct GameSlotUpdate {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot_settings: SlotSettings,
}

pub use crate::node::stream::SlotClientStatusUpdate as ClientUpdateSlotClientStatus;

#[derive(Debug, Serialize, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGamePlayerEnter))]
pub struct GamePlayerEnter {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot: Slot,
}
