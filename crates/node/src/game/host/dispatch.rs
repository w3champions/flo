use super::broadcast;
use super::clock::ActionTickStream;
use super::delay::{DelayedFrame, DelayedFrameStream};
use super::player::{PlayerDispatchInfo, PlayerSendError};
use super::sync::SyncMap;
use crate::error::*;
use crate::game::host::clock::Tick;
use crate::game::host::sync::PlayerDesync;
use crate::game::player_stream::{PlayerStream, PlayerStreamCmd, PlayerStreamHandle};
use crate::game::{
  GameEvent, GameEventSender, PlayerBanType, PlayerSlot, SlotClientStatus,
  SlotClientStatusUpdateSource,
};
use flo_net::packet::{Frame, PacketTypeId};
use flo_net::w3gs::{W3GSFrameExt, W3GSMetadata, W3GSPacket};
use flo_util::chat::{parse_chat_command, ChatCommand};
use flo_w3gs::action::{IncomingAction, OutgoingKeepAlive};
use flo_w3gs::protocol::action::{OutgoingAction, PlayerAction, TimeSlot};
use flo_w3gs::protocol::chat::ChatToHost;
use flo_w3gs::protocol::constants::LeaveReason;
use flo_w3gs::protocol::lag::{LagPlayer, StartLag, StopLag};
use flo_w3gs::protocol::leave::LeaveReq;
use flo_w3gs::protocol::leave::{LeaveAck, PlayerLeft};
use flo_w3gs::protocol::packet::*;
use futures::stream::StreamExt;
use parking_lot::Mutex;
use s2_grpc_utils::S2ProtoEnum;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;
use tracing_futures::Instrument;

#[derive(Debug)]
pub enum Cmd {
  RegisterStream {
    stream: PlayerStream,
    tx: oneshot::Sender<Result<PlayerStreamHandle>>,
  },
}

enum PeerMsg {
  Incoming { player_id: i32, frame: Frame },
  Closed { player_id: i32, stream_id: u64 },
}

impl PeerMsg {
  fn player_id(&self) -> i32 {
    match *self {
      PeerMsg::Incoming { player_id, .. } => player_id,
      PeerMsg::Closed { player_id, .. } => player_id,
    }
  }
}

#[derive(Debug)]
pub struct Dispatcher {
  game_id: i32,
  ct: CancellationToken,
  cmd_tx: Sender<Cmd>,
  start_notify: Arc<Notify>,
}

impl Drop for Dispatcher {
  fn drop(&mut self) {
    self.ct.cancel();
  }
}

impl Dispatcher {
  pub fn new(game_id: i32, slots: &[PlayerSlot], out_tx: GameEventSender) -> Self {
    let ct = CancellationToken::new();
    let start_notify = Arc::new(Notify::new());
    let (cmd_tx, cmd_rx) = channel(10);
    let (action_tx, action_rx) = channel(10);

    let state = State::new(game_id, slots, action_tx.clone(), ct.clone());

    let mut start_messages = vec![];
    if !state.chat_banned_player_ids.is_empty() {
      start_messages.push("One or more players in this game have been muted.".to_string());
    }

    tokio::spawn(
      Self::tick(
        state.shared.clone(),
        start_messages,
        start_notify.clone(),
        action_rx,
        ct.clone(),
      )
      .instrument(tracing::debug_span!("tick", game_id)),
    );

    tokio::spawn(
      Self::serve(state, cmd_rx, action_tx, out_tx, ct.clone())
        .instrument(tracing::debug_span!("serve", game_id)),
    );

    Dispatcher {
      ct,
      game_id,
      cmd_tx,
      start_notify,
    }
  }

  pub fn start(&mut self) {
    tracing::info!(game_id = self.game_id, "game started.");
    self.start_notify.notify_one();
  }

  pub async fn register_player_stream(&self, stream: PlayerStream) -> Result<PlayerStreamHandle> {
    let (tx, rx) = oneshot::channel();
    self
      .cmd_tx
      .send(Cmd::RegisterStream { stream, tx })
      .await
      .map_err(|_| Error::Cancelled)?;
    rx.await.map_err(|_| Error::Cancelled)?
  }

