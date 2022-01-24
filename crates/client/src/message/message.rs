use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::str::FromStr;

use flo_net::proto::flo_connect::{
  PacketGamePlayerLeave, PacketGamePlayerPingMapSnapshot, PacketGamePlayerPingMapSnapshotRequest,
  PacketGameSelectNode, PacketGameSelectNodeRequest, PacketGameStartReject, PacketGameStartRequest,
  PacketGameStarting, PacketPlayerPingMapUpdate,
};

use crate::error::{Error, Result};
use crate::observer::WatchGame;
use crate::ping::PingUpdate;
use crate::platform::{PlatformStateError, StartTestGame};
pub use flo_types::game::{
  DisconnectReason, MapDetail, MapForceOwned, MapPlayerOwned, PlayerSession, PlayerSessionUpdate,
  RejectReason,
};
use flo_types::game::{GameInfo, GameStatusUpdate, PlayerInfo, Slot, SlotSettings};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
  ReloadClientInfo,
  Connect(Connect),
  ListMaps,
  GetMapDetail(MapPath),
  GameSlotUpdateRequest(GameSlotUpdateRequest),
  GameSelectNodeRequest(PacketGameSelectNodeRequest),
  GamePlayerPingMapSnapshotRequest(PacketGamePlayerPingMapSnapshotRequest),
  ListNodesRequest,
  GameStartRequest(PacketGameStartRequest),
  StartTestGame(StartTestGame),
  KillTestGame,
  SetNodeAddrOverrides(SetNodeAddrOverrides),
  ClearNodeAddrOverrides,
  WatchGame(WatchGame),
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
  PlayerPingMapUpdate(PacketPlayerPingMapUpdate),
  GamePlayerPingMapSnapshot(PacketGamePlayerPingMapSnapshot),
  GameStartReject(PacketGameStartReject),
  GameStarting(PacketGameStarting),
  GameStarted(GameStarted),
  GameStartError(ErrorMessage),
  GameSlotClientStatusUpdate(ClientUpdateSlotClientStatus),
  GameStatusUpdate(GameStatusUpdate),
  GameDisconnect,
  SetNodeAddrOverridesError(ErrorMessage),
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
pub struct NodeList {
  pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize)]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub country_id: String,
  pub ping: Option<PingStats>,
}

#[derive(Debug, Serialize)]
pub struct GameStarted {
  pub game_id: i32,
  pub lan_game_name: String,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGameSlotUpdateRequest))]
pub struct GameSlotUpdateRequest {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot_settings: SlotSettings,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGameSlotUpdate))]
pub struct GameSlotUpdate {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot_settings: SlotSettings,
  pub player: Option<PlayerInfo>,
}

use crate::controller::SetNodeAddrOverrides;
pub use crate::node::stream::SlotClientStatusUpdate as ClientUpdateSlotClientStatus;
use flo_types::ping::PingStats;

#[derive(Debug, Serialize, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGamePlayerEnter))]
pub struct GamePlayerEnter {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot: Slot,
}
