use futures::stream::StreamExt;
use parking_lot::Mutex;
use s2_grpc_utils::S2ProtoEnum;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use tracing_futures::Instrument;

use flo_net::packet::{Frame, PacketTypeId};
use flo_net::w3gs::{frame_to_w3gs, w3gs_to_frame};
use flo_task::{SpawnScope, SpawnScopeHandle};
use flo_w3gs::action::IncomingAction;
use flo_w3gs::protocol::action::{OutgoingAction, PlayerAction};
use flo_w3gs::protocol::leave::LeaveReq;
use flo_w3gs::protocol::leave::{LeaveAck, PlayerLeft};
use flo_w3gs::protocol::packet::*;

use crate::error::*;
use crate::game::{
  GameEvent, GameEventSender, GameSlot, SlotClientStatus, SlotClientStatusUpdateSource,
};

use super::clock::ActionTickStream;
use crate::game::host::clock::Tick;
use flo_w3gs::protocol::constants::LeaveReason;

#[derive(Debug)]
pub enum Message {
  Incoming {
    player_id: i32,
    slot_player_id: u8,
    frame: Frame,
  },
  PlayerConnect {
    player_id: i32,
    slot_player_id: u8,
    tx: Sender<Frame>,
  },
  PlayerDisconnect {
    player_id: i32,
    slot_player_id: u8,
  },
}

#[derive(Debug)]
pub struct Dispatcher {
  scope: SpawnScope,
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
    let (action_tx, action_rx) = channel(10);

    tokio::spawn(Self::tick(
      state.player_map.clone(),
      start_rx,
      action_rx,
      scope.handle(),
    ));

    tokio::spawn(
      Self::serve(state, rx, action_tx, out_tx, scope.handle())
        .instrument(tracing::debug_span!("state_worker", game_id)),
    );

