use futures::lock::Mutex;
use futures::FutureExt;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::mpsc::Sender;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::{FloPacket, Frame};
use flo_net::proto::flo_node as proto;
use flo_net::stream::FloStream;
use flo_task::SpawnScope;
pub use flo_types::node::*;

use crate::error::*;
use crate::state::GlobalEvent;

mod host;
use crate::controller::ControllerServerHandle;
use crate::game::peer::PeerHandle;
use crate::state::event::GlobalEventSender;
use host::GameHost;

mod peer;

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
    g_event_sender: GlobalEventSender,
  ) -> Result<Self> {
    let scope = SpawnScope::new();
    let game_id = game.id;
    let (tx, mut rx) = GameEvent::channel(3);
    let slots: Vec<_> = Vec::<GameSlot>::unpack(game.slots)?
      .into_iter()
      .filter_map(PlayerSlot::from_game_slot)
      .collect();

    let mut scope_handle = scope.handle();
    let state = Arc::new(Mutex::new(State {
      game_id,
      g_event_sender,
      host: GameHost::new(game_id, &slots, tx.clone()),
      status: NodeGameStatus::Created,
      player_slots: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, slot))
        .collect(),
      tx,
      ctrl,
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
              if let Err(err) = Self::handle_event(&handle, event).await {
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
          NodeGameStatus::Ended => {
            guard
              .g_event_sender
              .send(GlobalEvent::GameEnded(game_id))
              .await
              .ok();
          }
          NodeGameStatus::Running => {
            guard.host.start_dispatch();
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
  ) -> Result<(), (FloStream, Error)> {
    use peer::PeerStream;

    let mut guard = self.0.lock().await;

    let peer_stream = if let Some(slot) = guard.player_slots.get_mut(&player_id) {
      if slot.sender.as_ref().is_some() {
        return Err((stream, Error::PlayerConnectionExists));
      }
      let (stream, handle) = PeerStream::new(player_id, stream)?;
      slot.sender = Some(handle);
      stream
    } else {
      return Err((stream, Error::PlayerNotFoundInGame));
    };
    let snapshot = guard.get_status_snapshot();
    guard
      .host
      .add_peer_stream(snapshot, peer_stream)
      .await
      .map_err(|err| (err.into(), Error::Cancelled))?;
    crate::metrics::CONNECTED_PLAYERS.inc();
    Ok(())
  }

  pub async fn update_player_client_status(
    &self,
    source: SlotClientStatusUpdateSource,
    player_id: i32,
    next_status: SlotClientStatus,
  ) -> Result<()> {
    let mut guard = self.0.lock().await;

    tracing::debug!(
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
        SlotClientStatus::Disconnected => {
          if let Some(_) = slot.sender.take() {
            tracing::debug!(player_id, "remove peer stream");
          }
          if slot.client_status == SlotClientStatus::Left {
            return Ok(());
          }
        }
        _ => {
          // send full status
          if let Some(frame) = send_all {
            if let Some(handle) = slot.sender.as_mut() {
              if let Err(_) = handle.send_frame(frame).await {
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
        guard.check_game_end().await;
      }
      SlotClientStatus::Disconnected => {
        guard.check_game_end().await;
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
          Some(tx.send_frame(frame.clone()).map(|_| ()))
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
  pub disconnected_at_ms: Option<u32>,
  pub sender: Option<PeerHandle>,
}

impl PlayerSlot {
  fn from_game_slot(slot: GameSlot) -> Option<PlayerSlot> {
    let player = slot.player?;
    Some(PlayerSlot {
      id: slot.id,
      settings: slot.settings,
      player,
      client_status: slot.client_status,
      disconnected_at_ms: None,
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
  async fn check_game_end(&mut self) {
    if self.player_slots.values().all(|slot| {
      slot.client_status == SlotClientStatus::Left
        || slot.client_status == SlotClientStatus::Disconnected
    }) {
      self.status = NodeGameStatus::Ended;
      tracing::debug!("all player left, end game");
      self
        .g_event_sender
        .send(GlobalEvent::GameEnded(self.game_id))
        .await
        .ok();
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
      && self
        .player_slots
        .values()
        .all(|slot| slot.client_status == SlotClientStatus::Loaded)
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
