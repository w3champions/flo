use crate::controller::ControllerClient;
use crate::error::*;
use crate::lan::game::LanGameInfo;
use crate::lan::LanEvent;
use crate::types::GameStatusUpdate;
use crate::types::SlotClientStatus;
use backoff::backoff::Backoff;
use backoff::{self, ExponentialBackoff};
use flo_net::packet::*;
use flo_net::proto::flo_node as proto;
use flo_net::stream::FloStream;
use flo_net::w3gs::{W3GSAckQueue, W3GSFrameExt, W3GSMetadata, W3GSPacket, W3GSPacketTypeId};
use flo_state::Addr;
use flo_types::node::NodeGameStatusSnapshot;
use flo_w3gs::action::IncomingAction;
use flo_w3gs::protocol::chat::ChatFromHost;
use futures::FutureExt;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tokio::time::{sleep, Sleep};
use tokio_util::sync::CancellationToken;
use tracing_futures::Instrument;

pub struct NodeStream {
  tx: NodeStreamSender,
  ct: CancellationToken,
  shutdown_notify: Arc<Notify>,
}

impl NodeStream {
  pub async fn shutdown(self) {
    self.ct.cancel();
    self.shutdown_notify.notified().await;
  }
}

impl Drop for NodeStream {
  fn drop(&mut self) {
    self.ct.cancel();
  }
}

impl NodeStream {
  pub async fn connect(
    game: &LanGameInfo,
    addr: SocketAddr,
    token: NodeConnectToken,
    client: Addr<ControllerClient>,
    game_tx: Sender<W3GSPacket>,
    left_game: Arc<AtomicBool>,
  ) -> Result<Self> {
    let ct = CancellationToken::new();
    let shutdown_notify = Arc::new(Notify::new());
    let (tx, rx) = channel(10);

    let session = Session {
      game_id: game.game.game_id,
      player_id: game.game.player_id,
      slot_player_id: game.slot_info.slot_player_id,
      addr,
      token,
      client,
      game_tx,
      rx,
      ct: ct.clone(),
      ack_q: W3GSAckQueue::new(),
      tick: 0,
      ack: 0,
      time: 0,
      last_connected_at: None,
      left_game,
    };

    tokio::spawn(
      session
        .run()
        .map({
          let shutdown_notify = shutdown_notify.clone();
          move |_| {
            shutdown_notify.notify_one();
          }
        })
        .instrument(tracing::debug_span!("worker", game_id = game.game.game_id)),
    );

    Ok(Self {
      tx: NodeStreamSender { tx },
      ct,
      shutdown_notify,
    })
  }

  pub fn sender(&self) -> NodeStreamSender {
    self.tx.clone()
  }
}

struct Session {
  game_id: i32,
  #[allow(unused)]
  player_id: i32,
  slot_player_id: u8,
  addr: SocketAddr,
  token: NodeConnectToken,
  client: Addr<ControllerClient>,
  game_tx: Sender<W3GSPacket>,
  rx: Receiver<WorkerMsg>,
  ct: CancellationToken,
  ack_q: W3GSAckQueue,
  tick: u32,
  time: u32,
  ack: u32,
  last_connected_at: Option<Instant>,
  left_game: Arc<AtomicBool>,
}

impl Session {
  async fn run(mut self) {
    let mut reconnect_backoff = ExponentialBackoff {
      initial_interval: Duration::from_secs(1),
      max_interval: Duration::from_secs(5),
      max_elapsed_time: Some(Duration::from_secs(60)),
      ..Default::default()
    };
    let ct = self.ct.clone();

    let stream = 'main: loop {
      let (mut stream, conn): (FloStream, Connection) = {
        if self
          .last_connected_at
          .map(|v| Instant::now() - v > Connection::MIN_DURATION)
          .unwrap_or(false)
        {
          reconnect_backoff.reset();
        }

        loop {
          if self.last_connected_at.is_some() {
            self
              .send_private_message("Reconnecting to the server...")
              .await
              .ok();
          }

          tokio::select! {
            _ = ct.cancelled() => {
              tracing::info!("session cancelled");
              break 'main None;
            },
            res = self.connect() => {
              match res {
                Ok(pair) => {
                  break pair;
                }
                Err(err) => {
                  tracing::error!("connect node: {}", err);
                  use flo_net::proto::flo_node::ClientConnectRejectReason;
                  match err {
                    Error::NodeConnectionRejected(reason, _) if reason != ClientConnectRejectReason::Multi => {
                      break 'main None;
                    },
                    _ => {
                      if let Some(delay) = reconnect_backoff.next_backoff() {
                        tracing::error!("connect node error: {:?}", err);
                        sleep(delay).await;
                      } else {
                        tracing::error!("connect node: timeout");
                        break 'main None;
                      }
                    }
                  }
                }
              }
            }
          }
        }
      };

      self.last_connected_at.replace(Instant::now());
      tracing::debug!("node connected.");

      let res = conn.run(&mut stream, &mut self).await;
      match res {
        Ok(res) => match res {
          ConnectionRunResult::Cancelled | ConnectionRunResult::GameDisconnected => {
            break 'main Some(stream);
          }
          ConnectionRunResult::NodeDisconnected => {
            if self.left_game.load(Ordering::SeqCst) {
              tracing::info!("node session ended");
              break 'main Some(stream);
            } else {
              tracing::error!("node disconnected unexpectedly");
            }
            if let Some(delay) = reconnect_backoff.next_backoff() {
              sleep(delay).await;
            }
          }
        },
        Err(err) => {
          tracing::error!("unexpected node conn error: {}", err);
          break 'main Some(stream);
        }
      }
    };

