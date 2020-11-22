use bs_diesel_utils::executor::ExecutorError;
use flo_state::RegistryError;
use thiserror::Error;
use tonic::Status;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Task cancelled")]
  TaskCancelled,
  #[error("Node not found")]
  NodeNotFound,
  #[error("Node not ready")]
  NodeNotReady,
  #[error("Node rejected connection: {addr:?}: {reason:?}")]
  NodeConnectionRejected {
    addr: std::net::SocketAddrV4,
    reason: flo_net::proto::flo_node::ControllerConnectRejectReason,
  },
  #[error("Unexpected node response")]
  NodeResponseUnexpected,
  #[error("Node request processing")]
  NodeRequestProcessing,
  #[error("Node request timeout")]
  NodeRequestTimeout,
  #[error("Node request cancelled")]
  NodeRequestCancelled,
  #[error("Invalid node address: {0}")]
  InvalidNodeAddress(String),
  #[error("Player stream closed")]
  PlayerStreamClosed,
  #[error("Player token expired")]
  PlayerTokenExpired,
  #[error("Join link expired")]
  JoinTokenExpired,
  #[error("You are not the host player")]
  PlayerNotHost,
  #[error("Player not found")]
  PlayerNotFound,
  #[error("Game not found")]
  GameNotFound,
  #[error("Only games with `Preparing` status are deletable")]
  GameNotDeletable,
  #[error("Invalid game data, please re-create")]
  GameDataInvalid,
  #[error("The game you are trying to join is full")]
  GameFull,
  #[error("Create game request already exists")]
  GameCreating,
  #[error("Create game request rejected: {0:?}")]
  GameCreateReject(flo_net::proto::flo_node::ControllerCreateGameRejectReason),
  #[error("Create game request rejected: {0:?}")]
  GameLeaveRejected(flo_net::proto::flo_node::UpdateSlotClientStatusRejectReason),
  #[error("Game node not selected")]
  GameNodeNotSelected,
  #[error("Slot update denied")]
  GameSlotUpdateDenied,
  #[error("Game already started")]
  GameStarted,
  #[error("Game not in starting state")]
  GameNotStarting,
  #[error("This map has no player slot")]
  MapHasNoPlayer,
  #[error("Player not in game")]
  PlayerNotInGame,
  #[error("Player already in game")]
  PlayerAlreadyInGame,
  #[error("Player slot not found")]
  PlayerSlotNotFound,
  #[error("Send to player channel timeout")]
  PlayerChannelSendTimeout,
  #[error("Player channel closed")]
  PlayerChannelClosed,
  #[error("Player source id is invalid")]
  PlayerSourceIdInvalid,
  #[error("Invalid player source state")]
  InvalidPlayerSourceState,
  #[error("Actor not found")]
  ActorNotFound,
  #[error("Too many players")]
  TooManyPlayers,
  #[error("Game has no player")]
  GameHasNoPlayer,
  #[error("Player colors are conflicting")]
  PlayerColorConflict,
  #[error("Invalid player team value")]
  PlayerTeamInvalid,
  #[error("Operation timeout")]
  Timeout(#[from] tokio::time::Elapsed),
  #[error("net: {0}")]
  Net(#[from] flo_net::error::Error),
  #[error("db error: {0}")]
  Db(#[from] bs_diesel_utils::result::DbError),
  #[error("db migration: {0}")]
  DbMigration(#[from] diesel_migrations::RunMigrationsError),
  #[error("json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("json web token: {0}")]
  JsonWebToken(#[from] jsonwebtoken::errors::Error),
  #[error("proto: {0}")]
  Proto(#[from] s2_grpc_utils::result::Error),
  #[error("gRPC transport: {0}")]
  GrpcTransport(#[from] tonic::transport::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for String {
  fn from(e: Error) -> Self {
    format!("{}", e)
  }
}

impl From<diesel::result::Error> for Error {
  fn from(e: diesel::result::Error) -> Self {
    Self::Db(e.into())
  }
}

impl From<Error> for Status {
  fn from(e: Error) -> Status {
    match e {
      e @ Error::GameNotFound
      | e @ Error::PlayerNotFound
      | e @ Error::MapHasNoPlayer
      | e @ Error::GameFull
      | e @ Error::GameNotDeletable
      | e @ Error::JoinTokenExpired => Status::invalid_argument(e.to_string()),
      e @ Error::PlayerTokenExpired => Status::unauthenticated(e.to_string()),
      Error::JsonWebToken(e) => Status::unauthenticated(e.to_string()),
      e => Status::internal(e.to_string()),
    }
  }
}

impl From<ExecutorError<diesel::result::Error>> for Error {
  fn from(e: ExecutorError<diesel::result::Error>) -> Self {
    match e {
      ExecutorError::Task(e) => Error::Db(e.into()),
      ExecutorError::Executor(e) => e.into(),
    }
  }
}

impl From<ExecutorError<Error>> for Error {
  fn from(e: ExecutorError<Error>) -> Self {
    match e {
      ExecutorError::Task(e) => e,
      ExecutorError::Executor(e) => Error::Db(e),
    }
  }
}

impl From<flo_state::error::Error> for Error {
  fn from(err: flo_state::error::Error) -> Self {
    match err {
      flo_state::error::Error::WorkerGone => Self::TaskCancelled,
    }
  }
}

impl From<flo_state::RegistryError> for Error {
  fn from(err: flo_state::RegistryError) -> Self {
    match err {
      RegistryError::RegistryGone => Self::TaskCancelled,
    }
  }
}

/// Helper trait to convert Option<Result<T>> to Result<T>
pub trait TaskCancelledExt<T> {
  fn or_cancelled(self) -> Result<T>;
}

impl<T, E> TaskCancelledExt<T> for Option<Result<T, E>>
where
  E: Into<Error>,
{
  fn or_cancelled(self) -> Result<T, Error> {
    match self {
      None => Err(Error::TaskCancelled),
      Some(Ok(v)) => Ok(v),
      Some(Err(e)) => Err(e.into()),
    }
  }
}
