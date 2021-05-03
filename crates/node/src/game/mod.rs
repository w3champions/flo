use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures::lock::Mutex;
use futures::FutureExt;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use tokio::sync::mpsc::Sender;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::{FloPacket, Frame, PacketTypeId};
use flo_net::proto::flo_node as proto;
use flo_net::stream::FloStream;
use flo_task::SpawnScope;
pub use flo_types::node::*;
use host::stream::PlayerStreamHandle;
pub use host::AckError;
use host::GameHost;

use crate::controller::ControllerServerHandle;
use crate::error::*;
use crate::observer::ObserverPublisherHandle;
use crate::state::event::GlobalEventSender;
use crate::state::GlobalEvent;
use flo_w3gs::constants::LeaveReason;

mod host;

#[derive(Debug)]
pub enum GameEvent {
  GameStatusChange(NodeGameStatus),
  PlayerStatusChange(i32, SlotClientStatus, SlotClientStatusUpdateSource),
}

pub type GameEventSender = Sender<GameEvent>;

impl FloEvent for GameEvent {
  const NAME: &'static str = "GameEvent";
}

#[derive(Debug)]
pub struct GameSession {
  scope: SpawnScope,
  game_id: i32,
  state: Arc<Mutex<State>>,
  shared: Arc<SharedState>,
}

