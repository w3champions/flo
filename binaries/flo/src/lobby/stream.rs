use parking_lot::RwLock;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_event::*;
pub use flo_net::connect::*;
use flo_net::packet::*;
use flo_net::stream::FloStream;

use super::ws::{message, WsMessageSender};
use crate::error::*;
use crate::lobby::ws::message::OutgoingMessage;
use crate::lobby::LobbyGameInfo;
use crate::node::NodeRegistryRef;

pub type LobbyStreamEventSender = EventSender<LobbyStreamEvent>;

#[derive(Debug)]
pub struct LobbyStream {
  frame_sender: Sender<Frame>,
  current_game_id: Arc<RwLock<Option<i32>>>,
  dropper: Arc<Notify>,
}

impl Drop for LobbyStream {
  fn drop(&mut self) {
    self.dropper.notify();
  }
}

impl LobbyStream {
  pub fn new(
    id: u64,
    domain: &str,
    mut ws_sender: WsMessageSender,
    event_sender: LobbyStreamEventSender,
    nodes_reg: NodeRegistryRef,
    token: String,
  ) -> Self {
    let current_game_id = Arc::new(RwLock::new(None));
    let (frame_sender, frame_receiver) = channel(5);
    let dropper = Arc::new(Notify::new());

    tokio::spawn(
      {
        let current_game_id = current_game_id.clone();
        let domain = domain.to_string();
        let mut event_sender = event_sender.clone();
        let dropper = dropper.clone();
        async move {
          let serve = Self::connect_and_serve(
            id,
            &domain,
            event_sender.clone(),
            nodes_reg,
            frame_receiver,
            current_game_id,
            ws_sender.clone(),
            token,
          );

          let result = tokio::select! {
            _ = dropper.notified() => {
              Ok(())
            }
            res = serve => {
              res
            }
          };

          if let Err(err) = result {
            ws_sender
              .send(OutgoingMessage::ConnectRejected(
                message::ErrorMessage::new(match &err {
                  Error::ConnectionRequestRejected(reason) => {
                    format!("server rejected: {:?}", reason)
                  }
                  other => other.to_string(),
                }),
              ))
              .await
              .ok();
            event_sender
              .send_or_log_as_error(LobbyStreamEvent::ConnectionErrorEvent(err))
              .await;
          }

          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    Self {
      frame_sender,
      current_game_id,
      dropper,
    }
  }

  async fn connect_and_serve(
    id: u64,
    domain: &str,
    mut event_sender: LobbyStreamEventSender,
    nodes_reg: NodeRegistryRef,
    mut frame_receiver: Receiver<Frame>,
    current_game_id: Arc<RwLock<Option<i32>>>,
    mut ws_sender: WsMessageSender,
    token: String,
  ) -> Result<()> {
    let addr = format!("{}:{}", domain, flo_constants::LOBBY_SOCKET_PORT);
    tracing::debug!("connect addr: {}", addr);

    let mut stream = FloStream::connect(addr).await?;

    stream
      .send(PacketConnectLobby {
        connect_version: Some(crate::version::FLO_VERSION.into()),
        token,
      })
      .await?;

    let reply = stream.recv_frame().await?;

    let (session, nodes) = flo_net::try_flo_packet! {
      reply => {
        p = PacketConnectLobbyAccept => {
          nodes_reg.update_nodes(p.nodes.clone())?;
          (
            PlayerSession::unpack(p.session)?,
            p.nodes
          )
        }
        p = PacketConnectLobbyReject => {
          return Err(Error::ConnectionRequestRejected(S2ProtoEnum::unpack_enum(p.reason())))
        }
      }
    };

    *current_game_id.write() = session.game_id.clone();

    event_sender
      .send(LobbyStreamEvent::ConnectedEvent)
      .await
      .map_err(|_| Error::TaskCancelled)?;
    ws_sender
      .send({
        let mut list = message::NodeList {
          nodes: Vec::with_capacity(nodes.len()),
        };
        for node in nodes {
          list.nodes.push(message::Node {
            id: node.id,
            name: node.name,
            location: node.location,
            country_id: node.country_id,
            ping: nodes_reg.get_current_ping(node.id),
          })
        }
        message::OutgoingMessage::ListNodes(list)
      })
      .await?;
    ws_sender
      .send(message::OutgoingMessage::PlayerSession(session))
      .await?;

    loop {
      tokio::select! {
        next_send = frame_receiver.recv() => {
          if let Some(frame) = next_send {
            match stream.send_frame(frame).await {
              Ok(_) => {},
              Err(e) => {
                tracing::debug!("exiting: send error: {}", e);
                break;
              }
            }
          } else {
            tracing::debug!("exiting: sender dropped");
            break;
          }
        }
        recv = stream.recv_frame() => {
          match recv {
            Ok(mut frame) => {
              if frame.type_id == PacketTypeId::Ping {
                frame.type_id = PacketTypeId::Pong;
                match stream.send_frame(frame).await {
                  Ok(_) => {
                    continue;
                  },
                  Err(e) => {
                    tracing::debug!("exiting: send error: {}", e);
                    break;
                  }
                }
              }

              match Self::dispatch(&mut event_sender, &mut ws_sender, &nodes_reg, current_game_id.clone(), frame).await {
                Ok(_) => {},
                Err(e) => {
                  tracing::debug!("exiting: dispatch: {}", e);
                  break;
                }
              }
            },
            Err(e) => {
              tracing::debug!("exiting: recv: {}", e);
              break;
            }
          }
        }
      }
    }

    ws_sender
      .send(OutgoingMessage::Disconnect(message::Disconnect {
        reason: message::DisconnectReason::Unknown,
        message: "Server connection closed".to_string(),
      }))
      .await
      .ok();
    event_sender
      .send(LobbyStreamEvent::DisconnectedEvent(id))
      .await
      .ok();
    tracing::debug!("exiting");

    Ok(())
  }

  pub fn get_sender_cloned(&self) -> Sender<Frame> {
    self.frame_sender.clone()
  }

  pub fn current_game_id(&self) -> Option<i32> {
    self.current_game_id.read().clone()
  }

  // forward server packets to the websocket connection
  async fn dispatch(
    event_sender: &mut LobbyStreamEventSender,
    ws_sender: &mut WsMessageSender,
    nodes: &NodeRegistryRef,
    current_game_id: Arc<RwLock<Option<i32>>>,
    frame: Frame,
  ) -> Result<()> {
    let msg = flo_net::try_flo_packet! {
      frame => {
        p = PacketLobbyDisconnect => {
          message::OutgoingMessage::Disconnect(message::Disconnect {
            reason: S2ProtoEnum::unpack_i32(p.reason)?,
            message: "Server closed the connection".to_string()
          })
        }
        p = PacketGameInfo => {
          nodes.set_selected_node(p.game.as_ref().and_then(|g| {
            let node = g.node.as_ref()?;
            node.id.clone()
          }))?;

          let game: GameInfo = p.game.extract()?;

          event_sender.send_or_log_as_error(LobbyStreamEvent::GameInfoUpdateEvent(GameInfoUpdateEvent {
            game_info: Some(LobbyGameInfo {
              game_id: game.id,
              map_path: game.map.clone().extract()?.path,
            })
          })).await;

          OutgoingMessage::CurrentGameInfo(game)
        }
        p = PacketGamePlayerEnter => {
          OutgoingMessage::GamePlayerEnter(p)
        }
        p = PacketGamePlayerLeave => {
          OutgoingMessage::GamePlayerLeave(p)
        }
        p = PacketGameSlotUpdate => {
          OutgoingMessage::GameSlotUpdate(p)
        }
        p = PacketPlayerSessionUpdate => {
          if p.game_id.is_none() {
            nodes.set_selected_node(None)?;
          }
          *current_game_id.write() = p.game_id.clone();
          event_sender.send_or_log_as_error(LobbyStreamEvent::GameInfoUpdateEvent(GameInfoUpdateEvent {
            game_info: None
          })).await;
          OutgoingMessage::PlayerSessionUpdate(S2ProtoUnpack::unpack(p)?)
        }
        p = PacketListNodes => {
          nodes.update_nodes(p.nodes.clone())?;
          let mut list = message::NodeList {
            nodes: Vec::with_capacity(p.nodes.len())
          };
          for node in p.nodes {
            list.nodes.push(message::Node {
              id: node.id,
              name: node.name,
              location: node.location,
              country_id: node.country_id,
              ping: nodes.get_current_ping(node.id),
            })
          }
          OutgoingMessage::ListNodes(list)
        }
        p = PacketGameSelectNode => {
          nodes.set_selected_node(p.node_id)?;
          OutgoingMessage::GameSelectNode(p)
        }
        p = PacketGamePlayerPingMapUpdate => {
          OutgoingMessage::GamePlayerPingMapUpdate(p)
        }
        p = PacketGamePlayerPingMapSnapshot => {
          OutgoingMessage::GamePlayerPingMapSnapshot(p)
        }
        p = PacketGameStartReject => {
          OutgoingMessage::GameStartReject(p)
        }
        p = PacketGameStarting => {
           event_sender.send_or_log_as_error(LobbyStreamEvent::GameStartingEvent(GameStartingEvent {
            game_id: p.game_id,
          })).await;
          OutgoingMessage::GameStarting(p)
        }
        p = PacketGamePlayerToken => {
          event_sender.send_or_log_as_error(LobbyStreamEvent::GameStartedEvent(GameStartedEvent {
            node_id: p.node_id,
            game_id: p.game_id,
            player_token: p.player_token,
          })).await;
          OutgoingMessage::GameStarted(message::GameStarted{ game_id: p.game_id })
        }
      }
    };

    ws_sender.send(msg).await?;
    Ok(())
  }
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::LobbyDisconnectReason")]
pub enum DisconnectReason {
  Unknown = 0,
  Multi = 1,
  Maintenance = 2,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::Session")]
pub struct PlayerSession {
  pub player: PlayerInfo,
  pub status: PlayerStatus,
  pub game_id: Option<i32>,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::PacketPlayerSessionUpdate")]
pub struct PlayerSessionUpdate {
  pub status: PlayerStatus,
  pub game_id: Option<i32>,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerStatus")]
pub enum PlayerStatus {
  Idle = 0,
  InGame = 1,
}

#[derive(Debug, S2ProtoUnpack, Serialize)]
#[s2_grpc(message_type = "flo_net::proto::flo_connect::PlayerInfo")]
pub struct PlayerInfo {
  pub id: i32,
  pub name: String,
  pub source: PlayerSource,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::PlayerSource")]
pub enum PlayerSource {
  Test = 0,
  BNet = 1,
}

#[derive(Debug, S2ProtoEnum, PartialEq, Copy, Clone, Serialize)]
#[s2_grpc(proto_enum_type = "flo_net::proto::flo_connect::ConnectLobbyRejectReason")]
pub enum RejectReason {
  Unknown = 0,
  ClientVersionTooOld = 1,
  InvalidToken = 2,
}

#[derive(Debug)]
pub enum LobbyStreamEvent {
  ConnectedEvent,
  ConnectionErrorEvent(Error),
  GameInfoUpdateEvent(GameInfoUpdateEvent),
  GameStartingEvent(GameStartingEvent),
  GameStartedEvent(GameStartedEvent),
  DisconnectedEvent(u64),
}

#[derive(Debug)]
pub struct GameInfoUpdateEvent {
  pub game_info: Option<LobbyGameInfo>,
}

#[derive(Debug)]
pub struct GameStartingEvent {
  pub game_id: i32,
}

#[derive(Debug)]
pub struct GameStartedEvent {
  pub node_id: i32,
  pub game_id: i32,
  pub player_token: Vec<u8>,
}

impl FloEvent for LobbyStreamEvent {
  const NAME: &'static str = "LobbyStreamEvent";
}
