use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("set ping interval failed")]
  SetPingIntervalFailed,
  #[error("invalid selected node id: {0}")]
  InvalidSelectedNodeId(i32),
  #[error("broadcast nodes config failed")]
  BroadcastNodesConfigFailed,
  #[error("ping node timeout")]
  PingNodeTimeout,
  #[error("invalid ping node reply")]
  InvalidPingNodeReply,
  #[error("Warcraft ||| not located")]
  War3NotLocated,
  #[error("connection request rejected by server: {0:?}")]
  ConnectionRequestRejected(crate::lobby::RejectReason),
  #[error("task failed to execute to completion: {0}")]
  TaskJoinError(#[from] tokio::task::JoinError),
  #[error("task cancelled")]
  TaskCancelled,
  #[error("event channel closed: {0}")]
  EventChannelClosed(&'static str),
  #[error("server not connected")]
  ServerNotConnected,
  #[error("json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  #[error("websocket: {0}")]
  Websocket(#[from] async_tungstenite::tungstenite::error::Error),
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
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