impl GameSession {
  pub fn new(
    game: proto::Game,
    ctrl: ControllerServerHandle,
    obs: ObserverPublisherHandle,
    g_event_sender: GlobalEventSender,
  ) -> Result<Self> {
    let scope = SpawnScope::new();
    let game_id = game.id;
    let (tx, mut rx) = GameEvent::channel(32);
    let slots: Vec<_> = Vec::<GameSlot>::unpack(game.slots)?
      .into_iter()
      .filter_map(PlayerSlot::from_game_slot)
      .collect();

    let mut scope_handle = scope.handle();
    let state = Arc::new(Mutex::new(State {
      game_id,
      g_event_sender,
      host: GameHost::new(game_id, &slots, obs.clone(), tx.clone()),
      status: NodeGameStatus::Created,
      player_slots: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, slot))
        .collect(),
      tx,
      ctrl,
      obs,
    }));

    let shared = Arc::new(SharedState {
      game_id,
      created_at: SystemTime::now(),
    });

    let sess = Self {
      scope,
      game_id,
      state,
      shared,
    };

    tokio::spawn({
      let handle = sess.handle();
      async move {
        loop {
          tokio::select! {
            _ = scope_handle.left() => {
              break;
            }
            next = rx.recv() => {
              let event = match next {
                Some(event) => event,
                None => break,
              };
              if let Err(err) = Self::handle_event(&handle, event).instrument(tracing::info_span!("game", id = game_id)).await {
                tracing::error!("handle events: {}", err);
              }
            }
          }
        }
        tracing::debug!("exiting");
      }
        .instrument(tracing::debug_span!("event_worker", game_id))
    });

    Ok(sess)
  }

  pub fn handle(&self) -> GameSessionHandle {
    GameSessionHandle(self.state.clone())
  }

  async fn handle_event(handle: &GameSessionHandle, event: GameEvent) -> Result<()> {
    match event {
      GameEvent::PlayerStatusChange(player_id, status, source) => {
        handle
          .update_player_client_status(source, player_id, status)
          .await?;
      }
      GameEvent::GameStatusChange(status) => {
        let mut guard = handle.0.lock().await;
        let game_id = guard.game_id;
        guard.status = status;
        guard.broadcast_status_update(StatusUpdate::Full).await?;
        match status {
          NodeGameStatus::Running => {
            guard.host.start();
          }
          NodeGameStatus::Ended => {
            guard
              .g_event_sender
              .send(GlobalEvent::GameEnded(game_id))
              .await
              .ok();
          }
          _ => {}
        }
      }
    }
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct GameSessionHandle(Arc<Mutex<State>>);

impl GameSessionHandle {
  pub async fn register_player_stream(
    &self,
    player_id: i32,
    stream: FloStream,
  ) -> Result<(), (Option<FloStream>, Error)> {
    use host::stream::PlayerStream;

    let mut guard = self.0.lock().await;
    {
      let slot = if let Some(v) = guard.player_slots.get_mut(&player_id) {
        v
      } else {
        return Err((stream.into(), Error::PlayerNotFoundInGame));
      };

      if slot.sender.is_some() {
        return Err((stream.into(), Error::PlayerConnectionExists));
      }

      match slot.client_status {
        SlotClientStatus::Pending
        | SlotClientStatus::Connected
        | SlotClientStatus::Disconnected => {}
        other => {
          return Err((stream.into(), Error::InvalidPlayerSlotClientStatus(other)));
        }
      };
    };

    let stream = PlayerStream::new(player_id, stream);
    let snapshot = guard.get_status_snapshot();
    let sender = guard
      .host
      .register_player_stream(stream, snapshot)
      .await
      .map_err(|err| (None, err))?;
    guard
      .player_slots
      .get_mut(&player_id)
      .map(|slot| slot.sender.replace(sender));
    Ok(())
  }

  pub async fn retry_shutdown(
    &self,
    player_id: i32,
    leave_reason: Option<LeaveReason>,
    stream: &mut FloStream,
  ) -> Result<(), Error> {
    let mut guard = self.0.lock().await;
    guard
      .host
      .notify_player_shutdown(player_id, leave_reason)
      .await?;
    stream
      .send_frame(Frame::new_empty(PacketTypeId::ClientShutdownAck))
      .await?;
    stream.flush().await?;
    Ok(())
  }

  pub async fn update_player_client_status(
    &self,
    source: SlotClientStatusUpdateSource,
    player_id: i32,
    next_status: SlotClientStatus,
  ) -> Result<()> {
    let mut guard = self.0.lock().await;

    tracing::info!(
      player_id,
      "update player client status: {:?} => {:?}",
      source,
      next_status
    );

    let game_id = guard.game_id;
    let game_status = guard.status;

    let send_all = if source == SlotClientStatusUpdateSource::Node
      && next_status == SlotClientStatus::Connected
    {
      Some(guard.get_status_update_frame(game_id, StatusUpdate::Full)?)
    } else {
      None
    };

    let slot = guard
      .player_slots
      .get_mut(&player_id)
      .ok_or_else(|| Error::PlayerNotFoundInGame)?;

    if source == SlotClientStatusUpdateSource::Node {
      match next_status {
        // clean up sender
        SlotClientStatus::Disconnected | SlotClientStatus::Left => {
          if let Some(v) = slot.sender.take() {
            tracing::debug!(player_id, "remove stream: {}", v.stream_id());
          }
          if slot.client_status == SlotClientStatus::Left {
            return Ok(());
          }
        }
        _ => {
          // send full status
          if let Some(frame) = send_all {
            if let Some(handle) = slot.sender.as_mut() {
              if let Err(_) = handle.send(frame).await {
                return Err(Error::Cancelled);
              }
            }
          }
        }
      }
    }

    if next_status == slot.client_status {
      return Ok(());
    }

    if source == SlotClientStatusUpdateSource::Client {
      if next_status == SlotClientStatus::Disconnected {
        tracing::error!(
          player_id,
          "peer cannot send disconnected if it is disconnected"
        );
        return Err(Error::InvalidClientStatusTransition(
          slot.client_status,
          next_status,
        ));
      }

      match (slot.client_status, next_status) {
        // player joined lan game
        (SlotClientStatus::Connected, SlotClientStatus::Joined) => {}
        // player left the lan game
        (SlotClientStatus::Joined, SlotClientStatus::Connected) => {}
        (SlotClientStatus::Joined, SlotClientStatus::Loading) => {}
        (SlotClientStatus::Loading, SlotClientStatus::Loaded) => {}
        (_, SlotClientStatus::Left) => {}
        _ => {
          return Err(Error::InvalidClientStatusTransition(
            slot.client_status,
            next_status,
          ));
        }
      }
    }

    slot.client_status = next_status;

    match next_status {
      SlotClientStatus::Left => {
        if !guard.check_game_end().await {
          if guard.status == NodeGameStatus::Loading {
            guard.check_game_all_loaded().await;
          }
        }
      }
      SlotClientStatus::Disconnected => {
        if !guard.check_game_end().await {
          if guard.status == NodeGameStatus::Loading {
            guard.check_game_all_loaded().await;
          }
        }
      }
      SlotClientStatus::Pending => {}
      SlotClientStatus::Connected => {
        if guard.status == NodeGameStatus::Created {
          guard.status = NodeGameStatus::Waiting;
        }
      }
      SlotClientStatus::Joined => {
        if guard.status == NodeGameStatus::Waiting {
          // everyone has joined, start the game
          guard.check_game_all_joined().await;
        } else {
          // someone leave the lan game after the game started
          tracing::debug!(player_id, "rejoin");
        }
      }
      SlotClientStatus::Loading => {}
      SlotClientStatus::Loaded => {
        if guard.status == NodeGameStatus::Loading {
          guard.check_game_all_loaded().await;
        }
      }
    }

    // game status changed
    if guard.status != game_status {
      let next_status = guard.status;
      guard
        .tx
        .send(GameEvent::GameStatusChange(next_status))
        .await
        .map_err(|_| Error::Cancelled)?;
    }

    let update = if guard.status != game_status {
      StatusUpdate::Slot {
        player_id,
        status: next_status,
        game_status: Some(guard.status),
      }
    } else {
      StatusUpdate::Slot {
        player_id,
        status: next_status,
        game_status: None,
      }
    };

    guard.broadcast_status_update(update).await?;

    Ok(())
  }
}

#[derive(Debug)]
enum StatusUpdate {
  Slot {
    player_id: i32,
    status: SlotClientStatus,
    game_status: Option<NodeGameStatus>,
  },
  Full,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotClientStatusUpdateSource {
  Node,
  Controller,
  Client,
}

#[derive(Debug)]
struct State {
  game_id: i32,
  g_event_sender: GlobalEventSender,
  host: GameHost,
  status: NodeGameStatus,
  player_slots: BTreeMap<i32, PlayerSlot>,
  ctrl: ControllerServerHandle,
  tx: GameEventSender,
  obs: ObserverPublisherHandle,
}

impl State {
  fn get_status_update_frame(&self, game_id: i32, update: StatusUpdate) -> Result<Frame> {
    let frame = match update {
      StatusUpdate::Slot {
        player_id,
        status: slot_status,
        game_status,
      } => {
        if let Some(game_status) = game_status {
          tracing::debug!(
            game_id,
            player_id,
            "broadcast slot and game status update: slot_status = {:?}, game_status = {:?}",
            slot_status,
            game_status
          );
          use flo_net::proto::flo_node::PacketNodeGameStatusUpdate;
          let mut pkt = PacketNodeGameStatusUpdate {
            game_id: self.game_id,
            ..Default::default()
          };
          pkt.set_status(game_status.into_proto_enum());
          pkt
            .insert_updated_player_game_client_status_map(player_id, slot_status.into_proto_enum());
          pkt.encode_as_frame()?
        } else {
          tracing::debug!(
            game_id,
            player_id,
            "broadcast slot update: {:?}",
            slot_status
          );
          use flo_net::proto::flo_node::PacketClientUpdateSlotClientStatus;
          PacketClientUpdateSlotClientStatus {
            game_id: self.game_id,
            player_id,
            status: slot_status.into_proto_enum().into(),
          }
          .encode_as_frame()?
        }
      }
      StatusUpdate::Full => {
        use flo_net::proto::flo_node::PacketNodeGameStatusUpdate;
        tracing::debug!("broadcast full game update");
        let mut pkt = PacketNodeGameStatusUpdate {
          game_id: self.game_id,
          ..Default::default()
        };
        pkt.set_status(self.status.into_proto_enum());
        for slot in self.player_slots.values() {
          pkt.insert_updated_player_game_client_status_map(
            slot.player.player_id,
            slot.client_status.into_proto_enum(),
          );
        }
        pkt.encode_as_frame()?
      }
    };
    Ok(frame)
  }

  async fn broadcast_status_update(&mut self, update: StatusUpdate) -> Result<()> {
    let game_id = self.game_id;
    let frame = self.get_status_update_frame(game_id, update)?;

    let ctrl = self.ctrl.clone();
    let report = { ctrl.send(frame.clone()).map(|_| ()) };
    let broadcast = { self.broadcast(frame) };

    tokio::join!(broadcast, report);

    Ok(())
  }

  async fn broadcast(&mut self, frame: Frame) {
    use futures::stream::{FuturesUnordered, StreamExt};
    let f: FuturesUnordered<_> = self
      .player_slots
      .values_mut()
      .filter_map(|slot| {
        if let Some(tx) = slot.sender.as_mut() {
          Some(tx.send(frame.clone()).map(|_| ()))
        } else {
          None
        }
      })
      .collect();
    if f.is_empty() {
      return;
    }
    f.collect::<()>().await;
  }

  fn get_status_snapshot(&self) -> NodeGameStatusSnapshot {
    NodeGameStatusSnapshot::from(self)
  }
}

#[derive(Debug)]
pub struct PlayerSlot {
  pub id: u32,
  pub settings: GameSlotSettings,
  pub player: GamePlayer,
  pub client_status: SlotClientStatus,
  pub sender: Option<PlayerStreamHandle>,
}

impl PlayerSlot {
  fn from_game_slot(slot: GameSlot) -> Option<PlayerSlot> {
    let player = slot.player?;
    Some(PlayerSlot {
      id: slot.id,
      settings: slot.settings,
      player,
      client_status: slot.client_status,
      sender: None,
    })
  }
}

#[derive(Debug)]
struct SharedState {
  game_id: i32,
  created_at: SystemTime,
}

impl State {
  async fn check_game_end(&mut self) -> bool {
    if self.player_slots.values().all(|slot| {
      (slot.client_status == SlotClientStatus::Left
        || slot.client_status == SlotClientStatus::Disconnected)
        || slot.settings.team == 24
    }) {
      self.status = NodeGameStatus::Ended;
      tracing::debug!("all player left, end game");
      self
        .g_event_sender
        .send(GlobalEvent::GameEnded(self.game_id))
        .await
        .ok();
      self.obs.remove_game(self.game_id);
      true
    } else {
      false
    }
  }

  async fn check_game_all_joined(&mut self) {
    if self
      .player_slots
      .values()
      .all(|slot| slot.client_status == SlotClientStatus::Joined)
    {
      tracing::debug!("all joined");
      self.status = NodeGameStatus::Loading;
    }
  }

  async fn check_game_all_loaded(&mut self) {
    if self.status == NodeGameStatus::Loading
      && self.player_slots.values().all(|slot| {
        [
          SlotClientStatus::Loaded,
          SlotClientStatus::Disconnected,
          SlotClientStatus::Left,
        ]
        .contains(&slot.client_status)
      })
    {
      self.status = NodeGameStatus::Running;
      tracing::debug!("all loaded");
    }
  }
}

#[derive(Debug)]
pub struct GameSlot {
  id: u32,
  settings: GameSlotSettings,
  player: Option<GamePlayer>,
  client_status: SlotClientStatus,
}

impl S2ProtoUnpack<proto::GameSlot> for GameSlot {
  fn unpack(value: proto::GameSlot) -> Result<Self, s2_grpc_utils::result::Error> {
    Ok(GameSlot {
      id: value.id,
      settings: GameSlotSettings::unpack(value.settings)?,
      player: Option::<GamePlayer>::unpack(value.player)?,
      client_status: SlotClientStatus::unpack(value.client_status)?,
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
#[repr(i32)]
pub enum Race {
  Human = 0,
  Orc = 1,
  NightElf = 2,
  Undead = 3,
  Random = 4,
}

#[derive(Debug, Copy, Clone, S2ProtoEnum)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_common::Computer))]
#[repr(i32)]
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
  pub ban_list: Vec<PlayerBanType>,
}

impl<'a> From<&'a State> for NodeGameStatusSnapshot {
  fn from(state: &'a State) -> Self {
    NodeGameStatusSnapshot {
      game_id: state.game_id,
      game_status: state.status,
      player_game_client_status_map: state
        .player_slots
        .values()
        .map(|slot| (slot.player.player_id, slot.client_status))
        .collect(),
    }
  }
}

#[derive(Debug, Copy, Clone, S2ProtoEnum, PartialEq)]
#[s2_grpc(proto_enum_type(flo_net::proto::flo_node::PlayerBanType))]
#[repr(i32)]
pub enum PlayerBanType {
  Chat = 0,
}
