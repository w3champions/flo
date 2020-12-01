use crate::error::*;
use crate::game::state::GameRegistry;
use crate::game::state::{GameSlotClientStatusUpdate, GameStatusUpdate};
use crate::game::{Game, GameStatus};
use crate::node::state::request::{CreatedGameInfo, NodeRequestActor, NodeRequestExt};
use crate::node::{NodeConnConfig, PlayerLeaveResponse};
use crate::state::ActorMapExt;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use flo_net::packet::*;
use flo_net::proto::flo_common::*;
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;
use flo_state::reply::FutureReply;
use flo_state::{async_trait, Actor, Addr, Container, Context, Handler, Message};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};

use crate::game::state::registry::Remove;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::delay_for;
use tracing_futures::Instrument;

pub struct NodeConnActor {
  config: NodeConnConfig,
  reconnect_backoff: ExponentialBackoff,
  status: NodeConnStatus,
  request_actor: Option<Container<NodeRequestActor>>,
  game_reg_addr: Addr<GameRegistry>,
}

impl NodeConnActor {
  pub fn new(config: NodeConnConfig, game_reg_addr: Addr<GameRegistry>) -> Self {
    Self {
      config,
      status: NodeConnStatus::Connecting,
      reconnect_backoff: Self::default_backoff(),
      request_actor: None,
      game_reg_addr,
    }
  }

  fn default_backoff() -> ExponentialBackoff {
    ExponentialBackoff {
      initial_interval: Duration::from_secs(5),
      current_interval: Duration::from_secs(5),
      max_interval: Duration::from_secs(60),
      multiplier: 1.5,
      ..Default::default()
    }
  }
}

#[async_trait]
impl Actor for NodeConnActor {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    self.handle(ctx, Connect).await
  }
}

enum NodeConnectError {
  Retry(Error),
  Fatal(Error),
}

impl From<Error> for NodeConnectError {
  fn from(err: Error) -> Self {
    NodeConnectError::Retry(err)
  }
}

impl From<flo_net::error::Error> for NodeConnectError {
  fn from(err: flo_net::error::Error) -> Self {
    NodeConnectError::Retry(err.into())
  }
}

impl NodeConnActor {
  fn schedule_reconnect(&mut self, ctx: &mut Context<Self>) {
    self.request_actor.take();

    let delay = self
      .reconnect_backoff
      .next_backoff()
      .unwrap_or(self.reconnect_backoff.max_interval);
    tracing::error!(node_id = self.config.id, "reconnect: backoff: {:?}", delay);
    let addr = ctx.addr();
    ctx.spawn(async move {
      delay_for(delay).await;
      addr.send(Connect).await.ok();
    });
  }

  async fn connect(
    node_id: i32,
    ip: Ipv4Addr,
    port: u16,
    secret: &str,
  ) -> Result<FloStream, NodeConnectError> {
    let addr = SocketAddrV4::new(ip, port);
    let mut stream = FloStream::connect(addr).await?;

    stream
      .send(PacketControllerConnect {
        lobby_version: Some(crate::version::FLO_LOBBY_VERSION.into()),
        secret: secret.to_string(),
      })
      .await?;

    let res = stream.recv_frame().await?;

    flo_net::try_flo_packet! {
      res => {
        packet: PacketControllerConnectAccept => {
          tracing::debug!(node_id, "node connected: version = {:?}", packet.version);
        }
        packet: PacketControllerConnectReject => {
          tracing::debug!(node_id, "node connect rejected: reason = {:?}", packet.reason());
          return Err(NodeConnectError::Fatal(Error::NodeConnectionRejected {
            addr,
            reason: packet.reason(),
          }))
        }
      }
    };

    Ok(stream)
  }

