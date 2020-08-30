use futures::stream::StreamExt;
use s2_grpc_utils::S2ProtoEnum;
use std::collections::BTreeMap;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tracing_futures::Instrument;

use flo_net::packet::{Frame, PacketTypeId};
use flo_net::w3gs::{frame_to_w3gs, w3gs_to_frame};
use flo_task::{SpawnScope, SpawnScopeHandle};
use flo_w3gs::action::IncomingAction;
use flo_w3gs::protocol::action::PlayerAction;
use flo_w3gs::protocol::leave::LeaveReq;
use flo_w3gs::protocol::packet::*;

use crate::error::*;
use crate::game::{
  GameEvent, GameEventSender, GameSlot, SlotClientStatus, SlotClientStatusUpdateSource,
};

use super::action::ActionTickStream;
use crate::game::host::action::Tick;

#[derive(Debug)]
pub enum Message {
  Incoming {
    player_id: i32,
    slot_player_id: u8,
    frame: Frame,
  },
  PlayerConnect {
    player_id: i32,
    tx: Sender<Frame>,
  },
  PlayerDisconnect {
    player_id: i32,
  },
}

#[derive(Debug)]
pub struct Dispatcher {
  scope: SpawnScope,
  player_map: BTreeMap<i32, PlayerDispatchInfo>,
  start_tx: Option<oneshot::Sender<()>>,
}

impl Dispatcher {
  pub fn new(
    game_id: i32,
    slots: &[GameSlot],
    rx: Receiver<Message>,
    out_tx: GameEventSender,
  ) -> Self {
    let scope = SpawnScope::new();
    let state = State::new(slots);

    let (start_tx, start_rx) = oneshot::channel();

    tokio::spawn(
      Self::serve(state, rx, start_rx, out_tx, scope.handle())
        .instrument(tracing::debug_span!("worker", game_id)),
    );

    Dispatcher {
      scope,
      player_map: BTreeMap::new(),
      start_tx: Some(start_tx),
    }
  }

  pub fn start(&mut self) {
    if let Some(tx) = self.start_tx.take() {
      tracing::info!("game started.");
      tx.send(()).ok();
    }
  }

  async fn serve(
    mut state: State,
    mut rx: Receiver<Message>,
    mut start_rx: oneshot::Receiver<()>,
    mut out_tx: GameEventSender,
    mut scope: SpawnScopeHandle,
  ) {
    tokio::pin! {
      let dropped = scope.left();
    }

    let mut started = false;

    loop {
      tokio::select! {
        _ = &mut dropped => {
          break;
        }
        _ = &mut start_rx, if !started => {
          started = true;
        }
        Some(tick) = state.action_ticks.next(), if started => {
          state.time = state.time + tick.time_increment_ms;
          if let Err(err) = state.dispatch_action_tick(tick).await {
            tracing::error!("dispatch action tick: {}", err);
            break;
          }
        }
        next = rx.recv() => {
          if let Some(msg) = next {
            match state.dispatch(msg, &mut out_tx).await {
              Ok(_) => {},
              Err(err) => {
                tracing::error!("dispatch: {}", err);
              }
            }
          } else {
            break;
          }
        }
      }
    }
  }
}

#[derive(Debug)]
struct State {
  time: u32,
  ticks: usize,
  max_lag_time: u32,
  player_map: BTreeMap<i32, PlayerDispatchInfo>,
  action_ticks: ActionTickStream,
}

