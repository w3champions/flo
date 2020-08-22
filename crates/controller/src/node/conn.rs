use parking_lot::RwLock;
use s2_grpc_utils::S2ProtoUnpack;
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

use crate::error::*;
use crate::game::{Game, SlotStatus};
use crate::node::PlayerToken;

pub type HandlerSender = Sender<Frame>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct NodeConn {
  state: Arc<NodeConnState>,
}

impl Drop for NodeConn {
  fn drop(&mut self) {
    self.state.event_sender.close();
    self.state.dropper.notify();
  }
}

impl NodeConn {
  pub fn new(
    id: i32,
    addr: &str,
    secret: &str,
    event_sender: EventSender<NodeConnEvent>,
    delay: Option<Duration>,
  ) -> Result<Self> {
    let (sender, receiver) = channel(1);

    let (ip, port) = parse_addr(addr)?;

    let state = Arc::new(NodeConnState {
      id,
      event_sender: event_sender.clone(),
      sender,
      secret: secret.to_string(),
      status: RwLock::new(NodeConnStatus::Connecting),
      dropper: Notify::new(),
      pending_requests: RwLock::new(HashMap::new()),
    });

    tokio::spawn(
      {
        let state = state.clone();
        let mut event_sender = event_sender.clone();
        async move {
          if let Some(delay) = delay {
            tokio::select! {
              _ = state.dropper.notified() => {
                return;
              }
              _ = delay_for(delay) => {}
            }
          }

          if let Err(err) = Self::worker(state.clone(), ip, port, receiver).await {
            event_sender
              .send_or_log_as_error(NodeConnEvent::new(
                id,
                NodeConnEventData::WorkerErrorEvent(err),
              ))
              .await;
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("node conn worker", addr)),
    );

    Ok(Self { state })
  }

  pub async fn create_game(&self, game: &Game) -> Result<CreatedGameInfo> {
    let game_id = game.id;

    let req_id = RequestId::CreateGame(game_id);

    if *self.state.status.read() != NodeConnStatus::Connected {
      return Err(Error::NodeNotReady);
    }

    let pkt = PacketControllerCreateGame {
      game: Some(flo_net::proto::flo_node::Game {
        id: game_id,
        settings: Some(flo_net::proto::flo_node::GameSettings {
          map_path: game.map.path.clone(),
          map_sha1: game.map.sha1.to_vec(),
          map_checksum: game.map.checksum,
        }),
        slots: game
          .slots
          .iter()
          .enumerate()
          .filter_map(|(i, slot)| {
            if slot.settings.status == SlotStatus::Occupied {
              Some(flo_net::proto::flo_node::GameSlot {
                id: i as u32,
                player: slot.player.as_ref().map(|player| GamePlayer {
                  player_id: player.id,
                  name: player.name.clone(),
                }),
                settings: slot.settings.clone().into_packet().into(),
                client_status: Default::default(),
              })
            } else {
              None
            }
          })
          .collect(),
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
          let pkt = PacketClientUpdateSlotClientStatusRequest {
            player_id,
            game_id,
            status: SlotClientStatus::Left.into(),
          };

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
        packet = PacketControllerConnectAccept => {
          tracing::debug!("node connected: version = {:?}", packet.version);
        }
        packet = PacketControllerConnectReject => {
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
      .send_or_discard(NodeConnEvent::new(
        state.id,
        NodeConnEventData::ConnectedEvent,
      ))
      .await;

    loop {
      tokio::select! {
        _ = state.dropper.notified() => {
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
        NodeConnEventData::DisconnectedEvent,
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
  dropper: Arc<Notify>,
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
  sender: Sender<Frame>,
  secret: String,
  status: RwLock<NodeConnStatus>,
  dropper: Notify,
  pending_requests: RwLock<HashMap<RequestId, PendingRequest>>,
}

impl NodeConnState {
  pub async fn request<Fut>(self: Arc<Self>, id: RequestId, action: Fut) -> Result<Response>
  where
    Fut: Future<Output = Result<()>> + Send + 'static,
  {
    let (dropper, receiver) = {
      let mut guard = self.pending_requests.write();
      if guard.contains_key(&id) {
        return Err(Error::NodeRequestProcessing);
      }

      let dropper = Arc::new(Notify::new());
      let (sender, receiver) = oneshot::channel();
      let pending = PendingRequest {
        sender: Some(sender),
        dropper: dropper.clone(),
      };
      guard.insert(id, pending);
      (dropper, receiver)
    };
    tokio::spawn(
      {
        let state = self.clone();
        async move {
          let timeout = delay_for(REQUEST_TIMEOUT);

          tokio::pin!(timeout);

          let exit = tokio::select! {
            _ = dropper.notified() => {
              state.request_callback(id, Err(Error::NodeRequestCancelled));
              true
            }
            _ = &mut timeout => {
              state.request_callback(id, Err(Error::NodeRequestTimeout));
              tracing::debug!("action timeout");
              true
            }
            res = action => {
              match res {
                Ok(_) => {
                  false
                }
                Err(err) => {
                  state.request_callback(id, Err(err));
                  tracing::debug!("action error");
                  true
                }
              }
            }
          };

          if !exit {
            tokio::select! {
              _ = dropper.notified() => {
                state.request_callback(id, Err(Error::NodeRequestCancelled));
              },
              _ = &mut timeout => {
                state.request_callback(id, Err(Error::NodeRequestTimeout));
                tracing::debug!("response timeout");
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
        sender.send(result).ok();
      }
    }
  }

  async fn handle_frame(&self, frame: Frame) -> Result<()> {
    flo_net::try_flo_packet! {
      frame => {
        packet = PacketControllerCreateGameAccept => {
          let game_id = packet.game_id;
          self.request_callback(
            RequestId::CreateGame(game_id),
            CreatedGameInfo::unpack(packet).map_err(Into::into).map(Response::GameCreated),
          )
        }
        packet = PacketControllerCreateGameReject => {
          let game_id = packet.game_id;
          self.request_callback(
            RequestId::CreateGame(game_id),
            Err(Error::GameCreateReject(packet.reason()))
          )
        }
        packet = PacketClientUpdateSlotClientStatus => {
          let req = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          self.request_callback(
            req,
            Ok(Response::PlayerLeave(PlayerLeaveResponse::Accepted(packet.status())))
          )
        }
        packet = PacketClientUpdateSlotClientStatusReject => {
          let req = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          self.request_callback(
            req,
            Ok(Response::PlayerLeave(PlayerLeaveResponse::Rejected(packet.reason())))
          )
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
  ConnectedEvent,
  DisconnectedEvent,
  WorkerErrorEvent(Error),
}

impl FloEvent for NodeConnEvent {
  const NAME: &'static str = "NodeConnEvent";
}
