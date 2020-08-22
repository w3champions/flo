use futures::lock::Mutex;
use s2_grpc_utils::S2ProtoEnum;
use s2_grpc_utils::S2ProtoUnpack;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Notify;

use flo_event::*;
use flo_net::packet::OptionalFieldExt;
use flo_net::proto::flo_node as proto;

use crate::error::*;
use crate::session::NodeEvent;
use tracing_futures::Instrument;

pub type GameEventSender = EventFromSender<NodeEvent, GameEvent>;

#[derive(Debug)]
pub struct GameEvent {
  pub game_id: i32,
  pub data: GameEventData,
}

#[derive(Debug)]
pub enum GameEventData {
  Activated,
  AllPlayerLoaded,
  AllPlayerLeft,
}

impl FloEvent for GameEvent {
  const NAME: &'static str = "GameEvent";
}

#[derive(Debug, Clone)]
pub struct GameSessionRef {
  state: Arc<Mutex<State>>,
}

impl GameSessionRef {
  pub fn new(event_sender: GameEventSender, game: proto::Game) -> Result<Self> {
    let slots = Vec::<GameSlot>::unpack(game.slots)?;
    let dropper = Arc::new(Notify::new());

    let state = Arc::new(Mutex::new(State {
      event_sender,
      game_id: game.id,
      status: GameStatus::Created,
      slots: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, slot))
        .collect(),
      created_at: SystemTime::now(),
      dropper,
    }));

    Ok(Self { state })
  }

  pub async fn update_player_client_status(
    &self,
    player_id: i32,
    next_status: SlotClientStatus,
  ) -> Result<()> {
    let mut guard = self.state.lock().await;
    let game_id = guard.game_id;

    let slot = guard
      .slots
      .get_mut(&player_id)
      .ok_or_else(|| Error::PlayerNotFoundInGame)?;

    if next_status == slot.client_status {
      return Ok(());
    }

    if next_status < slot.client_status && next_status != SlotClientStatus::Disconnected {
      return Err(Error::InvalidClientStatusTransition);
    }

    slot.client_status = next_status;

    match next_status {
      SlotClientStatus::Left | SlotClientStatus::Disconnected => {
        guard.check_remain_players().await;
      }
      SlotClientStatus::Pending => {}
      SlotClientStatus::Connected => {
        if guard.status == GameStatus::Created {
          guard.status = GameStatus::Waiting;
          guard
            .event_sender
            .send_or_log_as_error(GameEvent {
              game_id,
              data: GameEventData::Activated,
            })
            .await;
        }
      }
      SlotClientStatus::Loading => {}
      SlotClientStatus::Loaded => {
        if guard.status == GameStatus::Waiting {
          guard.check_game_ready().await;
        }
      }
    }

    Ok(())
  }
}

#[derive(Debug)]
struct State {
  pub event_sender: GameEventSender,
  pub game_id: i32,
  pub status: GameStatus,
  pub slots: HashMap<i32, GameSlot>,
  pub created_at: SystemTime,
  dropper: Arc<Notify>,
}

impl Drop for State {
  fn drop(&mut self) {
    self.dropper.notify();
  }
}

impl State {
  async fn check_remain_players(&mut self) {
    if self
      .slots
      .values()
      .all(|slot| slot.client_status == SlotClientStatus::Left)
    {
      self
        .event_sender
        .send_or_log_as_error(GameEvent {
          game_id: self.game_id,
          data: GameEventData::AllPlayerLeft,
        })
        .await;
    }
  }

  async fn check_game_ready(&mut self) {}
}

#[derive(Debug)]
pub struct GameSlot {
  id: u32,
  settings: GameSlotSettings,
  player: GamePlayer,
  client_status: SlotClientStatus,
  disconnected_at_ms: Option<u32>,
}

impl S2ProtoUnpack<proto::GameSlot> for GameSlot {
  fn unpack(value: proto::GameSlot) -> Result<Self, s2_grpc_utils::result::Error> {
    Ok(GameSlot {
      id: value.id,
      settings: GameSlotSettings::unpack(value.settings)?,
      player: GamePlayer::unpack(value.player)?,
      client_status: SlotClientStatus::unpack(value.client_status)?,
      disconnected_at_ms: None,
    })
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::SlotSettings))]
pub struct GameSlotSettings {
  team: i32,
  color: i32,
  computer: Computer,
  handicap: i32,
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

#[derive(Debug, Copy, Clone, S2ProtoEnum, PartialOrd, PartialEq)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_node::GameStatus))]
pub enum GameStatus {
  Created,
  Waiting,
  Running,
  Ended,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum, PartialOrd, PartialEq)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_node::SlotClientStatus))]
pub enum SlotClientStatus {
  Pending,
  Connected,
  Loading,
  Loaded,
  Disconnected,
  Left,
}
