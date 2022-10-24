use super::clock::ActionTickStream;
use super::delay::{DelayedFrame, DelayedFrameStream};
use super::delay_equalizer::DelayEqualizer;
use super::player::{PlayerDispatchInfo, PlayerSendError};
use super::sync::SyncMap;
use super::{broadcast, GameHostOptions};
use crate::error::*;
use crate::game::host::clock::Tick;
use crate::game::host::stream::{PlayerStream, PlayerStreamCmd, PlayerStreamHandle};
use crate::game::host::sync::{ClockResult, PlayerDesync};
use crate::game::{
  AckError, GameEvent, GameEventSender, PlayerBanType, PlayerSlot, SlotClientStatus,
  SlotClientStatusUpdateSource,
};
use crate::observer::ObserverPublisherHandle;
use flo_net::packet::{Frame, PacketTypeId};
use flo_net::ping::{PingMsg, PingStream};
use flo_net::w3gs::{W3GSFrameExt, W3GSMetadata, W3GSPacket, W3GSPacketTypeId};
use flo_observer::record::{RTTStats, RTTStatsItem};
use flo_util::chat::{parse_chat_command, ChatCommand};
use flo_w3gs::action::{IncomingAction, IncomingAction2, OutgoingKeepAlive};
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
use std::time::{Duration, Instant};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{oneshot, watch, Notify};
use tokio::time::{interval_at, sleep, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing_futures::Instrument;

use std::iter::Iterator;

const DISPATCH_ACTIONS_MTU: usize = 1350 - 8;

#[derive(Debug)]
pub enum Cmd {
  RegisterStream {
    stream: PlayerStream,
    tx: oneshot::Sender<Result<PlayerStreamHandle>>,
  },
  RemovePlayer {
    player_id: i32,
    leave_reason: Option<LeaveReason>,
  },
}

enum PeerMsg {
  Incoming {
    player_id: i32,
    frame: Frame,
  },
  Closed {
    player_id: i32,
    stream_id: u64,
  },
  Shutdown {
    player_id: i32,
    leave_reason: Option<LeaveReason>,
  },
  Pong {
    player_id: i32,
    rtt: u32,
  },
}

impl PeerMsg {
  fn player_id(&self) -> i32 {
    match *self {
      PeerMsg::Incoming { player_id, .. } => player_id,
      PeerMsg::Closed { player_id, .. } => player_id,
      PeerMsg::Shutdown { player_id, .. } => player_id,
      PeerMsg::Pong { player_id, .. } => player_id,
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
  pub fn new(
    game_id: i32,
    opts: GameHostOptions,
    slots: &[PlayerSlot],
    obs: ObserverPublisherHandle,
    out_tx: GameEventSender,
  ) -> Self {
    let ct = CancellationToken::new();
    let start_notify = Arc::new(Notify::new());
    let (status_tx, status_rx) = watch::channel(DispatchStatus::Pending);
    let (cmd_tx, cmd_rx) = channel(10);
    let (action_tx, action_rx) = channel(32);
    let enabled_ping_equalizer = opts.enabled_ping_equalizer;

    let state = State::new(
      game_id,
      opts,
      slots,
      obs.clone(),
      status_rx,
      action_tx.clone(),
      ct.clone(),
    );

    let mut start_messages = vec![];

    if enabled_ping_equalizer {
      start_messages.push("Ping equalizer is enabled.".to_string());
    }

    let chat_banned_player_names: Vec<String> = state
      .chat_banned_player_ids
      .iter()
      .flat_map(|id| state._player_name_lookup.get(&id).cloned())
      .collect();

    if !chat_banned_player_names.is_empty() {
      start_messages.push(format!(
        "Some players in this game have been muted: {}",
        chat_banned_player_names.join(", ")
      ));
    }

    tokio::spawn(
      Self::tick(
        game_id,
        state.shared.clone(),
        start_messages,
        start_notify.clone(),
        status_tx,
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

  pub async fn notify_player_shutdown(
    &self,
    player_id: i32,
    leave_reason: Option<LeaveReason>,
  ) -> Result<()> {
    self
      .cmd_tx
      .send(Cmd::RemovePlayer {
        player_id,
        leave_reason,
      })
      .await
      .map_err(|_| Error::Cancelled)?;
    Ok(())
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
              tracing::error!(player_id, "player removed: dispatch peer: {}", err);
              state.shared.lock().remove_player_and_broadcast(player_id, None).ok();
            },
          }
        }
        Some(cmd) = rx.recv() => {
          match state.dispatch_cmd(cmd, &peer_tx, &mut action_tx, &mut out_tx).await {
            Ok(_) => {},
            Err(Error::Cancelled) => {},
            Err(err) => {
              tracing::error!("dispatch cmd: {}", err);
            },
          }
        }
      }
    }

    state.shared.lock().obs.remove_game(state.game_id);
  }

  async fn tick(
    game_id: i32,
    shared: Arc<Mutex<Shared>>,
    start_messages: Vec<String>,
    start_notify: Arc<Notify>,
    status_tx: watch::Sender<DispatchStatus>,
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
      shared.lock().set_started();
      status_tx.send(DispatchStatus::Running).ok();

      if !start_messages.is_empty() {
        let mut shared = shared.lock();
        for msg in start_messages {
          shared.broadcast_message(msg);
        }
      }

      let mut tick_stream = ActionTickStream::new(*crate::constants::GAME_DEFAULT_STEP_MS);
      let pause_timeout = sleep(Duration::from_secs(0));
      tokio::pin!(pause_timeout);

      {
        let ct = ct.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
          let base_time = tokio::time::Instant::from_std(Instant::now());
          let mut stream = interval_at(
            base_time + crate::constants::RTT_STATS_REPORT_DELAY,
            crate::constants::RTT_STATS_REPORT_INTERVAL,
          );
          stream.set_missed_tick_behavior(MissedTickBehavior::Skip);
          loop {
            tokio::select! {
              _ = ct.cancelled() => {
                break;
              }
              now = stream.tick() => {
                let time = now.saturating_duration_since(base_time).as_millis();
                shared.lock().push_rtt_stats(time as _);
              }
            }
          }
        });
      }

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
              ActionMsg::CheckStopLag => {
                if tick_stream.is_paused() {
                  match shared.lock().check_stop_lag() {
                    Ok(true) => {
                      tick_stream.resume();
                      status_tx.send(DispatchStatus::Running).ok();
                      tracing::info!(
                        game_id,
                        "resume clock: all lagging player resumed"
                      );
                    },
                    Err(err) => {
                      tracing::error!("check_stop_lag: {}", err);
                    },
                    _ => {}
                  }
                }
              },
              ActionMsg::ResumeClock => {
                tracing::info!(
                  game_id,
                  "resume clock"
                );
                tick_stream.resume();
                status_tx.send(DispatchStatus::Running).ok();
              }
            }
          }
          Some(tick) = tick_stream.next() => {
            match shared.lock().dispatch_action_tick(tick) {
              Ok(DispatchResult::Continue) => {},
              Ok(DispatchResult::Lag(tick)) => {
                tick_stream.replace_actions(tick.actions);
                pause_timeout.as_mut().reset((Instant::now() + crate::constants::GAME_CLOCK_MAX_PAUSE).into());
                tick_stream.pause();
                status_tx.send(DispatchStatus::Paused).ok();
              }
              Err(err) => {
                tracing::error!(
                  game_id,
                  "dispatch action tick: {}", err
                );
                break;
              }
            }
          }
          _ = &mut pause_timeout, if tick_stream.is_paused() => {
            if let Err(err) = shared.lock().drop_all_lag_players() {
              tracing::error!(
                game_id,
                "drop all lag players: {}", err
              );
              break;
            }
            tick_stream.resume();
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
  CheckStopLag,
  ResumeClock,
}

