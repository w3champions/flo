use crate::ping::PingError;
use flo_types::node::NodeGameStatus;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Unexpected node game status: {0:?}")]
  UnexpectedNodeGameStatus(NodeGameStatus),
  #[error("Invalid node token")]
  InvalidNodeToken,
  #[error("Not in game")]
  NotInGame,
  #[error("Node connection rejected: {1} ({0:?})")]
  NodeConnectionRejected(flo_net::proto::flo_node::ClientConnectRejectReason, String),
  #[error("Map checksum mismatch")]
  MapChecksumMismatch,
  #[error("Unexpected w3gs packet: {0:?}")]
  UnexpectedW3GSPacket(flo_w3gs::packet::Packet),
  #[error("Slot not resolved")]
  SlotNotResolved,
  #[error("Stream closed unexpectedly")]
  StreamClosed,
  #[error("Invalid map info")]
  InvalidMapInfo,
  #[error("Ping: {0}")]
  Ping(#[from] PingError),
  #[error("Warcraft ||| not located")]
  War3NotLocated,
  #[error("Connection request rejected by server: {0:?}")]
  ConnectionRequestRejected(crate::types::RejectReason),
  #[error("Local game info not yet received")]
  LocalGameInfoNotFound,
  #[error("Task cancelled: {0:?}")]
  TaskCancelled(anyhow::Error),
  #[error("Lan: {0}")]
  Lan(#[from] flo_lan::error::Error),
  #[error("Websocket: {0}")]
  Websocket(#[from] async_tungstenite::tungstenite::error::Error),
  #[error("W3GS: {0}")]
  W3GS(#[from] flo_w3gs::error::Error),
  #[error("Map: {0}")]
  War3Map(#[from] flo_w3map::error::Error),
  #[error("War3 data: {0}")]
  War3Data(#[from] flo_w3storage::error::Error),
  #[error("Net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("Platform: {0}")]
  Platform(#[from] flo_platform::error::Error),
  #[error("Packet conversion: {0}")]
  PacketConversion(#[from] s2_grpc_utils::result::Error),
  #[error("Task failed to execute to completion: {0}")]
  TaskJoinError(#[from] tokio::task::JoinError),
  #[error("Json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("Io: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<flo_state::error::Error> for Error {
  fn from(err: flo_state::error::Error) -> Self {
    match err {
      flo_state::error::Error::WorkerGone => Self::TaskCancelled(err.into()),
    }
  }
}

impl From<flo_state::RegistryError> for Error {
  fn from(err: flo_state::RegistryError) -> Self {
    match err {
      flo_state::RegistryError::RegistryGone => Self::TaskCancelled(err.into()),
    }
  }
}
