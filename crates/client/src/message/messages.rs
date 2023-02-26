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
use crate::platform::PlatformStateError;
pub use flo_types::game::{
  DisconnectReason, MapDetail, MapForceOwned, MapPlayerOwned, PlayerSession, PlayerSessionUpdate,
  RejectReason,
};
use flo_types::game::{GameInfo, GameStatusUpdate, PlayerInfo, Slot, SlotSettings};

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum IncomingMessage {
  ReloadClientInfo,
  Connect(Connect),
  Disconnect,
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
  WatchGameSetSpeed(WatchGameSetSpeed),
}

#[derive(Debug, Serialize, Clone)]
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
  WatchGame(WatchGameInfo),
  WatchGameError(ErrorMessage),
  WatchGameSetSpeedError(ErrorMessage),
  LanGameJoin(LanGameJoin),
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

#[derive(Debug, Serialize, Clone)]
pub struct ClientInfo {
  pub version: Cow<'static, str>,
  pub war3_info: War3Info,
}

#[derive(Debug, Serialize, Clone)]
pub struct War3Info {
  pub located: bool,
  pub error: Option<PlatformStateError>,
  pub version: Option<String>,
  pub user_data_path: Option<String>,
  pub installation_path: Option<String>,
  pub executable_path: Option<String>,
  pub ptr: bool,
}

#[derive(Debug, Serialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct Connect {
  pub token: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Disconnect {
  pub reason: DisconnectReason,
  pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MapList {
  pub data: Value,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MapPath {
  pub path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct NodeList {
  pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Node {
  pub id: i32,
  pub name: String,
  pub location: String,
  pub country_id: String,
  pub ping: Option<PingStats>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GameStarted {
  pub game_id: i32,
  pub lan_game_name: String,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoPack, Clone)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGameSlotUpdateRequest))]
pub struct GameSlotUpdateRequest {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot_settings: SlotSettings,
}

#[derive(Debug, Serialize, Deserialize, S2ProtoUnpack, Clone)]
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

#[derive(Debug, Serialize, S2ProtoUnpack, Clone)]
#[s2_grpc(message_type(flo_net::proto::flo_connect::PacketGamePlayerEnter))]
pub struct GamePlayerEnter {
  pub game_id: i32,
  pub slot_index: i32,
  pub slot: Slot,
}

#[derive(Debug, Serialize, Clone)]
pub struct WatchGameInfo {
  pub game_id: i32,
  pub delay_secs: Option<i64>,
  pub speed: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WatchGameSetSpeed {
  pub speed: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StartTestGame {
  pub name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LanGameJoin {
  pub lobby_name: String,
}