#[derive(Debug)]
struct State {
  game_id: i32,
  ct: CancellationToken,
  shared: Arc<Mutex<Shared>>,
  status_rx: watch::Receiver<DispatchStatus>,
  game_player_id_lookup: BTreeMap<u8, i32>,
  _player_name_lookup: BTreeMap<i32, String>,
  chat_banned_player_ids: Vec<i32>,
  left_players: BTreeSet<i32>,
}

impl State {
  fn new(
    game_id: i32,
    opts: GameHostOptions,
    slots: &[PlayerSlot],
    obs: ObserverPublisherHandle,
    status_rx: watch::Receiver<DispatchStatus>,
    _action_tx: Sender<ActionMsg>,
    ct: CancellationToken,
  ) -> Self {
    let delay_equalizer = if opts.enabled_ping_equalizer {
      Some(DelayEqualizer::new(
        slots.iter().filter(|s| s.settings.team != 24).count(),
      ))
    } else {
      None
    };
    State {
      game_id,
      ct,
      shared: Arc::new(Mutex::new(Shared::new(
        game_id,
        slots,
        obs,
        delay_equalizer,
      ))),
      status_rx,
      game_player_id_lookup: slots
        .into_iter()
        .map(|slot| ((slot.id + 1) as u8, slot.player.player_id))
        .collect(),
      _player_name_lookup: slots
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
    action_tx: &mut Sender<ActionMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    match cmd {
      Cmd::RegisterStream { stream, tx } => {
        tx.send(
          self
            .register_stream(stream, peer_tx, action_tx, out_tx)
            .await,
        )
        .ok();
      }
      Cmd::RemovePlayer {
        player_id,
        leave_reason,
      } => {
        if let Err(err) = peer_tx
          .send(PeerMsg::Shutdown {
            player_id,
            leave_reason,
          })
          .await
        {
          tracing::error!(game_id = self.game_id, player_id, "send shutdown: {}", err);
        }
      }
    }

    Ok(())
  }

  async fn register_stream(
    &mut self,
    stream: PlayerStream,
    peer_tx: &Sender<PeerMsg>,
    _action_tx: &mut Sender<ActionMsg>,
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

    let (status, delay, reconnected, resend_frames) = {
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
        player.update_lag_ms_after_reconnect();
        guard.broadcast_message(msg);
        (
          if *self.status_rx.borrow() != DispatchStatus::Pending {
            SlotClientStatus::Loaded
          } else {
            SlotClientStatus::Connected
          },
          delay,
          reconnected,
          resend_frames,
        )
      } else {
        (SlotClientStatus::Connected, delay, reconnected, None)
      }
    };

    if reconnected {
      tracing::info!(game_id = self.game_id, player_id, "reconnected");
    }

    out_tx
      .send(GameEvent::PlayerStatusChange(
        player_id,
        status,
        SlotClientStatusUpdateSource::Node,
      ))
      .await
      .map_err(|_| Error::Cancelled)?;

    let mut worker = PeerWorker::new(
      self.game_id,
      self.ct.clone(),
      stream,
      self.status_rx.clone(),
      peer_cmd_rx,
      peer_tx.clone(),
      delay,
    );
    tokio::spawn(
      async move {
        crate::metrics::PLAYERS_CONNECTIONS.inc();

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

        crate::metrics::PLAYERS_CONNECTIONS.dec();
      }
      .instrument(tracing::info_span!("peer", game_id, player_id)),
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
        tracing::debug!(player_id, "player stream closed: {}", stream_id);
        if self.left_players.contains(&player_id) {
          return Ok(());
        }

        let res = {
          let mut guard = self.shared.lock();
          guard.handle_peer_stream_close(player_id)?
        };

        let next_status = match res {
          ClosePlayerStreamResult::ClosedDisconnected => SlotClientStatus::Disconnected,
          ClosePlayerStreamResult::ClosedLeft => {
            self.left_players.insert(player_id);
            SlotClientStatus::Left
          }
          ClosePlayerStreamResult::Skipped => {
            tracing::warn!(
              game_id = self.game_id,
              player_id,
              stream_id,
              "close player stream: player stream already removed"
            );
            SlotClientStatus::Disconnected
          }
          ClosePlayerStreamResult::ClosedLagging => {
            tracing::warn!(
              game_id = self.game_id,
              player_id,
              stream_id,
              "lagging player stream closed"
            );
            action_tx
              .send(ActionMsg::CheckStopLag)
              .await
              .map_err(|_| Error::Cancelled)?;
            SlotClientStatus::Left
          }
        };

        tracing::debug!(
          game_id = self.game_id,
          player_id,
          "next client status: {:?}",
          next_status
        );

        out_tx
          .send(GameEvent::PlayerStatusChange(
            player_id,
            next_status,
            SlotClientStatusUpdateSource::Node,
          ))
          .await
          .map_err(|_| Error::Cancelled)?;
      }
      PeerMsg::Shutdown {
        player_id,
        leave_reason,
      } => {
        if !self.left_players.contains(&player_id) {
          let force = leave_reason.is_none();
          self
            .handle_player_leave(player_id, leave_reason, action_tx, out_tx)
            .await?;
          if force {
            tracing::warn!(game_id = self.game_id, player_id, "player force shutdown");
          } else {
            tracing::info!(game_id = self.game_id, player_id, "player shutdown");
          }
        }
      }
      PeerMsg::Pong { player_id, rtt } => {
        self.handle_pong(player_id, rtt);
      }
    }
    Ok(())
  }

  fn handle_pong(&mut self, player_id: i32, rtt: u32) {
    let mut shared = self.shared.lock();
    let delay = if shared.delay_equalizer.is_some() {
      if shared.active_players.contains(&player_id) {
        shared
          .delay_equalizer
          .as_mut()
          .and_then(|de| de.insert_rtt(player_id, rtt))
      } else {
        None
      }
    } else {
      None
    };
    shared.get_player(player_id).map(|info| {
      info.push_rtt(rtt);
      if let Some(delay) = delay {
        tracing::debug!(player_id, "auto set delay: {}", delay);
        if delay > 0 {
          info
            .set_delay(Duration::from_millis(delay as _).into())
            .ok();
        } else {
          info.set_delay(None).ok();
        }
      }
    });
  }

  async fn dispatch_incoming_w3gs(
    &mut self,
    player_id: i32,
    meta: W3GSMetadata,
    packet: Packet,
    action_tx: &mut Sender<ActionMsg>,
    _out_tx: &mut GameEventSender,
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
      PacketTypeId::DropReq => {
        tracing::info!(game_id = self.game_id, player_id, "drop request");
        let res = self.shared.lock().request_drop(player_id)?;
        match res {
          RequestDropResult::NoLaggingPlayer | RequestDropResult::Voting => {}
          RequestDropResult::Done => {
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
        let res = self.shared.lock().ack(player_id, checksum);
        match res {
          Ok(AckAction::Continue) => {}
          Ok(AckAction::CheckStopLag) => {
            action_tx
              .send(ActionMsg::CheckStopLag)
              .await
              .map_err(|_| Error::Cancelled)?;
          }
          Err(err) => {
            tracing::error!(
              game_id = self.game_id,
              player_id,
              "sync ack error: {:?}",
              err
            );
          }
        }
      }
      id => {
        tracing::warn!("unexpected w3gs packet id = {:?}", id);
      }
    }

    Ok(())
  }

  async fn handle_player_leave(
    &mut self,
    player_id: i32,
    reason: Option<LeaveReason>,
    action_tx: &mut Sender<ActionMsg>,
    out_tx: &mut GameEventSender,
  ) -> Result<()> {
    self.left_players.insert(player_id);

    let should_check_lag = {
      let mut guard = self.shared.lock();
      let player = guard
        .get_player(player_id)
        .ok_or_else(|| Error::PlayerNotFoundInGame)?;
      player.send_w3gs(Packet::simple(LeaveAck)?).ok();
      guard.remove_player_and_broadcast(player_id, reason)?;
      guard.lagging_player_ids.contains(&player_id)
    };

    if should_check_lag {
      action_tx
        .send(ActionMsg::CheckStopLag)
        .await
        .map_err(|_err| Error::Cancelled)?;
    }

    out_tx
      .send(GameEvent::PlayerStatusChange(
        player_id,
        SlotClientStatus::Left,
        SlotClientStatusUpdateSource::Node,
      ))
      .await
      .map_err(|_| Error::Cancelled)?;
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
    {
      let mut guard = self.shared.lock();
      guard.obs.push_w3gs(self.game_id, packet.clone());
      guard.broadcast(
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
    }
    Ok(())
  }

  async fn handle_command(
    &self,
    action_tx: &mut Sender<ActionMsg>,
    player_id: i32,
    cmd: ChatCommand<'_>,
  ) -> Result<bool> {
    let debug = cfg!(debug_assertions);
    match cmd.name() {
      "drop" if debug => {
        let shared = self.shared.clone();
        tokio::spawn(async move {
          shared
            .lock()
            .get_player(player_id)
            .map(|v| v.close_stream());
        });
      }
      "block" if debug => {
        if let Some(Some((ms,))) = cmd.parse_arguments::<Option<(u64,)>>().ok() {
          self.shared.clone().lock().get_player(player_id).map(|p| {
            p.set_block(Duration::from_millis(ms)).ok();
            tracing::debug!(player_id, "block for {}ms", ms);
          });
        } else {
          self
            .shared
            .lock()
            .private_message(player_id, "Invalid syntax, usage: !block 30");
        }
      }
      "delay" => {
        if let Some(Some((ms,))) = cmd.parse_arguments::<Option<(u16,)>>().ok() {
          let [min, max] = crate::constants::GAME_DELAY_RANGE;

          if ms == 0 {
            let mut guard = self.shared.lock();
            if !debug && guard.delay_equalizer.is_some() {
              self.shared.lock().private_message(
                player_id,
                format!("Cannot set delay because ping equalizer is enabled"),
              );
              return Ok(true);
            }
            match guard
              .get_player(player_id)
              .map(|player| -> Result<_> {
                player.set_delay(None)?;
                Ok(player.player_name().to_string())
              })
              .transpose()
            {
              Ok(name) => {
                if let Some(name) = name {
                  guard.broadcast_message(format!("Removed delay for {}", name));
                }
              }
              Err(_) => {}
            };
            return Ok(true);
          }

          let duration = Duration::from_millis(ms as _);
          if duration < min || duration > max {
            self.shared.lock().private_message(
              player_id,
              format!(
                "Invalid value, range {} - {}",
                min.as_millis(),
                max.as_millis()
              ),
            );
            return Ok(true);
          }

          {
            let mut guard = self.shared.lock();
            match guard
              .get_player(player_id)
              .map(|player| -> Result<_> {
                player.set_delay(Some(duration))?;
                Ok(player.player_name().to_string())
              })
              .transpose()
            {
              Ok(name) => {
                if let Some(name) = name {
                  guard.broadcast_message(format!("Set delay for {}: {}ms", name, ms));
                }
              }
              Err(_) => {}
            };
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
      "desync" if debug => {
        let mut lock = self.shared.lock();
        if let Some(player) = lock.get_player(player_id) {
          let pkt = W3GSPacket::with_payload(IncomingAction(TimeSlot {
            time_increment_ms: 1000,
            actions: vec![],
          }))?;
          player.send_w3gs(pkt).ok();
        }
      }
      "rtt" => {
        let mut lock = self.shared.lock();
        let msgs: Vec<_> = lock
          .map
          .values()
          .map(|v| {
            format!(
              "{}: {}",
              v.player_name(),
              match v.rtt() {
                Some(v) => format!(
                  "{:.1}ms (min: {}, max: {}, samples: {})",
                  v.avg, v.min, v.max, v.ticks
                ),
                None => "N/A".to_string(),
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
      "conn" if debug => {
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
      "step" if debug => match cmd.parse_arguments::<(u16,)>().ok() {
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
      "sync" if debug => {
        tracing::debug!("{}", self.shared.lock().sync.debug_pending());
      }
      _ => return Ok(false),
    };
    Ok(true)
  }
}

#[derive(Debug)]
struct Shared {
  game_id: i32,
  started: bool,
  map: BTreeMap<i32, PlayerDispatchInfo>,
  slot_id_lookup: BTreeMap<i32, u8>,
  sync: SyncMap,
  lagging_player_ids: BTreeSet<i32>,
  drop_votes: BTreeSet<i32>,
  obs: ObserverPublisherHandle,
  active_players: BTreeSet<i32>,
  delay_equalizer: Option<DelayEqualizer>,
}

impl Shared {
  fn new(
    game_id: i32,
    slots: &[PlayerSlot],
    obs: ObserverPublisherHandle,
    delay_equalizer: Option<DelayEqualizer>,
  ) -> Self {
    let sync = SyncMap::new(slots.iter().map(|s| s.player.player_id).collect());
    let mut slot_id_lookup = BTreeMap::new();
    let mut active_players = BTreeSet::new();
    Self {
      game_id,
      started: false,
      map: slots
        .into_iter()
        .map(|slot| {
          let p = PlayerDispatchInfo::new(slot);
          slot_id_lookup.insert(slot.player.player_id, p.slot_player_id());
          if !p.is_observer() {
            active_players.insert(slot.player.player_id);
          }
          (slot.player.player_id, p)
        })
        .collect(),
      slot_id_lookup,
      sync,
      lagging_player_ids: BTreeSet::new(),
      drop_votes: BTreeSet::new(),
      obs,
      active_players,
      delay_equalizer,
    }
  }

  fn set_started(&mut self) {
    self.started = true;
  }

  fn get_player(&mut self, player_id: i32) -> Option<&mut PlayerDispatchInfo> {
    self.map.get_mut(&player_id)
  }

  #[must_use]
  pub fn dispatch_action_tick(&mut self, mut tick: Tick) -> Result<DispatchResult> {
    let time_increment_ms = tick.time_increment_ms;
    if let ClockResult::Lag(timeouts) = self.sync.clock(time_increment_ms) {
      let player_ids: Vec<_> = timeouts.into_iter().map(|t| t.player_id).collect();
      if self.handle_lag(player_ids)? {
        return Ok(DispatchResult::Lag(tick));
      }
    }

    if tick.actions_bytes_len > DISPATCH_ACTIONS_MTU {
      tracing::debug!(
        "over-sized actions: tick = {}, size = {}, len = {}",
        self.sync.tick(),
        tick.actions_bytes_len,
        tick.actions.len(),
      );
      let mut remaining_size = tick.actions_bytes_len;
      while remaining_size > DISPATCH_ACTIONS_MTU {
        let mut actions_size = 0;
        let mut time_slot = TimeSlot {
          time_increment_ms: 0,
          actions: vec![],
        };
        while let Some((action_player_id, size)) =
          tick.actions.first().map(|a| (a.player_id, a.byte_len()))
        {
          if size > DISPATCH_ACTIONS_MTU {
            tick.actions.remove(0);
            remaining_size -= size;
            tracing::warn!(
              game_id = self.game_id,
              action_player_id,
              "over-sized action dropped: {}",
              size
            );
            break;
          }

          if actions_size + size > DISPATCH_ACTIONS_MTU {
            tracing::debug!(
              "fragment actions: tick = {}, size = {}, len = {}, remaining_size = {}",
              self.sync.tick(),
              actions_size,
              time_slot.actions.len(),
              remaining_size
            );
            let action_packet = Packet::with_payload(IncomingAction2(time_slot))?;
            self.obs.push_w3gs(self.game_id, action_packet.clone());
            self.broadcast(action_packet, broadcast::Everyone)?;
            break;
          }

          let action = tick.actions.remove(0);
          remaining_size -= size;
          actions_size += size;
          time_slot.actions.push(action);
        }
      }
    }
    let action_packet = Packet::with_payload(IncomingAction(TimeSlot {
      time_increment_ms,
      actions: tick.actions,
    }))?;
    self.obs.push_w3gs(self.game_id, action_packet.clone());
    self.broadcast(action_packet, broadcast::Everyone)?;
    Ok(DispatchResult::Continue)
  }

  fn push_rtt_stats(&mut self, time: u32) {
    let items = self.map.iter_mut().map(|(id, info)| {
      let stats = info.take_rtt();
      RTTStatsItem {
        player_id: *id,
        ticks: stats.ticks,
        min: stats.min,
        max: stats.max,
        avg: stats.avg,
      }
    });

    self
      .obs
      .push_rtt_stat(self.game_id, RTTStats::new(time, items))
  }

  fn handle_lag(&mut self, add_player_ids: Vec<i32>) -> Result<bool> {
    self.lagging_player_ids.extend(add_player_ids);
    self.obs.push_start_lag(
      self.game_id,
      self.lagging_player_ids.iter().cloned().collect(),
    );
    if let Some(items) = self.refresh_lag_packet()? {
      self.drop_votes.clear();
      let mut send_errors = vec![];
      for (recv_player_id, info) in &mut self.map {
        if !items.iter().any(|(v, _, _)| v == recv_player_id) {
          let mut start_lag_players = vec![];
          let mut start_lag_player_ids = vec![];
          for (lag_player, lag_slot, lag_duration_ms) in &items {
            start_lag_player_ids.push(*lag_player);
            start_lag_players.push(LagPlayer {
              player_id: *lag_slot,
              lag_duration_ms: *lag_duration_ms,
            });
          }
          if !start_lag_players.is_empty() {
            tracing::info!(
              game_id = self.game_id,
              player_id = recv_player_id,
              "send start lag: {:?}",
              start_lag_player_ids
            );
            info.set_lag_slots(start_lag_players.iter().map(|v| v.player_id));
            info
              .send_w3gs(W3GSPacket::simple(StartLag::new(start_lag_players))?)
              .err()
              .map(|err| send_errors.push((*recv_player_id, err)));
          }
        }
      }
      if !send_errors.is_empty() {
        self.handle_player_send_errors(send_errors)?;
      }
      Ok(true)
    } else {
      Ok(false)
    }
  }

  fn check_stop_lag(&mut self) -> Result<bool> {
    tracing::debug!(
      "check lag players: {:?}, current: {:?}",
      self.lagging_player_ids,
      self.map.keys().collect::<Vec<_>>()
    );
    if self.lagging_player_ids.is_empty() {
      return Ok(false);
    }
    let mut stop_lag_players = vec![];
    let mut packets = vec![];
    for id in self.lagging_player_ids.clone() {
      let info = if let Some(info) = self.map.get_mut(&id) {
        let reconnected =
          info.stream_id().is_some() && self.sync.player_pending_ticks(id) == Some(0);
        if reconnected {
          Some((info.slot_player_id(), info.end_lag()))
        } else {
          None
        }
      } else {
        // left
        self.slot_id_lookup.get(&id).cloned().map(|slot| (slot, 0))
      };
      if let Some((slot, lag_duration_ms)) = info {
        self.obs.push_end_lag(self.game_id, id);
        self.lagging_player_ids.remove(&id);
        stop_lag_players.push(id);
        packets.push((
          slot,
          W3GSPacket::simple(StopLag(LagPlayer {
            player_id: slot,
            lag_duration_ms,
          }))?,
        ));
      }
    }
    for (slot, pkt) in packets {
      let targets = self
        .map
        .iter_mut()
        .filter_map(|(id, p)| {
          if p.remove_lag_slot(slot) {
            Some(*id)
          } else {
            None
          }
        })
        .collect::<Vec<_>>();
      tracing::info!(
        game_id = self.game_id,
        "send stop lag to {:?}: {:?}",
        targets,
        stop_lag_players
      );
      self.broadcast(pkt, broadcast::AllowList(&targets))?;
    }

    // tracing::debug!("remaining lag players: {:?}", self.lagging_player_ids);
    Ok(self.lagging_player_ids.is_empty())
  }

  fn refresh_lag_packet(&mut self) -> Result<Option<Vec<(i32, u8, u32)>>> {
    let items: Vec<_> = self
      .lagging_player_ids
      .clone()
      .into_iter()
      .filter_map(|player_id| {
        let player = self.get_player(player_id)?;
        let slot_player_id = player.slot_player_id();
        Some((player_id, slot_player_id, player.start_lag()))
      })
      .collect();
    if items.is_empty() {
      return Ok(None);
    }

    if !items.is_empty() {
      tracing::warn!("lag: items = {:?}, time = {}", items, self.sync.time());
    }

    Ok(Some(items))
  }

  fn handle_peer_stream_close(&mut self, player_id: i32) -> Result<ClosePlayerStreamResult> {
    if let Some(stream) = self.map.get_mut(&player_id).and_then(|v| {
      v.set_last_disconnect();
      v.take_stream()
    }) {
      if self.started {
        tracing::warn!(game_id = self.game_id, player_id, "player disconnected");
        stream.close();
        // don't need to check `lagging_player_ids`
        // because disconnect does not change lag status
        Ok(ClosePlayerStreamResult::ClosedDisconnected)
      } else {
        tracing::warn!(
          game_id = self.game_id,
          player_id,
          "player dropped before game start"
        );
        self.remove_player_and_broadcast(player_id, None)?;
        if self.lagging_player_ids.contains(&player_id) {
          Ok(ClosePlayerStreamResult::ClosedLagging)
        } else {
          Ok(ClosePlayerStreamResult::ClosedLeft)
        }
      }
    } else {
      Ok(ClosePlayerStreamResult::Skipped)
    }
  }

  fn remove_player_and_broadcast(
    &mut self,
    player_id: i32,
    reason: Option<LeaveReason>,
  ) -> Result<()> {
    self
      .delay_equalizer
      .as_mut()
      .map(|de| de.remove_player(player_id));
    let mut player = if let Some(v) = self.map.remove(&player_id) {
      v
    } else {
      return Ok(());
    };

    tracing::info!(game_id = self.game_id, player_id, "remove player");

    for p in self.map.values_mut() {
      p.remove_lag_slot(player.slot_player_id());
    }
    let pkt = Packet::simple(PlayerLeft {
      player_id: player.slot_player_id(),
      reason: reason.unwrap_or(LeaveReason::LeaveDisconnect),
    })?;
    self.obs.push_w3gs(self.game_id, pkt.clone());

    self.broadcast(pkt, broadcast::DenyList(&[player_id]))?;
    if let Some(desync) = self.sync.remove_player(player_id) {
      tracing::warn!(
        player_id,
        "desync detected after disconnecting player: {:?}",
        desync
      );
      tracing::warn!("{}", self.sync.debug_pending())
    }
    player.close_stream();
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

          let res = if info.stream_id().is_some() {
            info.send_w3gs(packet.clone()).err().map(|err| {
              info.close_stream();
              (*player_id, err)
            })
          } else {
            info.enqueue_w3gs(packet.clone());
            None
          };

          if info.ack_queue().pending_ack_len() >= crate::constants::GAME_PLAYER_MAX_ACK_QUEUE {
            return Some((*player_id, PlayerSendError::AckQueueFull));
          }

          res
        })
        .collect()
    };

    if !errors.is_empty() {
      self.handle_player_send_errors(errors)?;
    }

    Ok(())
  }

  pub fn handle_player_send_errors(&mut self, errors: Vec<(i32, PlayerSendError)>) -> Result<()> {
    for (player_id, err) in errors {
      match err {
        PlayerSendError::Closed(_frame) => {
          tracing::info!(game_id = self.game_id, player_id, "stream broken");
        }
        PlayerSendError::ChannelFull => {
          tracing::info!(game_id = self.game_id, player_id, "channel full");
        }
        PlayerSendError::AckQueueFull => {
          tracing::warn!(game_id = self.game_id, player_id, "ack queue full");
          self.remove_player_and_broadcast(player_id, None)?;
        }
        _ => {}
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
        "Drop player vote: {}/{}",
        self.drop_votes.len(),
        vote_required
      ));
    }
    if self.drop_votes.len() >= vote_required {
      self.drop_all_lag_players()?;
      Ok(RequestDropResult::Done)
    } else {
      Ok(RequestDropResult::Voting)
    }
  }

  pub fn drop_all_lag_players(&mut self) -> Result<()> {
    let drop_player_ids: Vec<_> = self.lagging_player_ids.iter().cloned().collect();
    for drop_player_id in &drop_player_ids {
      tracing::info!(
        game_id = self.game_id,
        player_id = *drop_player_id,
        "lagging player dropped."
      );
      self.remove_player_and_broadcast(*drop_player_id, None)?;
    }
    self.lagging_player_ids.clear();
    Ok(())
  }

  pub fn ack(&mut self, player_id: i32, checksum: u32) -> Result<AckAction> {
    let res = match self.sync.ack(player_id, checksum) {
      Ok(res) => {
        if let Some(checksum) = res.agreed_checksum.clone() {
          self
            .obs
            .push_tick_checksum(self.game_id, res.game_tick, checksum);
        }
        res
      }
      Err(err) => {
        tracing::error!(
          game_id = self.game_id,
          player_id,
          "desync: syn ack internal: {:?}: {}",
          err,
          self.sync.debug_pending()
        );

        match err {
          AckError::PlayerNotFound(_) => {}
          AckError::TickNotFound(desync) => {
            self.handle_desync(vec![desync])?;
          }
        }

        return if !self.lagging_player_ids.contains(&player_id) {
          Ok(AckAction::Continue)
        } else {
          Ok(AckAction::CheckStopLag)
        };
      }
    };
    let has_desync = res.desync.is_some();
    if let Some(desync) = res.desync {
      self.handle_desync(desync)?;
    }
    if !self.lagging_player_ids.contains(&player_id) && !has_desync {
      Ok(AckAction::Continue)
    } else {
      Ok(AckAction::CheckStopLag)
    }
  }

  fn handle_desync(&mut self, desync: Vec<PlayerDesync>) -> Result<()> {
    let mut handled = BTreeSet::new();
    let mut targets = vec![];
    for item in desync {
      if !handled.contains(&item.player_id) {
        handled.insert(item.player_id);

        tracing::warn!(
          game_id = self.game_id,
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
      self.remove_player_and_broadcast(player_id, None)?;
    }
    Ok(())
  }
}

enum AckAction {
  Continue,
  CheckStopLag,
}

enum RequestDropResult {
  NoLaggingPlayer,
  Voting,
  Done,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum DispatchStatus {
  Pending,
  Running,
  Paused,
}

struct PeerWorker {
  game_id: i32,
  ct: CancellationToken,
  stream: PlayerStream,
  status_rx: watch::Receiver<DispatchStatus>,
  in_rx: Receiver<PlayerStreamCmd>,
  dispatcher_tx: Sender<PeerMsg>,
  delay: DelayedFrameStream,
  delay_send_buf: Vec<Frame>,
  shutdown: bool,
}

impl PeerWorker {
  fn new(
    game_id: i32,
    ct: CancellationToken,
    stream: PlayerStream,
    status_rx: watch::Receiver<DispatchStatus>,
    in_rx: Receiver<PlayerStreamCmd>,
    out_tx: Sender<PeerMsg>,
    delay: Option<Duration>,
  ) -> Self {
    Self {
      game_id,
      ct,
      stream,
      status_rx,
      in_rx,
      dispatcher_tx: out_tx,
      delay: DelayedFrameStream::new(delay),
      delay_send_buf: Vec::new(),
      shutdown: false,
    }
  }

  async fn serve(&mut self, resend_frames: Option<Vec<Frame>>) -> Result<()> {
    let player_id = self.stream.player_id();
    let stream_ct = self.stream.token();

    if let Some(frames) = resend_frames {
      self.stream.get_mut().send_frames(frames).await?;
    }

    let mut delay_buf = VecDeque::new();
    let mut ping = PingStream::interval(
      crate::constants::GAME_PING_INTERVAL,
      crate::constants::GAME_PING_TIMEOUT,
    );
    let mut last_status = *self.status_rx.borrow();

    ping.start();

    loop {
      tokio::select! {
        _ = self.ct.cancelled() => {
          tracing::info!(
            game_id = self.game_id,
            player_id,
            "dispatcher cancelled"
          );
          break
        },
        _ = stream_ct.cancelled() => {
          tracing::info!(
            game_id = self.game_id,
            player_id,
            "stream cancelled"
          );
          break
        },
        Ok(_) = self.status_rx.changed() => {
          let status = *self.status_rx.borrow();
          if status != last_status {
            last_status = status;
            match status {
              DispatchStatus::Paused => {},
              DispatchStatus::Running => {},
              _ => {}
            }
          }
        }
        next = self.stream.get_mut().recv_frame() => {
          match next {
            Ok(frame) => {
              match frame.type_id {
                PacketTypeId::ClientShutdown => {
                  self.shutdown(&mut ping, player_id, None).await;
                  break;
                },
                PacketTypeId::W3GS if frame.payload.w3gs_type_id() == Some(W3GSPacketTypeId::LeaveReq) => {
                  let (_, packet) = frame.try_into_w3gs()?;
                  let req: LeaveReq = packet.decode_simple()?;
                  tracing::info!(
                    game_id = self.game_id,
                    player_id,
                    "leave reason: {:?}",
                    req.reason()
                  );
                  self.shutdown(&mut ping, player_id, Some(req.reason())).await;
                  break;
                }
                PingStream::PONG_TYPE_ID => {
                  if !self.delay.enabled() {
                    if ping.started() {
                      if let Some(rtt) = ping.capture_pong(frame) {
                        if self.dispatcher_tx.send(PeerMsg::Pong {
                          player_id,
                          rtt
                        }).await.is_err() {
                          break;
                        }
                      }
                    }
                    continue;
                  }
                },
                _ => {}
              }

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
                self.delay.set_delay(delay);
                None
              } else {
                self.delay.remove_delay()
              };
              if let Some(frames) = expired {
                self.dispatch_delayed(&mut ping, player_id, &frames).await?;
              }
            }
            PlayerStreamCmd::SetBlock(duration) => {
              tokio::select! {
                _ = stream_ct.cancelled() => {
                  break
                }
                _ = tokio::time::sleep(duration) => {}
              }
            }
          }
        }
        _ = self.delay.recv_expired(&mut delay_buf), if self.delay.enabled() => {
          self.dispatch_delayed(&mut ping, player_id, &delay_buf).await?;
          delay_buf.clear();
        }
        Some(next) = ping.next(), if ping.started() => {
          match next {
            PingMsg::Ping(frame) => {
              if self.delay.enabled() {
                self.delay.insert(DelayedFrame::Out(frame));
              } else {
                self.stream.get_mut().send_frame(frame).await?;
              }
            },
            PingMsg::Timeout => {
              tracing::info!(
                game_id = self.game_id,
                player_id,
                "ping timeout"
              );
              break;
            }
          }
        }
      }
    }

    Ok(())
  }

  async fn shutdown(
    &mut self,
    ping: &mut PingStream,
    player_id: i32,
    leave_reason: Option<LeaveReason>,
  ) {
    if self.shutdown {
      return;
    }
    self.shutdown = true;
    if self.delay.enabled() {
      if let Some(frames) = self.delay.remove_delay() {
        let in_frames = frames
          .into_iter()
          .filter_map(|frame| {
            if let DelayedFrame::In(_) = frame {
              Some(frame)
            } else {
              None
            }
          })
          .collect::<Vec<_>>();
        let res = self.dispatch_delayed(ping, player_id, &in_frames).await;
        if let Err(err) = res {
          tracing::error!(
            game_id = self.game_id,
            player_id,
            "flush delayed frames: {}",
            err
          );
        }
      }
    }
    let frame = Frame::new_empty(PacketTypeId::ClientShutdownAck);
    if let Err(err) = self.stream.get_mut().send_frame(frame).await {
      tracing::error!(
        game_id = self.game_id,
        player_id,
        "send shutdown ack: {}",
        err
      );
    }
    if let Err(err) = self.stream.get_mut().shutdown().await {
      tracing::error!(game_id = self.game_id, player_id, "shutdown: {}", err);
    }

    self
      .dispatcher_tx
      .send(PeerMsg::Shutdown {
        player_id,
        leave_reason,
      })
      .await
      .ok();
  }

  async fn dispatch_delayed<'a, I>(
    &mut self,
    ping: &mut PingStream,
    player_id: i32,
    frames: I,
  ) -> Result<()>
  where
    I: IntoIterator<Item = &'a DelayedFrame>,
  {
    let mut out_buf_write = false;
    let iter = frames.into_iter();
    let mut deferred_in_frames: Option<Vec<PeerMsg>> = None;
    for frame in iter {
      match frame {
        DelayedFrame::In(frame) => {
          if frame.type_id == PingStream::PONG_TYPE_ID {
            if ping.started() {
              if let Some(rtt) = ping.capture_pong(frame.clone()) {
                self
                  .dispatcher_tx
                  .send(PeerMsg::Pong { player_id, rtt })
                  .await
                  .ok();
              }
              continue;
            }
          }

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

#[derive(Debug)]
enum ClosePlayerStreamResult {
  ClosedDisconnected,
  ClosedLeft,
  ClosedLagging,
  Skipped,
}