  async fn stream_worker(addr: Addr<Self>, mut rx: mpsc::Receiver<Frame>, mut stream: FloStream) {
    const IDLE_TIMEOUT_DURATION: Duration = Duration::from_secs(30);
    const PING_TIMEOUT_DURATION: Duration = Duration::from_secs(10);
    let start_instant = Instant::now();
    let mut keepalive_timer = delay_for(IDLE_TIMEOUT_DURATION);
    enum KeepAliveStatus {
      Idle,
      Pinged,
    }
    let mut keepalive_status = KeepAliveStatus::Idle;
    loop {
      tokio::select! {
        _ = &mut keepalive_timer => {
          match keepalive_status {
            KeepAliveStatus::Idle => {
              let ms = (Instant::now() - start_instant).as_millis() as u32;
              if let Err(err) = stream.send(PacketPing { ms }).await {
                tracing::error!("send: {}", err);
                addr.send(Disconnected).await.ok();
                break;
              }
              keepalive_status = KeepAliveStatus::Pinged;
              keepalive_timer.reset((Instant::now() + PING_TIMEOUT_DURATION).into());
            },
            KeepAliveStatus::Pinged => {
              tracing::error!("ping timeout");
              addr.send(Disconnected).await.ok();
              break;
            },
          }
        }
        Some(frame) = rx.recv() => {
          keepalive_timer.reset((Instant::now() + IDLE_TIMEOUT_DURATION).into());
          keepalive_status = KeepAliveStatus::Idle;

          if let Err(err) = stream.send_frame(frame).await {
            tracing::error!("send: {}", err);
            addr.send(Disconnected).await.ok();
            break;
          }
        }
        res = stream.recv_frame() => {
          match res {
            Ok(frame) => {
              keepalive_timer.reset((Instant::now() + IDLE_TIMEOUT_DURATION).into());
              keepalive_status = KeepAliveStatus::Idle;

              if frame.type_id == PacketTypeId::Pong {
                continue;
              }

              let handle_res = match addr.send(IncomingFrame(frame)).await {
                Ok(res) => res,
                Err(_) => {
                  break;
                }
              };
              if let Err(err) = handle_res {
                tracing::error!("handle frame: {}", err);
                addr.send(Disconnected).await.ok();
                break;
              }
            },
            Err(err) => {
              tracing::error!("recv: {}", err);
              addr.send(Disconnected).await.ok();
              break;
            },
          }
        }
      }
    }
  }
}

struct Connect;

impl Message for Connect {
  type Result = ();
}

#[async_trait]
impl Handler<Connect> for NodeConnActor {
  async fn handle(&mut self, ctx: &mut Context<Self>, _: Connect) {
    if self.status == NodeConnStatus::Connected {
      tracing::warn!(node_id = self.config.id, "already connected");
      return;
    }

    let (ip, port) = match parse_addr(&self.config.addr) {
      Ok(v) => v,
      Err(err) => {
        self.status = NodeConnStatus::Error;
        tracing::error!(node_id = self.config.id, "parse node address: {}", err);
        return;
      }
    };
    let node_id = self.config.id;
    let secret = self.config.secret.clone();
    let stream = match Self::connect(node_id, ip, port, &secret).await {
      Ok(stream) => stream,
      Err(NodeConnectError::Retry(err)) => {
        tracing::error!(node_id, "error: {}", err);
        self.schedule_reconnect(ctx);
        return;
      }
      Err(NodeConnectError::Fatal(err)) => {
        self.status = NodeConnStatus::Error;
        tracing::error!(node_id, "fatal error: {}", err);
        return;
      }
    };
    let (tx, rx) = mpsc::channel(3);
    ctx.spawn(
      Self::stream_worker(ctx.addr(), rx, stream)
        .instrument(tracing::debug_span!("stream_worker", node_id)),
    );
    self.request_actor = NodeRequestActor::new(tx).start().into();
    self.reconnect_backoff.reset();
  }
}

struct Disconnected;

impl Message for Disconnected {
  type Result = ();
}

#[async_trait]
impl Handler<Disconnected> for NodeConnActor {
  async fn handle(&mut self, ctx: &mut Context<Self>, _: Disconnected) {
    self.schedule_reconnect(ctx);
  }
}

struct IncomingFrame(Frame);

