use parking_lot::RwLock;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::*;
use flo_net::proto::flo_connect as proto;
use flo_net::stream::FloStream;

use super::ws::{message, WsMessageSender};
use crate::controller::ws::message::{GamePlayerEnter, OutgoingMessage};
use crate::controller::LocalGameInfo;
use crate::error::*;
use crate::node::NodeRegistryRef;
use crate::types::*;

pub type ControllerStreamEventSender = EventSender<ControllerStreamEvent>;

#[derive(Debug)]
pub struct LobbyStream {
  frame_sender: Sender<Frame>,
  state: Arc<State>,
  dropper: Arc<Notify>,
}

impl Drop for LobbyStream {
  fn drop(&mut self) {
    self.dropper.notify();
  }
}

#[derive(Debug)]
struct State {
  event_sender: ControllerStreamEventSender,
  nodes_reg: NodeRegistryRef,
  current_game_info: Arc<RwLock<Option<Arc<LocalGameInfo>>>>,
  ws_sender: WsMessageSender,
}

impl State {
  async fn mutate_local_game_info<F, R>(&self, f: F) -> Option<R>
  where
    F: FnOnce(&mut LocalGameInfo) -> R,
  {
    let r = { self.current_game_info.read().clone() }?;
    let mut mutated = r.clone();

    let res = f(Arc::make_mut(&mut mutated));
    if !Arc::ptr_eq(&r, &mutated) {
      self.current_game_info.write().replace(mutated);
    }
    self
      .event_sender
      .clone()
      .send_or_log_as_error(ControllerStreamEvent::GameInfoUpdateEvent(
        GameInfoUpdateEvent {
          game_info: Some(r.clone()),
        },
      ))
      .await;
    Some(res)
  }
}

