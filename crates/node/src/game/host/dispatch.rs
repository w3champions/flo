use futures::stream::StreamExt;
use parking_lot::Mutex;
use s2_grpc_utils::S2ProtoEnum;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use tracing_futures::Instrument;

use flo_net::packet::{Frame, PacketTypeId};
use flo_net::w3gs::{frame_to_w3gs, w3gs_to_frame};
use flo_task::{SpawnScope, SpawnScopeHandle};
use flo_w3gs::action::IncomingAction;
use flo_w3gs::protocol::action::{OutgoingAction, PlayerAction, TimeSlot};
use flo_w3gs::protocol::leave::LeaveReq;
use flo_w3gs::protocol::leave::{LeaveAck, PlayerLeft};
use flo_w3gs::protocol::packet::*;

use super::broadcast;
use crate::error::*;
use crate::game::{
  GameEvent, GameEventSender, PlayerSlot, SlotClientStatus, SlotClientStatusUpdateSource,
};

use super::clock::ActionTickStream;
use crate::game::host::clock::Tick;
use flo_w3gs::protocol::chat::ChatToHost;
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
    slots: &[PlayerSlot],
    rx: Receiver<Message>,
    out_tx: GameEventSender,
  ) -> Self {
    let scope = SpawnScope::new();
    let state = State::new(slots);

    let (start_tx, start_rx) = oneshot::channel();
    let (action_tx, action_rx) = channel(10);

    tokio::spawn(
      Self::tick(state.shared.clone(), start_rx, action_rx, scope.handle())
        .instrument(tracing::debug_span!("tick_worker", game_id)),
    );

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
    shared: Arc<Mutex<Shared>>,
    start_rx: oneshot::Receiver<()>,
    mut rx: Receiver<ActionMsg>,
    mut scope: SpawnScopeHandle,
  ) {
    tokio::pin! {
      let dropped = scope.left();
    }

    if let Ok(_) = start_rx.await {
      let mut tick_stream = ActionTickStream::new(crate::constants::GAME_DEFAULT_STEP_MS);
      loop {
        tokio::select! {
          _ = &mut dropped => {
            break;
          }
          Some(msg) = rx.recv() => {
            match msg {
              ActionMsg::PlayerAction(action) => {
                tick_stream.add_player_action(action);
              }
            }
          }
          Some(tick) = tick_stream.next() => {
            if let Err(err) = shared.lock().dispatch_action_tick(tick) {
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
  sent_tick: u32,
  shared: Arc<Mutex<Shared>>,
  player_ack_map: BTreeMap<i32, usize>,
  game_player_id_lookup: BTreeMap<u8, i32>,
}

impl State {
  fn new(slots: &[PlayerSlot]) -> Self {
    State {
      sent_tick: 0,
      shared: Arc::new(Mutex::new(Shared::new(slots))),
      player_ack_map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, 0))
        .collect(),
      game_player_id_lookup: slots
        .into_iter()
        .map(|slot| ((slot.id + 1) as u8, slot.player.player_id))
        .collect(),
    }
  }

  fn ack_tick(&mut self, player_id: i32) {
    self
      .player_ack_map
      .get_mut(&player_id)
      .map(|tick| *tick += 1);
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
          self.shared.lock().get_player(player_id)?.tx.replace(tx);
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
        {
          let mut guard = self.shared.lock();
          if let Some(_) = guard.get_player(player_id)?.tx.take() {
            let pkt = Packet::simple(PlayerLeft {
              player_id: slot_player_id,
              reason: LeaveReason::LeaveDisconnect,
            })?;
            guard.broadcast(pkt, broadcast::Everyone)?;
          }
        }
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

    match packet.type_id() {
      PacketTypeId::LeaveReq => {
        let req: LeaveReq = packet.decode_simple()?;
        tracing::info!(player_id, "leave: {:?}", req.reason());

        let pkt = Packet::simple(PlayerLeft {
          player_id: slot_player_id,
          reason: req.reason(),
        })?;

        {
          let mut guard = self.shared.lock();
          let player = guard.get_player(player_id)?;
          player.send_w3gs(Packet::simple(LeaveAck)?).ok();
          player.disconnect();
          guard.broadcast(pkt, broadcast::DenyList(&[player_id]))?;
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
        self.dispatch_chat(player_id, packet).await?;
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

  pub async fn dispatch_chat(&mut self, player_id: i32, mut packet: Packet) -> Result<()> {
    use flo_w3gs::protocol::constants::PacketTypeId;

    let chat: ChatToHost = packet.decode_simple()?;
    packet.header.type_id = PacketTypeId::ChatFromHost;
    self.shared.lock().broadcast(
      packet,
      broadcast::AllowList(
        &chat
          .to_players
          .into_iter()
          .filter_map(|id| {
            if let Some(id) = self.game_player_id_lookup.get(&id).cloned() {
              if id != player_id {
                Some(id)
              } else {
                None
              }
            } else {
              None
            }
          })
          .collect::<Vec<_>>(),
      ),
    )?;
    Ok(())
  }
}

#[derive(Debug)]
struct Shared {
  map: BTreeMap<i32, PlayerDispatchInfo>,
}

impl Shared {
  fn new(slots: &[PlayerSlot]) -> Self {
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
    let action_packet = Packet::with_payload(IncomingAction(TimeSlot {
      time_increment_ms: tick.time_increment_ms,
      actions: tick.actions,
    }))?;
    self.broadcast(action_packet, broadcast::Everyone)?;
    Ok(())
  }

  pub fn broadcast<T: broadcast::BroadcastTarget>(
    &mut self,
    packet: Packet,
    target: T,
  ) -> Result<()> {
    let frame = w3gs_to_frame(packet);
    let errors: Vec<_> = self
      .map
      .iter_mut()
      .filter_map(|(player_id, info)| {
        if !target.contains(*player_id) {
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
            self.get_player(player_id)?.tx.take();
          }
          PlayerSendError::ChannelFull => {
            tracing::info!(player_id, "removing player: channel full");
            self.get_player(player_id)?.tx.take();
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
  ticks: usize,
  pending_ack_packets: Vec<Packet>,
  tx: Option<Sender<Frame>>,
}

impl PlayerDispatchInfo {
  fn new(_slot: &PlayerSlot) -> Self {
    Self {
      pending_ack_packets: vec![],
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
      match tx.try_send(frame) {
        Ok(_) => Ok(()),
        Err(TrySendError::Closed(frame)) => Err(PlayerSendError::Closed(frame)),
        Err(TrySendError::Full(_)) => Err(PlayerSendError::ChannelFull),
      }
    } else {
      Err(PlayerSendError::NotConnected(frame))
    }
  }
}

enum PlayerSendError {
  NotConnected(Frame),
  Closed(Frame),
  ChannelFull,
}

#[derive(Debug)]
enum DispatchResult {
  Continue,
  Action(PlayerAction),
  Lagged,
}
