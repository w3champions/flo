use parking_lot::RwLock;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack, S2ProtoUnpack};
use std::collections::HashMap;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Notify;
use tokio::time::delay_for;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::*;
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;
use flo_task::{SpawnScope, SpawnScopeHandle};

use crate::error::*;
use crate::game::{Game, SlotClientStatus, SlotStatus};
use crate::node::PlayerToken;
use crate::state::event::{FloControllerEvent, GameStatusUpdate};

pub type HandlerSender = Sender<Frame>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct NodeConn {
  scope: SpawnScope,
  state: Arc<NodeConnState>,
}

impl Drop for NodeConn {
  fn drop(&mut self) {
    self.state.event_sender.close();
  }
}

impl NodeConn {
  pub fn new(
    id: i32,
    addr: &str,
    secret: &str,
    event_sender: EventSender<NodeConnEvent>,
    ctrl_event_sender: Sender<FloControllerEvent>,
    delay: Option<Duration>,
  ) -> Result<Self> {
    let scope = SpawnScope::new();
    let (sender, receiver) = channel(1);

    let (ip, port) = parse_addr(addr)?;

    let state = Arc::new(NodeConnState {
      id,
      event_sender: event_sender.clone(),
      sender,
      ctrl_event_sender,
      secret: secret.to_string(),
      status: RwLock::new(NodeConnStatus::Connecting),
      pending_requests: RwLock::new(HashMap::new()),
    });

    tokio::spawn(
      {
        let state = state.clone();
        let mut event_sender = event_sender.clone();
        let mut scope = scope.handle();
        async move {
          if let Some(delay) = delay {
            tokio::select! {
              _ = scope.left() => {
                return;
              }
              _ = delay_for(delay) => {}
            }
          }

          if let Err(err) = Self::worker(state.clone(), ip, port, receiver, scope.clone()).await {
            event_sender
              .send_or_log_as_error(NodeConnEvent::new(id, NodeConnEventData::WorkerError(err)))
              .await;
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("node conn worker", addr)),
    );

    Ok(Self { scope, state })
  }

  pub async fn create_game(&self, game: &Game) -> Result<CreatedGameInfo> {
    let game_id = game.id;

    let req_id = RequestId::CreateGame(game_id);

    if *self.state.status.read() != NodeConnStatus::Connected {
      return Err(Error::NodeNotReady);
    }

    let mut slots = Vec::with_capacity(game.slots.len());
    for (i, slot) in game.slots.iter().enumerate() {
      if slot.settings.status == SlotStatus::Occupied {
        slots.push(flo_net::proto::flo_node::GameSlot {
          id: i as u32,
          player: slot.player.as_ref().map(|player| GamePlayer {
            player_id: player.id,
            name: player.name.clone(),
          }),
          settings: Some(slot.settings.clone().pack()?),
          client_status: Default::default(),
        });
      }
    }

    let pkt = PacketControllerCreateGame {
      game: Some(flo_net::proto::flo_node::Game {
        id: game_id,
        settings: Some(flo_net::proto::flo_node::GameSettings {
          map_path: game.map.path.clone(),
          map_sha1: game.map.sha1.to_vec(),
          map_checksum: game.map.checksum,
        }),
        slots,
        status: Default::default(),
      }),
    };

    let res = self
      .state
      .clone()
      .request(req_id, {
        let mut sender = self.state.sender.clone();
        async move {
          sender
            .send(pkt.encode_as_frame()?)
            .await
            .map_err(|_| Error::NodeRequestCancelled)
        }
      })
      .await?;

    if let Response::GameCreated(game_info) = res {
      Ok(game_info)
    } else {
      tracing::error!(game_id, "unexpected node response: {:?}", res);
      Err(Error::NodeResponseUnexpected)
    }
  }

  pub async fn player_leave(&self, game_id: i32, player_id: i32) -> Result<PlayerLeaveResponse> {
    let req = RequestId::PlayerLeave(PlayerLeaveRequestId { game_id, player_id });
    let res = self
      .state
      .clone()
      .request(req, {
        let mut sender = self.state.sender.clone();
        async move {
          let mut pkt = PacketControllerUpdateSlotStatus {
            player_id,
            game_id,
            ..Default::default()
          };

          pkt.set_status(flo_net::proto::flo_common::SlotClientStatus::Left);

          sender
            .send(pkt.encode_as_frame()?)
            .await
            .map_err(|_| Error::NodeRequestCancelled)
        }
      })
      .await?;

    if let Response::PlayerLeave(res) = res {
      Ok(res)
    } else {
      tracing::error!(game_id, player_id, "unexpected node response: {:?}", res);
      Err(Error::NodeResponseUnexpected)
    }
  }

  async fn worker(
    state: Arc<NodeConnState>,
    ip: Ipv4Addr,
    port: u16,
    mut receiver: Receiver<Frame>,
    mut scope: SpawnScopeHandle,
  ) -> Result<()> {
    let addr = SocketAddrV4::new(ip, port);
    let mut stream = FloStream::connect(addr).await?;

    stream
      .send(PacketControllerConnect {
        lobby_version: Some(crate::version::FLO_LOBBY_VERSION.into()),
        secret: state.secret.clone(),
      })
      .await?;

    let res = stream.recv_frame().await?;

    flo_net::try_flo_packet! {
      res => {
        packet: PacketControllerConnectAccept => {
          tracing::debug!("node connected: version = {:?}", packet.version);
        }
        packet: PacketControllerConnectReject => {
          return Err(Error::NodeConnectionRejected {
            addr,
            reason: packet.reason(),
          })
        }
      }
    };

    *state.status.write() = NodeConnStatus::Connected;
    state
      .event_sender
      .clone()
      .send_or_discard(NodeConnEvent::new(state.id, NodeConnEventData::Connected))
      .await;

    loop {
      tokio::select! {
        _ = scope.left() => {
          break;
        }
        frame = receiver.recv() => {
          let frame = if let Some(frame) = frame {
            frame
          } else {
            break;
          };
          if let Err(err) = stream.send_frame(frame).await {
            tracing::error!("send: {}", err);
            break;
          }
        }
        res = stream.recv_frame() => {
          match res {
            Ok(frame) => {
              if let Err(err) = state.handle_frame(frame).await {
                tracing::error!("handle frame: {}", err);
              }
            },
            Err(err) => {
              tracing::error!("recv: {}", err);
              break;
            },
          }
        }
      }
    }

    state
      .event_sender
      .clone()
      .send_or_discard(NodeConnEvent::new(
        state.id,
        NodeConnEventData::Disconnected,
      ))
      .await;
    tracing::debug!("exiting");

    Ok(())
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::PacketControllerCreateGameAccept))]
pub struct CreatedGameInfo {
  pub game_id: i32,
  pub player_tokens: Vec<PlayerToken>,
}

impl S2ProtoUnpack<flo_net::proto::flo_node::PlayerToken> for PlayerToken {
  fn unpack(
    value: flo_net::proto::flo_node::PlayerToken,
  ) -> Result<Self, s2_grpc_utils::result::Error> {
    let mut bytes = [0_u8; 16];
    if value.token.len() >= 16 {
      bytes.clone_from_slice(&value.token[0..16]);
    } else {
      (&mut bytes[0..(value.token.len())]).clone_from_slice(&value.token[0..(value.token.len())]);
    }
    Ok(PlayerToken {
      player_id: value.player_id,
      bytes,
    })
  }
}

#[derive(Debug)]
struct PendingRequest {
  sender: Option<oneshot::Sender<Result<Response>>>,
  done: Arc<Notify>,
  scope: SpawnScope,
}

impl Drop for PendingRequest {
  fn drop(&mut self) {
    if let Some(sender) = self.sender.take() {
      sender.send(Err(Error::NodeRequestCancelled)).ok();
    }
  }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum RequestId {
  CreateGame(i32),
  PlayerLeave(PlayerLeaveRequestId),
}

#[derive(Debug)]
enum Response {
  GameCreated(CreatedGameInfo),
  PlayerLeave(PlayerLeaveResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PlayerLeaveRequestId {
  game_id: i32,
  player_id: i32,
}

#[derive(Debug)]
pub enum PlayerLeaveResponse {
  Accepted(SlotClientStatus),
  Rejected(UpdateSlotClientStatusRejectReason),
}

#[derive(Debug)]
struct NodeConnState {
  id: i32,
  event_sender: EventSender<NodeConnEvent>,
  ctrl_event_sender: Sender<FloControllerEvent>,
  sender: Sender<Frame>,
  secret: String,
  status: RwLock<NodeConnStatus>,
  pending_requests: RwLock<HashMap<RequestId, PendingRequest>>,
}

impl NodeConnState {
  pub async fn request<Fut>(self: Arc<Self>, id: RequestId, action: Fut) -> Result<Response>
  where
    Fut: Future<Output = Result<()>> + Send + 'static,
  {
    let done = Arc::new(Notify::new());
    let (mut scope_handle, receiver) = {
      let mut guard = self.pending_requests.write();
      if guard.contains_key(&id) {
        return Err(Error::NodeRequestProcessing);
      }

      let scope = SpawnScope::new();
      let scope_handle = scope.handle();
      let (sender, receiver) = oneshot::channel();
      let pending = PendingRequest {
        sender: Some(sender),
        done: done.clone(),
        scope,
      };
      guard.insert(id, pending);
      (scope_handle, receiver)
    };
    tokio::spawn(
      {
        let state = self.clone();
        async move {
          let timeout = delay_for(REQUEST_TIMEOUT);

          tokio::pin!(timeout);

          let exit = tokio::select! {
            _ = scope_handle.left() => {
              state.request_callback(id, Err(Error::NodeRequestCancelled));
              true
            }
            _ = &mut timeout => {
              state.request_callback(id, Err(Error::NodeRequestTimeout));
              tracing::debug!("action timeout: {:?}", id);
              true
            }
            res = action => {
              match res {
                Ok(_) => {
                  false
                }
                Err(err) => {
                  state.request_callback(id, Err(err));
                  tracing::debug!("action error: {:?}", id);
                  true
                }
              }
            }
          };

          if !exit {
            tokio::select! {
              _ = scope_handle.left() => {
                state.request_callback(id, Err(Error::NodeRequestCancelled));
              }
              _ = done.notified() => {}
              _ = &mut timeout => {
                state.request_callback(id, Err(Error::NodeRequestTimeout));
                tracing::debug!("response timeout: {:?}", id);
              }
            }
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
    let response = receiver.await.map_err(|_| {
      tracing::error!("should not happen");
      Error::NodeRequestCancelled
    })?;
    response
  }

  pub fn request_callback(&self, id: RequestId, result: Result<Response>) {
    let pending = self.pending_requests.write().remove(&id);
    if let Some(mut pending) = pending {
      if let Some(sender) = pending.sender.take() {
        pending.done.notify();
        sender.send(result).ok();
      }
    }
  }

  async fn handle_frame(&self, frame: Frame) -> Result<()> {
    let mut ctrl_event_sender = self.ctrl_event_sender.clone();
    flo_net::try_flo_packet! {
      frame => {
        packet: PacketControllerCreateGameAccept => {
          let game_id = packet.game_id;
          self.request_callback(
            RequestId::CreateGame(game_id),
            CreatedGameInfo::unpack(packet).map_err(Into::into).map(Response::GameCreated),
          )
        }
        packet: PacketControllerCreateGameReject => {
          let game_id = packet.game_id;
          self.request_callback(
            RequestId::CreateGame(game_id),
            Err(Error::GameCreateReject(packet.reason()))
          )
        }
        packet: PacketControllerUpdateSlotStatusAccept => {
          let id = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          self.request_callback(
            id,
            Ok(Response::PlayerLeave(PlayerLeaveResponse::Accepted(S2ProtoEnum::unpack_enum(packet.status()))))
          )
        }
        packet: PacketControllerUpdateSlotStatusReject => {
          let id = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          self.request_callback(
            id,
            Ok(Response::PlayerLeave(PlayerLeaveResponse::Rejected(packet.reason())))
          )
        }
        packet: PacketClientUpdateSlotClientStatus => {
          if !self.event_sender.is_closed() {
            ctrl_event_sender.send(
              FloControllerEvent::GameSlotClientStatusUpdate(
                S2ProtoUnpack::unpack(packet)?
              )
            ).await.ok();
          }
        }
        packet: PacketNodeGameStatusUpdate => {
          if !self.event_sender.is_closed() {
            ctrl_event_sender.send(
              FloControllerEvent::GameStatusUpdate(vec![
                GameStatusUpdate::from(packet)
              ])
            ).await.ok();
          }
        }
        packet: PacketNodeGameStatusUpdateBulk => {
          if !self.event_sender.is_closed() {
            ctrl_event_sender.send(
              FloControllerEvent::GameStatusUpdate(packet.games.into_iter().map(Into::into).collect())
            ).await.ok();
          }
        }
      }
    }
    Ok(())
  }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum NodeConnStatus {
  Connecting,
  Connected,
}

fn parse_addr(addr: &str) -> Result<(Ipv4Addr, u16)> {
  let (ip, port) = if addr.contains(":") {
    let addr = if let Some(addr) = addr.parse::<SocketAddrV4>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeAddress(addr.to_string()));
    };

    (addr.ip().clone(), addr.port())
  } else {
    let addr: Ipv4Addr = if let Some(addr) = addr.parse::<Ipv4Addr>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeAddress(addr.to_string()));
    };
    let port = flo_constants::NODE_CONTROLLER_PORT;
    (addr, port)
  };
  Ok((ip, port))
}

#[derive(Debug)]
pub struct NodeConnEvent {
  pub node_id: i32,
  pub data: NodeConnEventData,
}

impl NodeConnEvent {
  fn new(node_id: i32, data: NodeConnEventData) -> Self {
    Self { node_id, data }
  }
}

#[derive(Debug)]
pub enum NodeConnEventData {
  Connected,
  Disconnected,
  WorkerError(Error),
}

impl FloEvent for NodeConnEvent {
  const NAME: &'static str = "NodeConnEvent";
}
