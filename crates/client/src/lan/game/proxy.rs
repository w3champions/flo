use crate::controller::{ControllerClient, GetWeakOutgoingMessageSender};
use crate::error::*;
use crate::lan::game::game::GameHandler;
use crate::lan::game::lobby::{LobbyAction, LobbyHandler};
use crate::lan::game::slot::index_to_player_id;
use crate::lan::game::LanGameInfo;
use crate::lan::LanEvent;
use crate::messages::OutgoingMessage;
use crate::node::stream::{NodeConnectToken, NodeStream, NodeStreamSender};
use crate::node::NodeInfo;
use flo_state::Addr;
use flo_task::{SpawnScope, SpawnScopeHandle};
use flo_types::node::{NodeGameStatus, SlotClientStatus};
use flo_w3gs::constants::LeaveReason;
use flo_w3gs::net::{W3GSListener, W3GSStream};
use flo_w3gs::protocol::constants::PacketTypeId;
use flo_w3gs::protocol::game::{GameLoadedSelf, PlayerLoaded};
use flo_w3gs::protocol::leave::{LeaveAck, LeaveReq};
use flo_w3gs::protocol::packet::Packet;
use flo_w3gs::protocol::packet::*;
use flo_w3gs::protocol::ping::{PingFromHost, PongToHost};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, Receiver, Sender, WeakSender};
use tokio::sync::{oneshot, watch};
use tokio::time::interval;
use tokio_stream::StreamExt;
use tracing_futures::Instrument;

const LOAD_SCREEN_PING_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub enum GameEndReason {
  Unknown,
  LeaveReq(LeaveReason),
}

pub struct LanProxy {
  _scope: SpawnScope,
  node_stream: NodeStream,
  port: u16,
  status_tx: watch::Sender<Option<NodeGameStatus>>,
  event_tx: Sender<PlayerEvent>,
}

impl LanProxy {
  pub async fn start(
    info: LanGameInfo,
    node: Arc<NodeInfo>,
    token: NodeConnectToken,
    client: Addr<ControllerClient>,
    game_version_string: String,
    save_replay: bool
  ) -> Result<Self> {
    let scope = SpawnScope::new();
    let listener = W3GSListener::bind().await?;
    let port = listener.port();
    let (status_tx, status_rx) = watch::channel(None);
    let (event_tx, event_rx) = channel(10);
    let (w3gs_tx, w3gs_rx) = channel(32);
    let game_id = info.game.game_id;

    tracing::debug!("connecting to node: {}", node.client_socket_addr());

    let end_reason = Arc::new(Mutex::new(None));

    let node_stream = NodeStream::connect(
      &info,
      node.client_socket_addr(),
      token,
      client.clone(),
      w3gs_tx.clone(),
      end_reason.clone(),
    )
    .await?;

    tracing::debug!("listening on port {}", port);

    let state = Arc::new(State {
      info,
      stream: node_stream.sender(),
      game_status_rx: status_rx,
    });

    tokio::spawn({
      let state = state.clone();
      let scope = scope.handle();
      let node = node.clone();
      let client = client.clone();
      async move {
        let res = state
          .serve(
            listener,
            event_rx,
            w3gs_tx,
            w3gs_rx,
            end_reason,
            scope,
            node,
            client.clone(),
            game_version_string,
            save_replay,
          )
          .await;

        if let Err(res) = res {
          tracing::error!("lan: {}", res);
        }

        client
          .notify(LanEvent::LanGameDisconnected { game_id })
          .await
          .ok();
        tracing::debug!("exiting");
      }
      .instrument(tracing::debug_span!("worker"))
    });

    Ok(LanProxy {
      _scope: scope,
      node_stream,
      port,
      status_tx,
      event_tx,
    })
  }

  pub async fn dispatch_game_status_change(&self, status: NodeGameStatus) {
    self.status_tx.send(Some(status)).ok();
  }

  pub async fn dispatch_player_event(&mut self, evt: PlayerEvent) {
    self.event_tx.send(evt).await.ok();
  }