impl LobbyStream {
  pub fn new(
    id: u64,
    domain: &str,
    mut ws_sender: WsMessageSender,
    event_sender: ControllerStreamEventSender,
    nodes_reg: NodeRegistryRef,
    token: String,
  ) -> Self {
    let (frame_sender, frame_receiver) = channel(5);
    let dropper = Arc::new(Notify::new());
    let state = Arc::new(State {
      event_sender: event_sender.clone(),
      nodes_reg,
      current_game_info: Arc::new(RwLock::new(None)),
      ws_sender: ws_sender.clone(),
    });

    tokio::spawn(
      {
        let state = state.clone();
        let domain = domain.to_string();
        let mut event_sender = event_sender.clone();
        let dropper = dropper.clone();
        async move {
          let serve = Self::connect_and_serve(id, &domain, token, frame_receiver, state);

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
              .send_or_log_as_error(ControllerStreamEvent::ConnectionErrorEvent(id, err))
              .await;
          }

          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    Self {
      frame_sender,
      state,
      dropper,
    }
  }

  async fn connect_and_serve(
    id: u64,
    domain: &str,
    token: String,
    mut frame_receiver: Receiver<Frame>,
    state: Arc<State>,
  ) -> Result<()> {
    let mut event_sender = state.event_sender.clone();
    let mut ws_sender = state.ws_sender.clone();

    let addr = format!("{}:{}", domain, flo_constants::CONTROLLER_SOCKET_PORT);
    tracing::debug!("connect addr: {}", addr);

    let mut stream = FloStream::connect(addr).await?;

    stream
      .send(proto::PacketClientConnect {
        connect_version: Some(crate::version::FLO_VERSION.into()),
        token,
      })
      .await?;

    let reply = stream.recv_frame().await?;

    let (session, nodes): (PlayerSession, _) = flo_net::try_flo_packet! {
      reply => {
        p = proto::PacketClientConnectAccept => {
          state.nodes_reg.update_nodes(p.nodes.clone())?;
          (
            PlayerSession::unpack(p.session)?,
            p.nodes
          )
        }
        p = proto::PacketClientConnectReject => {
          return Err(Error::ConnectionRequestRejected(S2ProtoEnum::unpack_enum(p.reason())))
        }
      }
    };

    let player_id = session.player.id;

    event_sender
      .send(ControllerStreamEvent::ConnectedEvent)
      .await
      .map_err(|_| Error::TaskCancelled)?;
    event_sender
      .send(ControllerStreamEvent::PlayerSessionUpdateEvent(
        PlayerSessionUpdateEvent::Full(session.clone()),
      ))
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
            ping: state.nodes_reg.get_current_ping(node.id),
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

              match Self::dispatch(player_id, &mut event_sender, &mut ws_sender, &state, frame).await {
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
      .send(ControllerStreamEvent::DisconnectedEvent(id))
      .await
      .ok();
    tracing::debug!("exiting");

    Ok(())
  }

  pub fn get_sender_cloned(&self) -> Sender<Frame> {
    self.frame_sender.clone()
  }

  pub fn current_game_id(&self) -> Option<i32> {
    self
      .state
      .current_game_info
      .read()
      .as_ref()
      .map(|info| info.game_id)
  }

  // handle controller packets
  async fn dispatch(
    player_id: i32,
    event_sender: &mut ControllerStreamEventSender,
    ws_sender: &mut WsMessageSender,
    state: &State,
    frame: Frame,
  ) -> Result<()> {
    let msg = flo_net::try_flo_packet! {
      frame => {
        p = proto::PacketClientDisconnect => {
          message::OutgoingMessage::Disconnect(message::Disconnect {
            reason: S2ProtoEnum::unpack_i32(p.reason)?,
            message: format!("Server closed the connection: {:?}", p.reason)
          })
        }
        p = proto::PacketGameInfo => {
          state.nodes_reg.set_selected_node(p.game.as_ref().and_then(|g| {
            g.node.as_ref().map(|node| node.id)
          }))?;

          let game = GameInfo::unpack(p.game)?;

          let local_game_info = Arc::new(LocalGameInfo::from_game_info(player_id, &game)?);
          state.current_game_info.write().replace(local_game_info.clone());

          let evt = ControllerStreamEvent::GameInfoUpdateEvent(GameInfoUpdateEvent {
            game_info: Some(local_game_info)
          });

          event_sender.send_or_log_as_error(evt).await;

          OutgoingMessage::CurrentGameInfo(game)
        }
        p = proto::PacketGamePlayerEnter => {
          state.mutate_local_game_info(|info| -> Result<_> {
            if let Some(slot) = info.slots.get_mut(p.slot_index as usize) {
              *slot = Slot::unpack(p.slot.clone())?;
              Ok(())
            } else {
              tracing::error!("PacketGamePlayerEnter: invalid slot index: {}", p.slot_index);
              Err(Error::InvalidMapInfo)
            }
          }).await.ok_or_else(|| {
            tracing::error!("PacketGamePlayerEnter came before PacketGameInfo");
            Error::UnexpectedControllerPacket
          })??;
          OutgoingMessage::GamePlayerEnter(S2ProtoUnpack::unpack(p)?)
        }
        p = proto::PacketGamePlayerLeave => {
          state.mutate_local_game_info(|info| -> Result<_> {
            if let Some(slot) = info.slots.iter_mut().find(|s| s.player.as_ref().map(|p| p.id) == Some(p.player_id)) {
              *slot = Slot::default();
              Ok(())
            } else {
              tracing::error!(game_id = p.game_id, player_id = p.player_id, "PacketGamePlayerLeave: player slot not found");
              Err(Error::InvalidMapInfo)
            }
          }).await.ok_or_else(|| {
            tracing::error!(game_id = p.game_id, player_id = p.player_id, "PacketGamePlayerLeave came before PacketGameInfo");
            Error::UnexpectedControllerPacket
          })??;
          OutgoingMessage::GamePlayerLeave(p)
        }
        p = proto::PacketGameSlotUpdate => {
          state.mutate_local_game_info(|info| -> Result<_> {
            if let Some(slot) = info.slots.get_mut(p.slot_index as usize) {
              slot.settings = SlotSettings::unpack(p.slot_settings.clone())?;
              Ok(())
            } else {
              tracing::error!("PacketGamePlayerEnter: invalid slot index: {}", p.slot_index);
              Err(Error::InvalidMapInfo)
            }
          }).await.ok_or_else(|| {
            tracing::error!("PacketGamePlayerEnter came before PacketGameInfo");
            Error::UnexpectedControllerPacket
          })??;
          OutgoingMessage::GameSlotUpdate(S2ProtoUnpack::unpack(p)?)
        }
        p = proto::PacketPlayerSessionUpdate => {
          let session = PlayerSessionUpdate::unpack(p)?;
          event_sender.send_or_log_as_error(ControllerStreamEvent::PlayerSessionUpdateEvent(PlayerSessionUpdateEvent::Partial(session.clone()))).await;
          if session.game_id.is_none() {
            state.nodes_reg.set_selected_node(None)?;
            state.current_game_info.write().take();
            event_sender.send_or_log_as_error(ControllerStreamEvent::GameInfoUpdateEvent(GameInfoUpdateEvent {
              game_info: None
            })).await;
          }
          OutgoingMessage::PlayerSessionUpdate(session)
        }
        p = proto::PacketListNodes => {
          state.nodes_reg.update_nodes(p.nodes.clone())?;
          let mut list = message::NodeList {
            nodes: Vec::with_capacity(p.nodes.len())
          };
          for node in p.nodes {
            list.nodes.push(message::Node {
              id: node.id,
              name: node.name,
              location: node.location,
              country_id: node.country_id,
              ping: state.nodes_reg.get_current_ping(node.id),
            })
          }
          OutgoingMessage::ListNodes(list)
        }
        p = proto::PacketGameSelectNode => {
          state.nodes_reg.set_selected_node(p.node_id)?;
          state.mutate_local_game_info(|info| info.node_id = p.node_id).await.ok_or_else(|| {
            tracing::error!("PacketGameSelectNode came before PacketGameInfo");
            Error::UnexpectedControllerPacket
          })?;
          OutgoingMessage::GameSelectNode(p)
        }
        p = proto::PacketGamePlayerPingMapUpdate => {
          OutgoingMessage::GamePlayerPingMapUpdate(p)
        }
        p = proto::PacketGamePlayerPingMapSnapshot => {
          OutgoingMessage::GamePlayerPingMapSnapshot(p)
        }
        p = proto::PacketGameStartReject => {
          OutgoingMessage::GameStartReject(p)
        }
        p = proto::PacketGameStarting => {
          event_sender.send_or_log_as_error(ControllerStreamEvent::GameStartingEvent(GameStartingEvent {
            game_id: p.game_id,
          })).await;
          OutgoingMessage::GameStarting(p)
        }
        p = proto::PacketGamePlayerToken => {
          event_sender.send_or_log_as_error(ControllerStreamEvent::GameStartedEvent(GameStartedEvent {
            node_id: p.node_id,
            game_id: p.game_id,
            player_token: p.player_token,
          })).await;
          OutgoingMessage::GameStarted(message::GameStarted{
            game_id: p.game_id,
            lan_game_name: {
              crate::lan::get_lan_game_name(p.game_id, p.player_id)
            }
          })
        }
        p = proto::PacketClientUpdateSlotClientStatus => {
          OutgoingMessage::GameSlotClientStatusUpdate(S2ProtoUnpack::unpack(p)?)
        }
      }
    };

    ws_sender.send(msg).await?;
    Ok(())
  }
}

#[derive(Debug)]
pub enum ControllerStreamEvent {
  ConnectedEvent,
  ConnectionErrorEvent(u64, Error),
  PlayerSessionUpdateEvent(PlayerSessionUpdateEvent),
  GameInfoUpdateEvent(GameInfoUpdateEvent),
  GameStartingEvent(GameStartingEvent),
  GameStartedEvent(GameStartedEvent),
  DisconnectedEvent(u64),
}

#[derive(Debug)]
pub enum PlayerSessionUpdateEvent {
  Full(PlayerSession),
  Partial(PlayerSessionUpdate),
}

#[derive(Debug)]
pub struct GameInfoUpdateEvent {
  pub game_info: Option<Arc<LocalGameInfo>>,
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

impl FloEvent for ControllerStreamEvent {
  const NAME: &'static str = "LobbyStreamEvent";
}