  async fn serve(
    mut state: State,
    mut rx: Receiver<Cmd>,
    mut action_tx: Sender<ActionMsg>,
    mut out_tx: GameEventSender,
    ct: CancellationToken,
  ) {
    let (peer_tx, mut peer_rx) = channel::<PeerMsg>(crate::constants::GAME_DISPATCH_BUF_SIZE);
    loop {
      tokio::select! {
        _ = ct.cancelled() => {
          break;
        }
        Some(msg) = peer_rx.recv() => {
          let player_id = msg.player_id();
          match state.dispatch_peer(msg, &mut action_tx, &mut out_tx).await {
            Ok(_) => {},
            Err(Error::Cancelled) => {},
            Err(err) => {
              tracing::error!(player_id, "dispatch peer: {}", err);
            },
          }
        }
        Some(cmd) = rx.recv() => {
          match state.dispatch_cmd(cmd, &peer_tx, &mut out_tx).await {
            Ok(_) => {},
            Err(Error::Cancelled) => {},
            Err(err) => {
              tracing::error!("dispatch cmd: {}", err);
            },
          }
        }
      }
    }
  }

  async fn tick(
    shared: Arc<Mutex<Shared>>,
    start_messages: Vec<String>,
    start_notify: Arc<Notify>,
    mut rx: Receiver<ActionMsg>,
    ct: CancellationToken,
  ) {
    let started = {
      tokio::select! {
        _ = start_notify.notified() => true,
        _ = ct.cancelled() => false,
      }
    };

    if started {
      if !start_messages.is_empty() {
        let mut shared = shared.lock();
        for msg in start_messages {
          shared.broadcast_message(msg);
        }
      }

      let mut tick_stream = ActionTickStream::new(crate::constants::GAME_DEFAULT_STEP_MS);

      loop {
        tokio::select! {
          _ = ct.cancelled() => {
            break;
          }
          Some(msg) = rx.recv() => {
            match msg {
              ActionMsg::PlayerAction(action) => {
                tick_stream.add_action(action);
              }
              ActionMsg::SetStep(step) => {
                tick_stream.set_step(step);
                shared
                  .lock()
                  .broadcast_message(format!("Game step has been set to {}ms.", tick_stream.step()));
              },
              ActionMsg::PauseClock => {
                tick_stream.pause();
              },
              ActionMsg::ResumeClock => {
                tick_stream.resume();
              }
            }
          }
          Some(tick) = tick_stream.next() => {
            match shared.lock().dispatch_action_tick(tick) {
              Ok(DispatchResult::Continue) => {},
              Ok(DispatchResult::Lag(tick)) => {
                tick_stream.replace_actions(tick.actions);
                tick_stream.pause();
              }
              Err(err) => {
                tracing::error!("dispatch action tick: {}", err);
                break;
              }
            }
          }
        }
      }
    }
  }
}

#[derive(Debug)]
enum ActionMsg {
  PlayerAction(PlayerAction),
  SetStep(u16),
  PauseClock,
  ResumeClock,
}

#[derive(Debug)]
struct State {
  game_id: i32,
  ct: CancellationToken,
  shared: Arc<Mutex<Shared>>,
  game_player_id_lookup: BTreeMap<u8, i32>,
  player_name_lookup: BTreeMap<i32, String>,
  chat_banned_player_ids: Vec<i32>,
  left_players: BTreeSet<i32>,
}