  pub fn port(&self) -> u16 {
    self.port
  }

  pub async fn shutdown(self) {
    self.node_stream.shutdown().await;
  }
}

#[derive(Debug)]
struct State {
  info: LanGameInfo,
  stream: NodeStreamSender,
  game_status_rx: watch::Receiver<Option<NodeGameStatus>>,
}

impl State {
  async fn serve(
    self: Arc<Self>,
    mut listener: W3GSListener,
    event_rx: Receiver<PlayerEvent>,
    mut w3gs_tx: Sender<Packet>,
    mut w3gs_rx: Receiver<Packet>,
    end_reason: Arc<Mutex<Option<GameEndReason>>>,
    mut scope: SpawnScopeHandle,
    node: Arc<NodeInfo>,
    mut client: Addr<ControllerClient>,
    game_version_string: String,
    save_replay: bool,
  ) -> Result<()> {
    let mut node_stream = self.stream.clone();
    let mut status_rx = self.game_status_rx.clone();
    let (stop_collect_player_events_tx, stop_rx) = oneshot::channel();

    tokio::pin! {
      let dropped = scope.left();
      let collect_player_events = self.collect_player_events(event_rx, stop_rx, &self.info);
    }

    // Lobby
    let mut stream = loop {
      let mut incoming = listener.incoming();

      let next = tokio::select! {
        _ = &mut dropped => {
          return Ok(())
        }
        _ = &mut collect_player_events => {
          return Ok(())
        }
        next = incoming.try_next() => {
          next
        }
      };

      tracing::debug!("connected");

      let mut stream: W3GSStream = match next {
        Ok(Some(stream)) => stream,
        Ok(None) => return Ok(()),
        Err(err) => {
          tracing::error!("lan stream: {}", err);
          continue;
        }
      };

      let weak_outgoing_tx = client.send(GetWeakOutgoingMessageSender).await?;
      let lobby_action = {
        let lobby = self.handle_lobby_stream(
          &mut stream,
          &mut node_stream,
          &mut status_rx,
          weak_outgoing_tx,
        );
        tokio::pin!(lobby);

        tokio::select! {
          _ = &mut dropped => {
            return Ok(())
          }
          _ = &mut collect_player_events => {
            return Ok(())
          }
          res = &mut lobby => {
            res?
          }
        }
      };
      match lobby_action {
        LobbyAction::Start => break stream,
        LobbyAction::Leave => continue,
      }
    };

    stop_collect_player_events_tx
      .send(())
      .expect("rx hold on stack");
    let (slot_status_map, mut event_rx) = match (&mut collect_player_events).await {
      Some(rx) => rx,
      None => return Ok(()),
    };

    let mut deferred_in_packets = vec![];
    let mut deferred_out_packets = vec![];

    // Load Screen
    {
      let load_screen = self.handle_load_screen(
        &self.info,
        &mut stream,
        &mut node_stream,
        &mut event_rx,
        &mut status_rx,
        &mut w3gs_rx,
        slot_status_map,
        &mut deferred_in_packets,
        &mut deferred_out_packets,
      );
      tokio::pin!(load_screen);

      tokio::select! {
        _ = &mut dropped => {
          return Ok(())
        }
        res = &mut load_screen => {
          res?
        }
      }

      tracing::debug!("all player loaded");
    };

    // Game Loop
    let mut game_handler = GameHandler::new(
      &self.info,
      &node,
      &mut stream,
      &mut node_stream,
      &mut status_rx,
      &mut w3gs_tx,
      &mut w3gs_rx,
      &mut client,
      &end_reason,
      game_version_string,
      save_replay,
    );
    tokio::select! {
      _ = &mut dropped => {}
      res = game_handler.run(deferred_in_packets, deferred_out_packets) => {
        match res {
          Ok(res) => {
            tracing::info!("game ended: {:?}", res);
          },
          Err(err) => {
            tracing::error!("game ended with error: {}", err);
          }
        }
      }
    };
    {
      let mut guard = end_reason.lock();
      if guard.is_none() {
        guard.replace(GameEndReason::Unknown);
      }
    }
    stream.flush().await.ok();
    Ok(())
  }