impl Message for IncomingFrame {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<IncomingFrame> for NodeConnActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    IncomingFrame(frame): IncomingFrame,
  ) -> Result<()> {
    use super::request::*;

    enum Parsed {
      Response(RequestDone),
      GameSlotClientStatusUpdate(GameSlotClientStatusUpdate),
      GameStatusUpdate(Vec<GameStatusUpdate>),
    }

    let parsed = flo_net::try_flo_packet! {
      frame => {
        packet: PacketControllerCreateGameAccept => {
          let game_id = packet.game_id;
          Parsed::Response(
            RequestDone::new(
              RequestId::CreateGame(game_id),
              CreatedGameInfo::unpack(packet).map_err(Into::into).map(Response::GameCreated),
            )
          )
        }
        packet: PacketControllerCreateGameReject => {
          let game_id = packet.game_id;
          Parsed::Response(
            RequestDone::new(
              RequestId::CreateGame(game_id),
              Err(Error::GameCreateReject(packet.reason()))
            )
          )
        }
        packet: PacketControllerUpdateSlotStatusAccept => {
          let id = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          Parsed::Response(
            RequestDone::new(
              id,
              Ok(Response::PlayerLeave(
                PlayerLeaveResponse::Accepted(S2ProtoEnum::unpack_enum(packet.status()))
              ))
            )
          )
        }
        packet: PacketControllerUpdateSlotStatusReject => {
          let id = RequestId::PlayerLeave(PlayerLeaveRequestId {
            game_id: packet.game_id,
            player_id: packet.player_id,
          });
          Parsed::Response(
            RequestDone::new(
              id,
              Ok(Response::PlayerLeave(PlayerLeaveResponse::Rejected(packet.reason())))
            )
          )
        }
        packet: PacketClientUpdateSlotClientStatus => {
          Parsed::GameSlotClientStatusUpdate(S2ProtoUnpack::unpack(packet)?)
        }
        packet: PacketNodeGameStatusUpdate => {
          Parsed::GameStatusUpdate(vec![GameStatusUpdate::from(packet)])
        }
        packet: PacketNodeGameStatusUpdateBulk => {
          Parsed::GameStatusUpdate(packet.games.into_iter().map(Into::into).collect())
        }
      }
    };

    match parsed {
      Parsed::Response(msg) => {
        if let Some(actor) = self.request_actor.as_ref() {
          tracing::debug!("response: {:?}", msg.id);
          actor.send(msg).await?;
        }
      }
      Parsed::GameSlotClientStatusUpdate(message) => {
        let addr = self.game_reg_addr.clone();
        ctx.spawn(async move {
          let game_id = message.game_id;
          if let Err(err) = addr.send_to(game_id, message).await {
            tracing::warn!(game_id, "GameSlotClientStatusUpdate: {}", err);
          }
        });
      }
      Parsed::GameStatusUpdate(messages) => {
        let addr = self.game_reg_addr.clone();
        ctx.spawn(async move {
          for message in messages {
            let game_id = message.game_id;
            let status = message.status;
            if let Err(err) = addr.send_to(message.game_id, message).await {
              let status = format!("{:?}", status);
              tracing::warn!(
                game_id,
                status = &status as &str,
                "game status update discarded: {:?}",
                err
              );
            } else {
              if !GameStatus::from(status).is_active() {
                tracing::debug!(game_id, "shutting down: reason: GameStatusUpdate");
                if let Err(err) = addr.send(Remove { game_id }).await {
                  tracing::warn!(game_id, "remove game: {:?}", err);
                }
              }
            }
          }
        });
      }
    }

    Ok(())
  }
}

pub struct NodeCreateGame {
  pub game: Game,
}

impl Message for NodeCreateGame {
  type Result = Result<FutureReply<Result<CreatedGameInfo>>>;
}

#[async_trait]
impl Handler<NodeCreateGame> for NodeConnActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    NodeCreateGame { game }: NodeCreateGame,
  ) -> Result<FutureReply<Result<CreatedGameInfo>>> {
    let addr = self
      .request_actor
      .as_ref()
      .map(|v| v.addr())
      .ok_or_else(|| Error::NodeNotReady)?;
    let (tx, rx) = FutureReply::channel();
    ctx.spawn(async move {
      tx.send(addr.create_game(game).await).ok();
    });
    Ok(rx)
  }
}

pub struct NodePlayerLeave {
  pub game_id: i32,
  pub player_id: i32,
}

impl Message for NodePlayerLeave {
  type Result = Result<FutureReply<Result<PlayerLeaveResponse>>>;
}

#[async_trait]
impl Handler<NodePlayerLeave> for NodeConnActor {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    NodePlayerLeave { game_id, player_id }: NodePlayerLeave,
  ) -> Result<FutureReply<Result<PlayerLeaveResponse>>> {
    let addr = self
      .request_actor
      .as_ref()
      .map(|v| v.addr())
      .ok_or_else(|| Error::NodeNotReady)?;
    let (tx, rx) = FutureReply::channel();
    ctx.spawn(async move {
      tx.send(addr.player_force_leave(game_id, player_id).await)
        .ok();
    });
    Ok(rx)
  }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum NodeConnStatus {
  Connecting,
  Connected,
  Error,
}

fn parse_addr(addr: &str) -> Result<(Ipv4Addr, u16)> {
  let (ip, port) = if addr.contains(":") {
    let addr = if let Some(addr) = addr.parse::<SocketAddrV4>().ok() {
      addr
    } else {
      return Err(Error::InvalidNodeAddress(addr.to_string()));
    };

    (
      addr.ip().clone(),
      addr.port() + flo_constants::NODE_CONTROLLER_PORT_OFFSET,
    )
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