    if let Some(mut stream) = stream {
      let mut flush_frames = vec![];
      self.rx.close();
      while let Some(msg) = self.rx.recv().await {
        match self.encode_worker_msg(msg) {
          Ok(frame) => flush_frames.push(frame),
          Err(err) => {
            tracing::error!("encode worker msg: {}", err);
            break;
          }
        }
      }

      match self.encode_worker_msg(WorkerMsg::StatusUpdate(SlotClientStatus::Left)) {
        Ok(frame) => {
          flush_frames.push(frame);
        }
        Err(err) => {
          tracing::error!("encode left status update packet: {}", err);
        }
      }

      if !flush_frames.is_empty() {
        tracing::debug!("flushing frames: {}", flush_frames.len());
        if let Err(err) = stream.send_frames(flush_frames).await {
          tracing::error!("flush frames: {}", err);
        }
      }

      tracing::debug!("flushing stream");
      if let Err(err) = stream.flush().await {
        tracing::error!("flush stream: {}", err);
      }
    }

    self.notify_disconnected().await;
  }

  async fn connect(&self) -> Result<(FloStream, Connection)> {
    let mut stream = FloStream::connect_no_delay(self.addr).await?;

    stream
      .send(proto::PacketClientConnect {
        version: Some(crate::version::FLO_VERSION.into()),
        token: self.token.to_vec(),
      })
      .await?;

    let frame = stream.recv_frame().await?;

    let (player_id, status_snapshot): (i32, NodeGameStatusSnapshot) = flo_net::try_flo_packet! {
      frame => {
        p: proto::PacketClientConnectAccept => {
          let game_id = p.game_id;
          let player_id = p.player_id;
          tracing::debug!(
            game_id,
            player_id,
            "node connected: version = {:?}, game_status = {:?}",
            p.version,
            p.game_status,
          );
          let status = NodeGameStatusSnapshot::unpack(p)?;
          (player_id, status)
        }
        p: proto::PacketClientConnectReject => {
          return Err(Error::NodeConnectionRejected(p.reason(), p.message))
        }
      }
    };

    if !self.ack_q.pending_ack_queue().is_empty() {
      let frames = self
        .ack_q
        .pending_ack_queue()
        .iter()
        .cloned()
        .map(|(meta, packet)| Frame::from_w3gs(meta, packet));
      stream.send_frames(frames).await?;
    }

    let game_id = status_snapshot.game_id;

    if self
      .client
      .notify(LanEvent::NodeStreamEvent {
        game_id: self.game_id,
        inner: NodeStreamEvent::GameStatusSnapshot(status_snapshot),
      })
      .await
      .is_err()
    {
      return Err(Error::TaskCancelled(anyhow::format_err!(
        "controller connection gone"
      )));
    }

    Ok((
      stream,
      Connection {
        game_id,
        _player_id: player_id,
      },
    ))
  }

  fn encode_worker_msg(&mut self, msg: WorkerMsg) -> Result<Frame> {
    let frame = match msg {
      WorkerMsg::StatusUpdate(status) => {
        let mut pkt =
          flo_net::proto::flo_node::PacketClientUpdateSlotClientStatusRequest::default();
        pkt.set_status(status.into_proto_enum());
        pkt.encode_as_frame()?
      }
      WorkerMsg::W3GS(pkt) => {
        // if pkt.type_id() == W3GSPacketTypeId::ChatToHost {
        //   use flo_util::chat::parse_chat_command;
        //   use flo_w3gs::protocol::chat::{ChatToHost};
        //   let pkt: ChatToHost = pkt.decode_simple()?;
        //   if let Some(cmd) = pkt.chat_message().and_then(|v| parse_chat_command(v)) {
        //     match cmd.name() {
        //       "conn" => {
        //         session
        //           .game_tx
        //           .send(W3GSPacket::simple(ChatFromHost::private_to_self(
        //             pkt.from_player,
        //             format!(
        //               "local: last_ack_received = {:?}, len = {}",
        //               session.ack_q.last_ack_received(),
        //               session.ack_q.pending_ack_len()
        //             ),
        //           ))?)
        //           .await
        //           .ok();
        //       }
        //       _ => {}
        //     }
        //   }
        // }

        let sid = self.ack_q.gen_next_send_sid();
        let ack_id = self.ack_q.take_ack_received();
        let meta = W3GSMetadata::new(pkt.type_id(), sid, ack_id);

        match pkt.type_id() {
          W3GSPacketTypeId::OutgoingKeepAlive => {
            self.ack += 1;
          }
          _ => {}
        }

        // tracing::debug!(
        //   "send#{} tick = {}, time = {}, ack = {}",
        //   meta.sid(),
        //   session.tick,
        //   session.time,
        //   session.ack,
        // );

        self.ack_q.push_send(meta.clone(), pkt.clone());
        Frame::from_w3gs(meta, pkt)
      }
    };
    Ok(frame)
  }

  async fn send_private_message<T: AsRef<str>>(&self, msg: T) -> Result<()> {
    if self.tick > 0 {
      self
        .game_tx
        .send(W3GSPacket::simple(ChatFromHost::private_to_self(
          self.slot_player_id,
          msg.as_ref(),
        ))?)
        .await
        .map_err(|_| Error::TaskCancelled(anyhow::format_err!("game not running")))?;
    }
    Ok(())
  }

  async fn notify_disconnected(&self) {
    self
      .client
      .notify(LanEvent::NodeStreamEvent {
        game_id: self.game_id,
        inner: NodeStreamEvent::Disconnected,
      })
      .await
      .ok();
  }
}