  async fn collect_player_events(
    &self,
    mut rx: Receiver<PlayerEvent>,
    mut stop: oneshot::Receiver<()>,
    initial: &LanGameInfo,
  ) -> Option<(HashMap<i32, SlotClientStatus>, Receiver<PlayerEvent>)> {
    let mut map: HashMap<i32, SlotClientStatus> = initial
      .game
      .slots
      .iter()
      .filter_map(|slot| {
        slot
          .player
          .as_ref()
          .map(|p| p.id)
          .map(|player_id| (player_id, slot.client_status))
      })
      .collect();
    loop {
      tokio::select! {
        next = rx.recv() => {
          match next {
            Some(evt) => {
              match evt {
                PlayerEvent::PlayerStatusChange { player_id, status } => {
                  map.insert(player_id, status);
                },
              }
            },
            None => return None,
          }
        }
        _ = &mut stop => {
          return Some((map, rx))
        }
      }
    }
  }

  async fn handle_lobby_stream(
    &self,
    stream: &mut W3GSStream,
    node_stream: &mut NodeStreamSender,
    status_rx: &mut watch::Receiver<Option<NodeGameStatus>>,
    weak_outgoing_tx: Option<WeakSender<OutgoingMessage>>,
  ) -> Result<LobbyAction> {
    let mut lobby_handler = LobbyHandler::new(
      &self.info,
      stream,
      Some(node_stream),
      status_rx,
      weak_outgoing_tx,
    );
    let action = lobby_handler.run().await?;
    Ok(action)
  }

  async fn handle_load_screen(
    &self,
    info: &LanGameInfo,
    stream: &mut W3GSStream,
    node_stream: &mut NodeStreamSender,
    event_rx: &mut Receiver<PlayerEvent>,
    status_rx: &mut watch::Receiver<Option<NodeGameStatus>>,
    w3gs_rx: &mut Receiver<Packet>,
    initial_status_map: HashMap<i32, SlotClientStatus>,
    deferred_in_packets: &mut Vec<Packet>,
    deferred_out_packets: &mut Vec<Packet>,
  ) -> Result<()> {
    let my_player_id = info.game.player_id;
    let my_slot_player_id = info.slot_info.my_slot_player_id;
    let mut loaded_sent = vec![];

    node_stream
      .report_slot_status(SlotClientStatus::Loading)
      .await?;

    // check pre game packets
    {
      let mut packets = vec![];
      for (player_id, status) in initial_status_map {
        match status {
          SlotClientStatus::Pending => {}
          SlotClientStatus::Connected => {}
          SlotClientStatus::Joined => {}
          SlotClientStatus::Loading => {}
          SlotClientStatus::Loaded => {
            if player_id != my_player_id {
              loaded_sent.push(player_id);
              tracing::debug!("player loaded (pre-game): {}", player_id);
              packets.push(get_player_loaded_packet(info, player_id)?);
            } else {
              tracing::warn!("received Loaded status for local player");
            }
          }
          SlotClientStatus::Disconnected => {}
          SlotClientStatus::Left => {}
        }
      }
      if !packets.is_empty() {
        stream.send_all(packets).await?;
      }
    }

    let mut ping_interval = interval(LOAD_SCREEN_PING_INTERVAL);
    let base_t = Instant::now();

    loop {
      tokio::select! {
        _ = ping_interval.tick() => {
          stream.send(Packet::simple(PingFromHost::with_payload_since(base_t))?).await?;
        }
        // war3 packets
        res = stream.recv() => {
          match res? {
            Some(pkt) => {
              tracing::debug!("load screen => {:?}", pkt.type_id());
              match pkt.type_id() {
                GameLoadedSelf::PACKET_TYPE_ID => {
                  tracing::debug!("self loaded: {}", my_slot_player_id);

                  if let Some(idx) = self.info.slot_info.stream_ob_slot.clone() {
                    stream.send(Packet::simple(PlayerLoaded {
                      player_id: index_to_player_id(idx)
                    })?).await?;
                  }

                  stream.send(Packet::simple(PlayerLoaded {
                    player_id: my_slot_player_id
                  })?).await?;

                  node_stream.report_slot_status(SlotClientStatus::Loaded).await?;
                },
                LeaveReq::PACKET_TYPE_ID => {
                  tracing::debug!("leave: {:?}", my_slot_player_id);
                  node_stream.report_slot_status(SlotClientStatus::Connected).await.ok();
                  stream.send(Packet::simple(LeaveAck)?).await?;
                  stream.flush().await?;
                  break;
                }
                PongToHost::PACKET_TYPE_ID => {}
                _ => {
                  deferred_out_packets.push(pkt);
                }
              }
            },
            None => {
              return Err(Error::StreamClosed)
            },
          }
        }
        // player events
        next = event_rx.recv() => {
          match next {
            Some(event) => handle_player_event(info, my_player_id, &mut loaded_sent, stream, event).await?,
            None => {
              break;
            },
          }
        }
        // node status ack
        changed = status_rx.changed() => {
          let next =
            if changed.is_ok() {
              status_rx.borrow().clone()
            } else {
              return Err(Error::TaskCancelled(anyhow::format_err!("game status tx dropped")))
            };
          match next {
            Some(status) => {
              match status {
                NodeGameStatus::Loading => {},
                NodeGameStatus::Running => {
                  event_rx.close();

                  while let Some(event) = event_rx.recv().await {
                    handle_player_event(info, my_player_id, &mut loaded_sent, stream, event).await?;
                  }

                  return Ok(())
                },
                other => {
                  return Err(Error::UnexpectedNodeGameStatus(other))
                }
              }
            },
            None => {},
          }
        }
        next = w3gs_rx.recv() => {
          if let Some(pkt) = next {
            if pkt.type_id() == PacketTypeId::PingFromHost {
              stream.send(pkt).await?;
            } else {
              deferred_in_packets.push(pkt);
            }
          } else {
            return Err(Error::TaskCancelled(anyhow::format_err!("w3g tx dropped")))
          }
        }
      }
    }

    Ok(())
  }
}

