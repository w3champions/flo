use parking_lot::RwLock;
use s2_grpc_utils::S2ProtoEnum;
use s2_grpc_utils::S2ProtoUnpack;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

use flo_net::packet::OptionalFieldExt;
use flo_net::proto::flo_node as proto;

use crate::error::*;
use crate::net::PlayerSender;

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
pub struct PlayerToken([u8; 16]);

impl PlayerToken {
  pub fn new() -> Self {
    let uuid = Uuid::new_v4();
    Self(*uuid.as_bytes())
  }

  pub fn to_vec(&self) -> Vec<u8> {
    self.0.to_vec()
  }
}

#[derive(Debug)]
pub struct PendingPlayer {
  pub player_id: i32,
  pub game_id: i32,
}

impl PendingPlayer {}

#[derive(Debug)]
pub struct ConnectedPlayer {
  pub token: PlayerToken,
  pub game_id: i32,
  pub game_session: Arc<RwLock<GameSession>>,
  pub sender: PlayerSender,
}

#[derive(Debug)]
pub struct GameSession {
  pub game_id: i32,
  pub status: GameStatus,
  pub slots: Vec<GameSlot>,
  pub created_at: SystemTime,
}

impl GameSession {
  pub fn new(game: proto::Game) -> Result<Self> {
    Ok(GameSession {
      game_id: game.id,
      status: GameStatus::Waiting,
      slots: S2ProtoUnpack::unpack(game.slots)?,
      created_at: SystemTime::now(),
    })
  }
}

#[derive(Debug)]
pub struct GameSlot {
  id: u32,
  settings: GameSlotSettings,
  player: GamePlayer,
  client_status: GameClientStatus,
  sender: Option<PlayerSender>,
  disconnected_at_ms: Option<u32>,
}

impl S2ProtoUnpack<proto::GameSlot> for GameSlot {
  fn unpack(value: proto::GameSlot) -> Result<Self, s2_grpc_utils::result::Error> {
    Ok(GameSlot {
      id: value.id,
      settings: GameSlotSettings::unpack(value.settings)?,
      player: GamePlayer::unpack(value.player)?,
      client_status: GameClientStatus::unpack(value.client_status)?,
      sender: None,
      disconnected_at_ms: None,
    })
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::SlotSettings))]
pub struct GameSlotSettings {
  team: u32,
  color: u32,
  computer: Computer,
  handicap: u32,
  race: Race,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_common::Race))]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_common::Computer))]
pub enum Computer {
  Easy = 0,
  Normal = 1,
  Insane = 2,
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::GamePlayer))]
pub struct GamePlayer {
  pub player_id: i32,
  pub name: String,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_node::GameStatus))]
pub enum GameStatus {
  Waiting,
  Running,
  Ended,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_node::GameClientStatus))]
pub enum GameClientStatus {
  Pending,
  Connected,
  Loading,
  Loaded,
  Left,
  Disconnected,
}