struct Connection {
  game_id: i32,
  _player_id: i32,
}

impl Connection {
  const MIN_DURATION: Duration = Duration::from_secs(3);
  const HOST_PING_TIMEOUT: Duration = Duration::from_secs(3);

  fn reset_timeout(t: Pin<&mut Sleep>) {
    t.reset((Instant::now() + Self::HOST_PING_TIMEOUT).into())
  }

  async fn run(
    mut self,
    stream: &mut FloStream,
    session: &mut Session,
  ) -> Result<ConnectionRunResult> {
    let ping_timeout = sleep(Self::HOST_PING_TIMEOUT);
    tokio::pin!(ping_timeout);

    let res = loop {
      tokio::select! {
        _ = &mut ping_timeout => {
          tracing::error!("node stream timeout");
          break ConnectionRunResult::NodeDisconnected
        }

        // cancel
        _ = session.ct.cancelled() => {
          tracing::info!("connection session cancelled");
          break ConnectionRunResult::Cancelled
        }

        // packet from node
        next = stream.recv_frame() => {
          match next {
            Ok(mut frame) => {
              match frame.type_id {
                PacketTypeId::Ping => {
                  Self::reset_timeout(ping_timeout.as_mut());

                  frame.type_id = PacketTypeId::Pong;
                  if let Err(err) = stream.send_frame(frame).await {
                    tracing::error!("send pong to node: {}", err);
                    break ConnectionRunResult::NodeDisconnected;
                  }
                }
                PacketTypeId::W3GS => {
                  let (meta, pkt) = frame.try_into_w3gs()?;

                  match pkt.type_id() {
                    W3GSPacketTypeId::IncomingAction => {
                      let time = IncomingAction::peek_time_increment_ms(pkt.payload.as_ref())?;
                      session.tick += 1;
                      session.time += time as u32;

                      Self::reset_timeout(ping_timeout.as_mut());
                    }
                    W3GSPacketTypeId::LeaveAck => {
                      tracing::info!("node leave ack received");
                    },
                    _ => {}
                  }

                  if !session.ack_q.ack_received(meta.sid()) {
                    tracing::debug!(
                      "discard resend: {}, {:?}, {:?}",
                      meta.sid(),
                      meta.ack_sid(),
                      pkt.type_id()
                    );
                    continue;
                  }
                  if let Some(ack_sid) = meta.ack_sid() {
                    session.ack_q.ack_sent(ack_sid);
                  }
                  if let Err(_) = session.game_tx.send(pkt).await {
                    tracing::debug!("w3gs receiver gone");
                    break ConnectionRunResult::GameDisconnected;
                  }
                }
                _ => {
                  if let Err(err) = self.handle_node_frame(session, frame).await {
                    tracing::error!("handle node frame: {}", err);
                    break ConnectionRunResult::NodeDisconnected;
                  }
                }
              }
            },
            Err(flo_net::error::Error::StreamClosed) => {
              tracing::error!("node stream closed");
              break ConnectionRunResult::NodeDisconnected;
            },
            Err(err) => {
              tracing::error!("node stream recv: {}", err);
              break ConnectionRunResult::NodeDisconnected;
            }
          }
        }

        // worker msgs
        next = session.rx.recv() => {
          match next {
            Some(msg) => {
              let frame = session.encode_worker_msg(msg)?;
              if let Err(err) = stream.send_frame(frame).await {
                tracing::error!("handle_worker_msg: {}", err);
                break ConnectionRunResult::NodeDisconnected;
              }
            },
            None => {
              break ConnectionRunResult::Cancelled;
            }
          }
        }
      }
    };

    Ok(res)
  }

