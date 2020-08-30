use flo_types::node::NodeGameStatus;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("unexpected node game status: {0:?}")]
  UnexpectedNodeGameStatus(NodeGameStatus),
  #[error("invalid node token")]
  InvalidNodeToken,
  #[error("not in game")]
  NotInGame,
  #[error("node connection rejected: {1} ({0:?})")]
  NodeConnectionRejected(flo_net::proto::flo_node::ClientConnectRejectReason, String),
  #[error("map checksum mismatch")]
  MapChecksumMismatch,
  #[error("unexpected controller packet")]
  UnexpectedControllerPacket,
  #[error("unexpected w3gs packet: {0:?}")]
  UnexpectedW3GSPacket(flo_w3gs::packet::Packet),
  #[error("slot not resolved")]
  SlotNotResolved,
  #[error("stream closed unexpectedly")]
  StreamClosed,
  #[error("set ping interval failed")]
  SetPingIntervalFailed,
  #[error("invalid selected node id: {0}")]
  InvalidSelectedNodeId(i32),
  #[error("invalid map info")]
  InvalidMapInfo,
  #[error("broadcast nodes config failed")]
  BroadcastNodesConfigFailed,
  #[error("ping node timeout")]
  PingNodeTimeout,
  #[error("invalid ping node reply")]
  InvalidPingNodeReply,
  #[error("Warcraft ||| not located")]
  War3NotLocated,
  #[error("connection request rejected by server: {0:?}")]
  ConnectionRequestRejected(crate::types::RejectReason),
  #[error("task cancelled")]
  TaskCancelled,
  #[error("lan: {0}")]
  Lan(#[from] flo_lan::error::Error),
  #[error("websocket: {0}")]
  Websocket(#[from] async_tungstenite::tungstenite::error::Error),
  #[error("W3GS: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("map: {0}")]
  War3Map(#[from] flo_w3map::error::Error),
  #[error("war3 data: {0}")]
  War3Data(#[from] flo_w3storage::error::Error),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("platform: {0}")]
  Platform(#[from] flo_platform::error::Error),
  #[error("packet conversion: {0}")]
  PacketConversion(#[from] s2_grpc_utils::result::Error),
  #[error("task failed to execute to completion: {0}")]
  TaskJoinError(#[from] tokio::task::JoinError),
  #[error("json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