    Dispatcher {
      scope,
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
    mut action_tx: Sender<ActionMsg>,
    mut out_tx: GameEventSender,
    mut scope: SpawnScopeHandle,
  ) {
    tokio::pin! {
      let dropped = scope.left();
    }

    loop {
      tokio::select! {
        _ = &mut dropped => {
          break;
        }
        next = rx.recv() => {
          if let Some(msg) = next {
            match state.dispatch(msg, &mut action_tx, &mut out_tx).await {
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

  async fn tick(
    player_map: Arc<Mutex<PlayerMap>>,
    start_rx: oneshot::Receiver<()>,
    mut rx: Receiver<ActionMsg>,
    mut scope: SpawnScopeHandle,
  ) {
    tokio::pin! {
      let dropped = scope.left();
    }

    let mut stream = ActionTickStream::new(crate::constants::GAME_DEFAULT_STEP_MS);

    if let Ok(_) = start_rx.await {
      loop {
        tokio::select! {
          _ = &mut dropped => {
            break;
          }
          Some(msg) = rx.recv() => {
            match msg {
              ActionMsg::PlayerAction(action) => {
                stream.add_player_action(action);
              }
            }
          }
          Some(tick) = stream.next() => {
            if let Err(err) = player_map.lock().dispatch_action_tick(tick) {
              tracing::error!("dispatch action tick: {}", err);
              break;
            }
          }
        }
      }
    }

    tracing::debug!("exiting")
  }
}

#[derive(Debug)]
enum ActionMsg {
  PlayerAction(PlayerAction),
}

#[derive(Debug)]
struct State {
  max_lag_time: u32,
  player_map: Arc<Mutex<PlayerMap>>,
  player_ack_map: BTreeMap<i32, AtomicUsize>,
}

impl State {
  fn new(slots: &[GameSlot]) -> Self {
    State {
      max_lag_time: 0,
      player_map: Arc::new(Mutex::new(PlayerMap::new(slots))),
      player_ack_map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, AtomicUsize::new(0)))
        .collect(),
    }
  }

  fn ack_tick(&self, player_id: i32) {
    self.player_ack_map.get(&player_id).map(|counter| {
      counter.fetch_add(1, Ordering::SeqCst);
    });
  }

  pub async fn dispatch(
    &mut self,
    msg: Message,
    action_tx: &mut Sender<ActionMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<DispatchResult> {
    match msg {
      Message::Incoming {
        player_id,
        slot_player_id,
        frame,
      } => match frame.type_id {
        PacketTypeId::W3GS => {
          let pkt = frame_to_w3gs(frame)?;

          if pkt.type_id() == OutgoingAction::PACKET_TYPE_ID {
            let payload: OutgoingAction = pkt.decode_payload()?;
            if let Err(_) = action_tx
              .send(ActionMsg::PlayerAction(PlayerAction {
                player_id: slot_player_id,
                data: payload.data,
              }))
              .await
            {
              return Err(Error::Cancelled);
            }
            return Ok(DispatchResult::Continue);
          }

          let res = self
            .dispatch_incoming_w3gs(player_id, slot_player_id, pkt, out_tx)
            .await?;
          return Ok(res);
        }
        _ => {
          self.dispatch_incoming_flo(player_id, frame, out_tx).await?;
        }
      },
      Message::PlayerConnect { player_id, tx, .. } => {
        {
          self.player_map.lock().get_player(player_id)?.tx.replace(tx);
        }
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Connected,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      Message::PlayerDisconnect {
        player_id,
        slot_player_id,
      } => {
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Disconnected,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
        let pkt = Packet::simple(PlayerLeft {
          player_id: slot_player_id,
          reason: LeaveReason::LeaveDisconnect,
        })?;
        {
          let mut guard = self.player_map.lock();
          guard.get_player(player_id)?.tx.take();
          guard.broadcast(pkt, None)?;
        }
      }
    }

    Ok(DispatchResult::Continue)
  }

  pub async fn dispatch_incoming_w3gs(
    &mut self,
    player_id: i32,
    slot_player_id: u8,
    mut packet: Packet,
    out_tx: &mut GameEventSender,
  ) -> Result<DispatchResult> {
    use flo_w3gs::protocol::constants::PacketTypeId;

    match packet.type_id() {
      PacketTypeId::LeaveReq => {
        let req: LeaveReq = packet.decode_simple()?;
        tracing::info!(player_id, "leave: {:?}", req.reason());

        let pkt = Packet::simple(PlayerLeft {
          player_id: slot_player_id,
          reason: req.reason(),
        })?;

        {
          let mut guard = self.player_map.lock();
          {
            let player = guard.get_player(player_id)?;
            player.send_w3gs(Packet::simple(LeaveAck)?).ok();
            player.disconnect();
          }
          guard.broadcast(pkt, Some(&[player_id]))?;
        }
        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Left,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      PacketTypeId::ChatToHost => {
        packet.header.type_id = PacketTypeId::ChatFromHost;
        self
          .player_map
          .lock()
          .broadcast(packet, Some(&[player_id]))?;
      }
      PacketTypeId::OutgoingKeepAlive => {
        self.ack_tick(player_id);
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
}

#[derive(Debug)]
struct PlayerMap {
  map: BTreeMap<i32, PlayerDispatchInfo>,
}

impl PlayerMap {
  fn new(slots: &[GameSlot]) -> Self {
    Self {
      map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, PlayerDispatchInfo::new(slot)))
        .collect(),
    }
  }

  fn get_player(&mut self, player_id: i32) -> Result<&mut PlayerDispatchInfo> {
    if let Some(player) = self.map.get_mut(&player_id) {
      Ok(player)
    } else {
      tracing::error!(player_id, "unknown player id");
      return Err(Error::PlayerNotFoundInGame);
    }
  }

  pub fn dispatch_action_tick(&mut self, tick: Tick) -> Result<()> {
    self.broadcast(Packet::with_payload(IncomingAction::from(tick))?, None)?;
    Ok(())
  }

  pub fn broadcast(&mut self, packet: Packet, excludes: Option<&[i32]>) -> Result<()> {
    let frame = w3gs_to_frame(packet);
    let excludes = excludes.unwrap_or(&[] as &[i32]);
    let errors: Vec<_> = self
      .map
      .iter_mut()
      .filter_map(|(player_id, info)| {
        if excludes.contains(player_id) {
          return None;
        }
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
            self.map.remove(&player_id);
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

  fn disconnect(&mut self) -> Option<Sender<Frame>> {
    self.tx.take()
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