  async fn handle_node_frame(&mut self, session: &mut Session, frame: Frame) -> Result<()> {
    let client = &session.client;
    let game_id = self.game_id;

    flo_net::try_flo_packet! {
      frame => {
        p: proto::PacketClientUpdateSlotClientStatus => {
          tracing::debug!(game_id, player_id = p.player_id, "update slot client status: {:?}", p.status());
          flo_log::result_ok!(
            "send NodeStreamEvent::SlotClientStatusUpdate",
            client.notify(LanEvent::NodeStreamEvent {
              game_id,
              inner: NodeStreamEvent::SlotClientStatusUpdate(S2ProtoUnpack::unpack(p)?)
            }).await
          );
        }
        p: proto::PacketClientUpdateSlotClientStatusReject => {
          tracing::error!(game_id = p.game_id, player_id = p.player_id, "update slot client status rejected: {:?}", p.reason());
          flo_log::result_ok!(
            "send NodeStreamEvent::Disconnected",
            client.notify(LanEvent::NodeStreamEvent {
              game_id,
              inner: NodeStreamEvent::Disconnected
            }).await
          );
        }
        p: flo_net::proto::flo_node::PacketNodeGameStatusUpdate => {
          tracing::debug!(game_id = p.game_id, "update game status: {:?}", p);
          flo_log::result_ok!(
            "send NodeStreamEvent::GameStatusUpdate",
            client.notify(LanEvent::NodeStreamEvent {
              game_id,
              inner: NodeStreamEvent::GameStatusUpdate(p.into())
            }).await
          );
        }
      }
    }
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct NodeStreamSender {
  tx: Sender<WorkerMsg>,
}

impl NodeStreamSender {
  pub async fn report_slot_status(&mut self, status: SlotClientStatus) -> Result<()> {
    if let Err(_err) = self.tx.send(WorkerMsg::StatusUpdate(status)).await {
      tracing::error!("report_slot_status failed");
    }
    Ok(())
  }

  #[inline]
  pub async fn send_w3gs(&mut self, pkt: W3GSPacket) -> Result<()> {
    let type_id = pkt.type_id();
    self.tx.send(WorkerMsg::W3GS(pkt)).await.err().map(|_err| {
      tracing::error!("node stream send cancelled: {:?}", type_id);
    });
    Ok(())
  }
}

enum ConnectionRunResult {
  Cancelled,
  GameDisconnected,
  NodeDisconnected,
}

enum WorkerMsg {
  StatusUpdate(SlotClientStatus),
  W3GS(W3GSPacket),
}

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
pub struct NodeConnectToken([u8; 16]);

impl NodeConnectToken {
  pub fn from_vec(bytes: Vec<u8>) -> Option<Self> {
    if bytes.len() != 16 {
      return None;
    }
    let mut token = NodeConnectToken([0; 16]);
    token.0.copy_from_slice(&bytes[..]);
    Some(token)
  }

  pub fn to_vec(&self) -> Vec<u8> {
    self.0.to_vec()
  }
}

#[derive(Debug)]
pub enum NodeStreamEvent {
  SlotClientStatusUpdate(SlotClientStatusUpdate),
  GameStatusSnapshot(NodeGameStatusSnapshot),
  GameStatusUpdate(GameStatusUpdate),
  Disconnected,
}

#[derive(Debug, S2ProtoUnpack, serde::Serialize, Clone)]
#[s2_grpc(message_type(
  flo_net::proto::flo_connect::PacketGameSlotClientStatusUpdate,
  flo_net::proto::flo_node::PacketClientUpdateSlotClientStatus
))]
pub struct SlotClientStatusUpdate {
  pub player_id: i32,
  pub game_id: i32,
  #[s2_grpc(proto_enum)]
  pub status: SlotClientStatus,
}