impl State {
  fn new(slots: &[GameSlot]) -> Self {
    State {
      time: 0,
      ticks: 0,
      max_lag_time: 0,
      player_map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, PlayerDispatchInfo::new(slot)))
        .collect(),
      action_ticks: ActionTickStream::new(crate::constants::GAME_DEFAULT_STEP_MS),
    }
  }

  pub async fn dispatch(
    &mut self,
    msg: Message,
    out_tx: &mut GameEventSender,
  ) -> Result<DispatchResult> {
    match msg {
      Message::Incoming {
        player_id,
        slot_player_id,
        frame,
      } => match frame.type_id {
        PacketTypeId::W3GS => {
          let res = self
            .dispatch_incoming_w3gs(player_id, slot_player_id, frame_to_w3gs(frame)?, out_tx)
            .await?;
          return Ok(res);
        }
        _ => {
          self.dispatch_incoming_flo(player_id, frame, out_tx).await?;
        }
      },
      Message::PlayerConnect { player_id, tx } => {
        self.get_player(player_id)?.tx.replace(tx);
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Connected,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      Message::PlayerDisconnect { player_id } => {
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Disconnected,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
        self.get_player(player_id)?.tx.take();
      }
    }

    Ok(DispatchResult::Continue)
  }

  pub async fn dispatch_incoming_w3gs(
    &mut self,
    player_id: i32,
    slot_player_id: u8,
    packet: Packet,
    out_tx: &mut GameEventSender,
  ) -> Result<DispatchResult> {
    use flo_w3gs::protocol::constants::PacketTypeId;
    use flo_w3gs::protocol::leave::{LeaveAck, PlayerLeft};
    let player = self.get_player(player_id)?;

    match packet.type_id() {
      PacketTypeId::LeaveReq => {
        let req: LeaveReq = packet.decode_simple()?;
        tracing::info!(player_id, "leave: {:?}", req.reason());
        player.send_w3gs(Packet::simple(LeaveAck)?).ok();
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Left,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
        player.disconnect();
        let pkt = Packet::simple(PlayerLeft {
          player_id: slot_player_id,
          reason: req.reason(),
        })?;
        self.broadcast(pkt)?;
      }
      PacketTypeId::OutgoingKeepAlive => {
        player.ack_tick();
      }
      PacketTypeId::OutgoingAction => {
        self
          .action_ticks
          .add_player_action(slot_player_id, packet.decode_payload()?);
      }
      id => {
        tracing::debug!("id = {:?}", id);
      }
    }

    Ok(DispatchResult::Continue)
  }

  pub async fn dispatch_incoming_flo(
    &mut self,
    player_id: i32,
    frame: Frame,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    flo_net::try_flo_packet! {
      frame => {
        p: flo_net::proto::flo_node::PacketClientUpdateSlotClientStatusRequest => {
          let status = SlotClientStatus::unpack_enum(p.status());
          out_tx
            .send(GameEvent::PlayerStatusChange(
              player_id,
              status,
              SlotClientStatusUpdateSource::Client
            ))
            .await
            .map_err(|_| Error::Cancelled)?;
        }
      }
    }
    Ok(())
  }

  pub async fn dispatch_action_tick(&mut self, tick: Tick) -> Result<()> {
    if !tick.actions.is_empty() {
      tracing::debug!(
        "actions: {:?}, tick = {}, time = {}",
        tick.actions.iter().map(|a| a.player_id).collect::<Vec<_>>(),
        self.ticks,
        self.time
      );
    }
    self.broadcast(Packet::with_payload(IncomingAction::from(tick))?)?;
    self.ticks = self.ticks + 1;
    Ok(())
  }

  pub fn broadcast(&mut self, packet: Packet) -> Result<()> {
    let frame = w3gs_to_frame(packet);
    let errors: Vec<_> = self
      .player_map
      .iter_mut()
      .filter_map(|(player_id, info)| {
        if info.connected() {
          info.send(frame.clone()).err().map(|err| (*player_id, err))
        } else {
          None
        }
      })
      .collect();

    if !errors.is_empty() {
      for (player_id, err) in errors {
        match err {
          PlayerSendError::Closed(_frame) => {
            tracing::info!(player_id, "removing player: stream broken");
            self.player_map.remove(&player_id);
          }
          PlayerSendError::Lagged => {
            tracing::info!(player_id, "lagged");
          }
          _ => {}
        }
      }
    }

    Ok(())
  }

  fn get_player(&mut self, player_id: i32) -> Result<&mut PlayerDispatchInfo> {
    if let Some(player) = self.player_map.get_mut(&player_id) {
      Ok(player)
    } else {
      tracing::error!(player_id, "unknown player id");
      return Err(Error::PlayerNotFoundInGame);
    }
  }
}

#[derive(Debug)]
struct PlayerDispatchInfo {
  lagging: bool,
  ticks: usize,
  overflow_frames: Vec<Frame>,
  tx: Option<Sender<Frame>>,
}

impl PlayerDispatchInfo {
  fn new(_slot: &GameSlot) -> Self {
    Self {
      lagging: false,
      overflow_frames: vec![],
      ticks: 0,
      tx: None,
    }
  }

  fn ack_tick(&mut self) {
    self.ticks = self.ticks + 1;
  }

  fn disconnect(&mut self) {
    self.tx.take();
  }

  fn connected(&self) -> bool {
    self.tx.is_some()
  }

  fn send_w3gs(&mut self, pkt: Packet) -> Result<(), PlayerSendError> {
    self.send(w3gs_to_frame(pkt))
  }

  fn send(&mut self, frame: Frame) -> Result<(), PlayerSendError> {
    if let Some(tx) = self.tx.as_mut() {
      if self.lagging {
        self.overflow_frames.push(frame);
        Err(PlayerSendError::Lagged)
      } else {
        match tx.try_send(frame) {
          Ok(_) => Ok(()),
          Err(TrySendError::Closed(frame)) => Err(PlayerSendError::Closed(frame)),
          Err(TrySendError::Full(frame)) => {
            self.lagging = true;
            self.overflow_frames.push(frame);
            Err(PlayerSendError::Lagged)
          }
        }
      }
    } else {
      Err(PlayerSendError::NotConnected(frame))
    }
  }
}

enum PlayerSendError {
  NotConnected(Frame),
  Closed(Frame),
  Lagged,
}

#[derive(Debug)]
enum DispatchResult {
  Continue,
  Action(PlayerAction),
  Lagged,
}