impl State {
  fn new(
    game_id: i32,
    slots: &[PlayerSlot],
    _action_tx: Sender<ActionMsg>,
    ct: CancellationToken,
  ) -> Self {
    State {
      game_id,
      ct,
      shared: Arc::new(Mutex::new(Shared::new(game_id, slots))),
      game_player_id_lookup: slots
        .into_iter()
        .map(|slot| ((slot.id + 1) as u8, slot.player.player_id))
        .collect(),
      player_name_lookup: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, slot.player.name.clone()))
        .collect(),
      chat_banned_player_ids: slots
        .into_iter()
        .filter_map(|v| {
          if v.player.ban_list.contains(&PlayerBanType::Chat) {
            Some(v.player.player_id)
          } else {
            None
          }
        })
        .collect(),
      left_players: BTreeSet::new(),
    }
  }

  pub async fn dispatch_cmd(
    &mut self,
    cmd: Cmd,
    peer_tx: &Sender<PeerMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    match cmd {
      Cmd::RegisterStream { stream, tx } => {
        tx.send(self.register_stream(stream, peer_tx, out_tx).await)
          .ok();
      }
    }

    Ok(())
  }

  async fn register_stream(
    &mut self,
    stream: PlayerStream,
    peer_tx: &Sender<PeerMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<PlayerStreamHandle> {
    let game_id = self.game_id;
    let player_id = stream.player_id();
    tracing::info!(player_id, stream_id = stream.id(), "player connected");

    if self.left_players.contains(&player_id) {
      return Err(Error::PlayerAlreadyLeft);
    }

    let (peer_cmd_tx, peer_cmd_rx) = channel(crate::constants::PEER_CHANNEL_SIZE);
    let sender = PlayerStreamHandle::new(&stream, peer_cmd_tx.clone());

    let (status, delay, resend_frames) = {
      let mut guard = self.shared.lock();
      let player = guard
        .get_player(player_id)
        .ok_or_else(|| Error::PlayerAlreadyLeft)?;
      let reconnected = !player.pristine();
      let delay = player.delay().cloned();
      player.register_sender(sender.clone());
      if reconnected {
        let resend_frames = player.get_resend_frames();
        let msg = format!("Reconnected to the server: {}", player.player_name());
        guard.broadcast_message(msg);
        (
          if guard.sync.tick() > 0 {
            SlotClientStatus::Loaded
          } else {
            SlotClientStatus::Connected
          },
          delay,
          resend_frames,
        )
      } else {
        (SlotClientStatus::Connected, delay, None)
      }
    };

    out_tx
      .send(GameEvent::PlayerStatusChange(
        player_id,
        status,
        SlotClientStatusUpdateSource::Node,
      ))
      .await
      .map_err(|_| Error::Cancelled)?;

    let mut worker = PeerWorker::new(stream, peer_cmd_rx, peer_tx.clone(), delay);
    tokio::spawn(
      async move {
        if let Err(err) = worker.serve(resend_frames).await {
          match err {
            Error::Cancelled => {}
            err => tracing::error!("worker: {}", err),
          }
        }
        worker
          .dispatcher_tx
          .send(PeerMsg::Closed {
            player_id,
            stream_id: worker.stream.id(),
          })
          .await
          .ok();
      }
      .instrument(tracing::debug_span!("peer", game_id, player_id)),
    );
    Ok(sender)
  }

  pub async fn dispatch_peer(
    &mut self,
    msg: PeerMsg,
    action_tx: &mut Sender<ActionMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    match msg {
      PeerMsg::Incoming { player_id, frame } => match frame.type_id {
        PacketTypeId::W3GS => {
          let (meta, pkt) = frame.try_into_w3gs()?;
          self
            .dispatch_incoming_w3gs(player_id, meta, pkt, action_tx, out_tx)
            .await?;
        }
        _ => {
          self.dispatch_incoming_flo(player_id, frame, out_tx).await?;
        }
      },
      PeerMsg::Closed {
        player_id,
        stream_id,
      } => {
        if let Some(player) = self.shared.lock().get_player(player_id) {
          if player.stream_id() == Some(stream_id) {
            player.close_sender();
          }
        }

        if !self.left_players.contains(&player_id) {
          out_tx
            .send(GameEvent::PlayerStatusChange(
              player_id,
              SlotClientStatus::Disconnected,
              SlotClientStatusUpdateSource::Node,
            ))
            .await
            .map_err(|_| Error::Cancelled)?;
        }
      }
    }
    Ok(())
  }

  async fn dispatch_incoming_w3gs(
    &mut self,
    player_id: i32,
    meta: W3GSMetadata,
    packet: Packet,
    action_tx: &mut Sender<ActionMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    use flo_w3gs::protocol::constants::PacketTypeId;

    let slot_player_id = {
      let mut shared = self.shared.lock();
      let player = shared
        .get_player(player_id)
        .ok_or_else(|| Error::PlayerNotFoundInGame)?;
      if !player.update_ack(meta.clone()) {
        tracing::warn!(
          player_id,
          "discard resend: {}, {:?}, {:?}",
          meta.sid(),
          meta.ack_sid(),
          packet.type_id()
        );
        return Ok(());
      }
      player.slot_player_id()
    };

    match packet.type_id() {
      PacketTypeId::OutgoingAction => {
        let payload: OutgoingAction = packet.decode_payload()?;
        action_tx
          .send(ActionMsg::PlayerAction(PlayerAction {
            player_id: slot_player_id,
            data: payload.data,
          }))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      PacketTypeId::LeaveReq => {
        let req: LeaveReq = packet.decode_simple()?;
        tracing::info!(
          game_id = self.game_id,
          player_id,
          "leave: {:?}",
          req.reason()
        );

        let pkt = Packet::simple(PlayerLeft {
          player_id: slot_player_id,
          reason: req.reason(),
        })?;

        {
          let mut guard = self.shared.lock();
          let player = guard
            .get_player(player_id)
            .ok_or_else(|| Error::PlayerNotFoundInGame)?;
          player.send_w3gs(Packet::simple(LeaveAck)?).ok();
          player.close_sender();
          guard.broadcast(pkt, broadcast::DenyList(&[player_id]))?;
        }

        self.left_players.insert(player_id);
        self.shared.lock().map.remove(&player_id);

        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            SlotClientStatus::Left,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      PacketTypeId::DropReq => {
        tracing::info!(game_id = self.game_id, player_id, "drop request");
        let res = self.shared.lock().request_drop(player_id)?;
        match res {
          RequestDropResult::NoLaggingPlayer | RequestDropResult::Voting => {}
          RequestDropResult::Done(dropped_player_ids) => {
            self.left_players.extend(dropped_player_ids);
            action_tx
              .send(ActionMsg::ResumeClock)
              .await
              .map_err(|_| Error::Cancelled)?;
          }
        }
      }
      PacketTypeId::ChatToHost => {
        self.dispatch_chat(player_id, packet, action_tx).await?;
      }
      PacketTypeId::OutgoingKeepAlive => {
        let payload: OutgoingKeepAlive = packet.decode_simple()?;
        let checksum = payload.checksum;
        // tracing::debug!("player_id = {}, checksum = {}", player_id, checksum);
        let res = self.shared.lock().ack(player_id, checksum);
        match res {
          Ok(AckAction::Continue) => {}
          Ok(AckAction::ResumeClock) => {
            tracing::info!("all lagging player dropped or resumed.");
            action_tx
              .send(ActionMsg::ResumeClock)
              .await
              .map_err(|_| Error::Cancelled)?;
          }
          Err(err) => {
            tracing::error!("sync ack error: {:?}", err);
          }
        }
      }
      id => {
        tracing::warn!("unexpected w3gs packet id = {:?}", id);
      }
    }

    Ok(())
  }

  async fn dispatch_incoming_flo(
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

  async fn dispatch_chat(
    &mut self,
    player_id: i32,
    mut packet: Packet,
    action_tx: &mut Sender<ActionMsg>,
  ) -> Result<()> {
    use flo_w3gs::protocol::constants::PacketTypeId;

    let chat: ChatToHost = packet.decode_simple()?;
    if let Some(cmd) = chat.chat_message().and_then(parse_chat_command) {
      if self.handle_command(action_tx, player_id, cmd).await? {
        return Ok(());
      }
    }

    if self.chat_banned_player_ids.contains(&player_id) && chat.is_in_game_chat() {
      return Ok(());
    }

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

  async fn handle_command(
    &self,
    action_tx: &mut Sender<ActionMsg>,
    player_id: i32,
    cmd: ChatCommand<'_>,
  ) -> Result<bool> {
    match cmd.name() {
      "drop" => {
        let shared = self.shared.clone();
        let ct = self.ct.clone();
        tokio::spawn(async move {
          shared
            .lock()
            .get_player(player_id)
            .map(|v| v.close_sender());
        });
      }
      "delay" => {
        if let Some(Some((ms,))) = cmd.parse_arguments::<Option<(u16,)>>().ok() {
          let [min, max] = crate::constants::GAME_DELAY_RANGE;

          if ms == 0 {
            let mut guard = self.shared.lock();
            let name = guard.get_player(player_id).map(|player| {
              player.set_delay(None);
              player.player_name().to_string()
            });
            if let Some(name) = name {
              guard.broadcast_message(format!("Removed delay for {}", name));
            }
            return Ok(true);
          }

          let duration = Duration::from_millis(ms as _);
          if duration < min || duration > max {
            self.shared.lock().private_message(
              player_id,
              format!(
                "Invalid value, range {} ~ {}",
                min.as_millis(),
                max.as_millis()
              ),
            );
            return Ok(true);
          }

          {
            let mut guard = self.shared.lock();
            let name = guard.get_player(player_id).map(|player| {
              player.set_delay(Some(duration));
              player.player_name().to_string()
            });
            if let Some(name) = name {
              guard.broadcast_message(format!("Set delay for {}: {}ms", name, ms));
            }
          }
        } else {
          let mut lock = self.shared.lock();
          let msgs: Vec<_> = lock
            .map
            .values()
            .map(|v| {
              format!(
                "{}: {}",
                v.player_name(),
                match v.delay() {
                  Some(v) => format!("+{}ms", v.as_millis()),
                  None => "Not set".to_string(),
                }
              )
            })
            .collect();
          if let Some(player) = lock.get_player(player_id) {
            for msg in msgs {
              player.send_private_message(&msg);
            }
          }
        }
      }
      // "ping" => {
      //   let mut lock = self.shared.lock();
      //   let msgs: Vec<_> = lock
      //     .map
      //     .values()
      //     .map(|v| {
      //       format!(
      //         "{}: {}",
      //         v.player_name(),
      //         match v.ping() {
      //           Some(v) => format!("{}ms", v),
      //           None => "N/A".to_string(),
      //         }
      //       )
      //     })
      //     .collect();
      //   for msg in msgs {
      //     lock.broadcast_message(msg);
      //   }
      //   true
      // }
      "conn" => {
        let mut lock = self.shared.lock();
        let msgs: Vec<_> = lock
          .map
          .values()
          .map(|v| {
            let q = v.ack_queue();
            format!(
              "{}: last_ack_received = {:?}, len = {}",
              v.player_name(),
              q.last_ack_received(),
              q.pending_ack_len()
            )
          })
          .collect();
        for msg in msgs {
          lock.private_message(player_id, msg);
        }
      }
      "step" => match cmd.parse_arguments::<(u16,)>().ok() {
        Some((step,)) => {
          action_tx.send(ActionMsg::SetStep(step)).await.ok();
        }
        None => {
          self
            .shared
            .lock()
            .private_message(player_id, "Invalid syntax, usage: !step 30");
        }
      },
      _ => return Ok(false),
    };
    Ok(true)
  }
}

#[derive(Debug)]
struct Shared {
  game_id: i32,
  map: BTreeMap<i32, PlayerDispatchInfo>,
  sync: SyncMap,
  lagging_player_ids: BTreeSet<i32>,
  drop_votes: BTreeSet<i32>,
}

impl Shared {
  fn new(game_id: i32, slots: &[PlayerSlot]) -> Self {
    let sync = SyncMap::new(slots.iter().map(|s| s.player.player_id).collect());
    Self {
      game_id,
      map: slots
        .into_iter()
        .map(|slot| (slot.player.player_id, PlayerDispatchInfo::new(slot)))
        .collect(),
      sync,
      lagging_player_ids: BTreeSet::new(),
      drop_votes: BTreeSet::new(),
    }
  }

  fn get_player(&mut self, player_id: i32) -> Option<&mut PlayerDispatchInfo> {
    self.map.get_mut(&player_id)
  }

  #[must_use]
  pub fn dispatch_action_tick(&mut self, tick: Tick) -> Result<DispatchResult> {
    let time_increment_ms = tick.time_increment_ms;
    if let Some(timeouts) = self.sync.clock(time_increment_ms) {
      let player_ids: Vec<_> = timeouts.into_iter().map(|t| t.player_id).collect();
      if self.handle_lag(player_ids)? {
        return Ok(DispatchResult::Lag(tick));
      }
    }
    let action_packet = Packet::with_payload(IncomingAction(TimeSlot {
      time_increment_ms,
      actions: tick.actions,
    }))?;
    self.broadcast(action_packet, broadcast::Everyone)?;
    Ok(DispatchResult::Continue)
  }

  fn handle_lag(&mut self, add_player_ids: Vec<i32>) -> Result<bool> {
    self.lagging_player_ids.extend(add_player_ids);
    if let Some((pkt, ids)) = self.refresh_lag_packet()? {
      self.drop_votes.clear();
      self.broadcast(pkt, broadcast::DenyList(&ids))?;
      Ok(true)
    } else {
      Ok(false)
    }
  }

  fn check_stop_lag(&mut self) -> Result<bool> {
    if self.lagging_player_ids.is_empty() {
      return Ok(false);
    }
    let mut stop_lag_players = vec![];
    let mut packets = vec![];
    for (id, info) in &mut self.map {
      if self.lagging_player_ids.contains(id) {
        if self.sync.player_pending_ticks(*id) == Some(0) {
          self.lagging_player_ids.remove(id);
          stop_lag_players.push(*id);
          packets.push(W3GSPacket::simple(StopLag(LagPlayer {
            player_id: info.slot_player_id(),
            lag_duration_ms: info.end_lag(),
          }))?);
        }
      }
    }
    for pkt in packets {
      self.broadcast(pkt, broadcast::DenyList(&stop_lag_players))?;
    }

    Ok(self.lagging_player_ids.is_empty())
  }

  fn refresh_lag_packet(&mut self) -> Result<Option<(W3GSPacket, Vec<i32>)>> {
    let mut lag_player_ids = Vec::with_capacity(self.lagging_player_ids.len());
    let lag_players: Vec<_> = self
      .lagging_player_ids
      .clone()
      .into_iter()
      .filter_map(|player_id| {
        let player = self.get_player(player_id)?;
        let slot_player_id = player.slot_player_id();
        lag_player_ids.push(player_id);
        Some(LagPlayer {
          player_id: slot_player_id,
          lag_duration_ms: player.start_lag(),
        })
      })
      .collect();
    if lag_players.is_empty() {
      return Ok(None);
    }

    if !lag_players.is_empty() {
      tracing::warn!(
        "lag: players = {:?}, time = {}",
        lag_player_ids,
        self.sync.time()
      );
    }

    let payload = StartLag::new(lag_players);
    Ok(Some((W3GSPacket::simple(payload)?, lag_player_ids)))
  }

  fn disconnect_player_and_broadcast(&mut self, player_id: i32) -> Result<()> {
    let player = if let Some(v) = self.get_player(player_id) {
      v
    } else {
      return Ok(());
    };
    let pkt = Packet::simple(PlayerLeft {
      player_id: player.slot_player_id(),
      reason: LeaveReason::LeaveDisconnect,
    })?;

    self.map.remove(&player_id);
    self.lagging_player_ids.remove(&player_id);
    self.broadcast(pkt, broadcast::DenyList(&[player_id]))?;
    if let Some(desync) = self.sync.remove_player(player_id) {
      tracing::warn!(
        player_id,
        "desync detected after disconnecting player: {:?}",
        desync
      );
      tracing::warn!("{}", self.sync.debug_pending())
    }
    Ok(())
  }

  pub fn broadcast<T: broadcast::BroadcastTarget>(
    &mut self,
    packet: Packet,
    target: T,
  ) -> Result<()> {
    let errors: Vec<_> = {
      self
        .map
        .iter_mut()
        .filter_map(|(player_id, info)| {
          if !target.contains(*player_id) {
            return None;
          }

          if info.stream_id().is_some() {
            info
              .send_w3gs(packet.clone())
              .err()
              .map(|err| (*player_id, err))
          } else {
            info.enqueue_w3gs(packet.clone());
            None
          }
        })
        .collect()
    };

    if !errors.is_empty() {
      for (player_id, err) in errors {
        match err {
          PlayerSendError::Closed(_frame) => {
            tracing::info!(game_id = self.game_id, player_id, "stream broken");
          }
          PlayerSendError::ChannelFull => {
            tracing::info!(
              game_id = self.game_id,
              player_id,
              "removing player: channel full"
            );
            self.disconnect_player_and_broadcast(player_id)?;
          }
          _ => {}
        }
      }
    }

    Ok(())
  }

  pub fn broadcast_message<T: AsRef<str> + Send + 'static>(&mut self, message: T) {
    self.map.iter_mut().for_each(|(_, info)| {
      info.send_private_message(message.as_ref());
    });
  }

  pub fn private_message<T: AsRef<str> + Send + 'static>(&mut self, player_id: i32, message: T) {
    if let Some(info) = self.map.get_mut(&player_id) {
      info.send_private_message(message.as_ref());
    }
  }

  pub fn request_drop(&mut self, player_id: i32) -> Result<RequestDropResult> {
    let lagging = self.lagging_player_ids.len();

    if lagging == 0 {
      return Ok(RequestDropResult::NoLaggingPlayer);
    }

    let vote_required = (self.map.len().saturating_sub(lagging) as f32 / 2.0).ceil() as usize;
    if self.drop_votes.insert(player_id) {
      self.broadcast_message(format!(
        "Received drop vote: {}/{}",
        self.drop_votes.len(),
        vote_required
      ));
    }
    if self.drop_votes.len() >= vote_required {
      let drop_player_ids: Vec<_> = self.lagging_player_ids.iter().cloned().collect();
      for drop_player_id in &drop_player_ids {
        tracing::info!(player_id = *drop_player_id, "lagging player dropped.");
        self.disconnect_player_and_broadcast(*drop_player_id)?;
      }
      self.lagging_player_ids.clear();
      Ok(RequestDropResult::Done(drop_player_ids))
    } else {
      Ok(RequestDropResult::Voting)
    }
  }

  pub fn ack(&mut self, player_id: i32, checksum: u32) -> Result<AckAction> {
    let res = self.sync.ack(player_id, checksum)?;
    if let Some(desync) = res.desync {
      self.handle_desync(desync)?;
    }
    if self.check_stop_lag()? {
      Ok(AckAction::ResumeClock)
    } else {
      Ok(AckAction::Continue)
    }
  }

  fn handle_desync(&mut self, desync: Vec<PlayerDesync>) -> Result<()> {
    let mut handled = BTreeSet::new();
    let mut targets = vec![];
    for item in desync {
      if !handled.contains(&item.player_id) {
        handled.insert(item.player_id);

        tracing::warn!(
          player_id = item.player_id,
          "desync detected: time = {}, tick = {}",
          item.time,
          item.tick
        );
        tracing::warn!("{}", self.sync.debug_pending());

        if let Some(name) = self.map.get(&item.player_id).map(|v| v.player_name()) {
          targets.push((
            item.player_id,
            format!(
              "Desync detected: {} (time = {}, tick = {})",
              name, item.time, item.tick
            ),
          ));
        }
      }
    }

    for (player_id, message) in targets {
      self.broadcast_message(message);
      self.disconnect_player_and_broadcast(player_id)?;
    }
    Ok(())
  }
}

enum AckAction {
  Continue,
  ResumeClock,
}

enum RequestDropResult {
  NoLaggingPlayer,
  Voting,
  Done(Vec<i32>),
}

struct PeerWorker {
  stream: PlayerStream,
  in_rx: Receiver<PlayerStreamCmd>,
  dispatcher_tx: Sender<PeerMsg>,
  delay: DelayedFrameStream,
  delay_send_buf: Vec<Frame>,
}

impl PeerWorker {
  fn new(
    stream: PlayerStream,
    in_rx: Receiver<PlayerStreamCmd>,
    out_tx: Sender<PeerMsg>,
    delay: Option<Duration>,
  ) -> Self {
    Self {
      stream,
      in_rx,
      dispatcher_tx: out_tx,
      delay: DelayedFrameStream::new(delay),
      delay_send_buf: Vec::new(),
    }
  }

  async fn serve(&mut self, resend_frames: Option<Vec<Frame>>) -> Result<()> {
    let player_id = self.stream.player_id();
    let ct = self.stream.token();

    if let Some(frames) = resend_frames {
      self.stream.get_mut().send_frames(frames).await?;
    }

    let mut delay_buf = VecDeque::new();

    loop {
      tokio::select! {
        _ = ct.cancelled() => break,
        next = self.stream.get_mut().recv_frame() => {
          match next {
            Ok(frame) => {
              if self.delay.enabled() {
                self.delay.insert(DelayedFrame::In(frame));
                continue;
              }

              if self.dispatcher_tx.send(PeerMsg::Incoming {
                player_id,
                frame
              }).await.is_err() {
                break;
              }
            }
            Err(err) => {
              tracing::debug!("recv: {}", err);
              break;
            }
          }
        }
        Some(cmd) = self.in_rx.recv() => {
          match cmd {
            PlayerStreamCmd::Send(frame) => {
              if self.delay.enabled() {
                self.delay.insert(DelayedFrame::Out(frame));
                continue;
              }
              self.stream.get_mut().send_frame(frame).await?;
            }
            PlayerStreamCmd::SetDelay(delay) => {
              let expired = if let Some(delay) = delay {
                self.delay.set_delay(delay)
              } else {
                self.delay.remove_delay()
              };
              if let Some(frames) = expired {
                self.dispatch_delayed(player_id, &frames).await?;
              }
            }
          }
        }
        res = self.delay.recv_expired(&mut delay_buf) => {
          match res {
            Ok(()) => {
              self.dispatch_delayed(player_id, &delay_buf).await?;
            }
            Err(err) => {
              tracing::debug!("delay: {}", err);
              break;
            }
          }
        }
      }
    }

    Ok(())
  }

  async fn dispatch_delayed<'a, I>(&mut self, player_id: i32, frames: I) -> Result<()>
  where
    I: IntoIterator<Item = &'a DelayedFrame>,
  {
    let mut out_buf_write = false;
    let iter = frames.into_iter();
    let mut deferred_in_frames: Option<Vec<PeerMsg>> = None;
    for frame in iter {
      match frame {
        DelayedFrame::In(frame) => {
          let msg = PeerMsg::Incoming {
            player_id,
            frame: frame.clone(),
          };
          if let Some(ref mut buf) = deferred_in_frames {
            buf.push(msg);
          } else {
            match self.dispatcher_tx.try_send(msg) {
              Ok(_) => {}
              Err(TrySendError::Full(msg)) => {
                deferred_in_frames.get_or_insert_with(|| vec![]).push(msg);
              }
              Err(TrySendError::Closed(_)) => return Err(Error::Cancelled),
            }
          }
        }
        DelayedFrame::Out(frame) => {
          if !out_buf_write {
            self.delay_send_buf.clear();
            out_buf_write = true;
          }
          self.delay_send_buf.push(frame.clone());
        }
      }
    }
    if out_buf_write {
      self
        .stream
        .get_mut()
        .send_frames(self.delay_send_buf.iter().cloned())
        .await?;
    }
    if let Some(frames) = deferred_in_frames {
      for frame in frames {
        self
          .dispatcher_tx
          .send(frame)
          .await
          .map_err(|_| Error::Cancelled)?;
      }
    }
    Ok(())
  }
}

#[derive(Debug)]
enum DispatchResult {
  Continue,
  Lag(Tick),
}