async fn handle_player_event(
  info: &LanGameInfo,
  my_player_id: i32,
  loaded_sent: &mut Vec<i32>,
  stream: &mut W3GSStream,
  event: PlayerEvent,
) -> Result<()> {
  match event {
    PlayerEvent::PlayerStatusChange { player_id, status } => match status {
      SlotClientStatus::Pending | SlotClientStatus::Connected | SlotClientStatus::Joined => {
        tracing::warn!(
          player_id,
          "unexpected player status update during load screen: {:?}",
          status
        );
      }
      SlotClientStatus::Loading => {}
      SlotClientStatus::Loaded => {
        if player_id != my_player_id && !loaded_sent.contains(&player_id) {
          tracing::debug!("player loaded: {}", player_id);
          stream
            .send(get_player_loaded_packet(info, player_id)?)
            .await?;
          loaded_sent.push(player_id);
        }
      }
      SlotClientStatus::Disconnected => {}
      SlotClientStatus::Left => {}
    },
  }
  Ok(())
}

fn get_player_loaded_packet(info: &LanGameInfo, player_id: i32) -> Result<Packet> {
  let slot = info
    .slot_info
    .player_infos
    .iter()
    .find(|s| s.player_id == player_id);
  if let Some(slot_info) = slot {
    tracing::debug!(
      player_id,
      "player at slot {} loaded: {}",
      slot_info.slot_player_id,
      slot_info.name
    );
    return Ok(Packet::simple(PlayerLoaded {
      player_id: slot_info.slot_player_id,
    })?);
  } else {
    tracing::error!("player slot was not found");
    return Err(Error::SlotNotResolved);
  }
}

#[derive(Debug)]
pub enum PlayerEvent {
  PlayerStatusChange {
    player_id: i32,
    status: SlotClientStatus,
  },
}
