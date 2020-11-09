mod stream;
#[cfg(test)]
mod stream_test;

pub use crate::controller::stream::GameReceivedEvent;
use crate::controller::stream::{ControllerEvent, ControllerEventData, PlayerSessionUpdateEvent};
pub use crate::controller::stream::{ControllerStream, SendFrame};
use crate::error::*;
use crate::lan::{
  Lan, LanEvent, ReplaceLanGame, StopLanGame, UpdateLanGamePlayerStatus, UpdateLanGameStatus,
};
use crate::node::stream::NodeStreamEvent;
use crate::node::{
  GetNode, NodeRegistry, SetActiveNode, UpdateAddressesAndGetNodePingMap, UpdateNodes,
};
use crate::platform::{GetClientConfig, PlatformActor};
use crate::types::PlayerSession;
use crate::ws::message::{self, OutgoingMessage};
use crate::ws::ConnectController;
use crate::ws::{WsEvent, WsSession};
use flo_config::ClientConfig;
use flo_state::{
  async_trait, Actor, Addr, Container, Context, Handler, Message, RegistryRef, Service,
};
use std::sync::Arc;

pub struct ControllerClient {
  config: ClientConfig,
  platform: Addr<PlatformActor>,
  nodes: Addr<NodeRegistry>,
  lan: Addr<Lan>,
  conn: Option<Container<ControllerStream>>,
  conn_id: u64,
  ws_conn: Option<WsSession>,
  current_session: Option<PlayerSession>,
}

impl ControllerClient {
  async fn ws_send(&self, message: OutgoingMessage) {
    if let Some(sender) = self.ws_conn.as_ref().map(|v| v.sender()) {
      sender.send_or_discard(message).await;
    }
  }

  async fn replace_lan_game(&self, event: GameReceivedEvent) {
    let game_id = event.game_info.game_id;
    tracing::info!(game_id, "replace lan game");
    let player_session = if let Some(v) = self.current_session.as_ref() {
      v
    } else {
      tracing::error!("received GameStartedEvent but there is no active player session.");
      return;
    };

    let node_info = if let Ok(node_info) = self
      .nodes
      .send(GetNode {
        node_id: event.node_id,
      })
      .await
    {
      if let Some(v) = node_info {
        v
      } else {
        tracing::error!("unknown node id: {}", event.node_id);
        return;
      }
    } else {
      return;
    };

    let msg = ReplaceLanGame {
      my_player_id: player_session.player.id,
      node: Arc::new(node_info),
      player_token: event.player_token,
      game: event.game_info,
    };

    if let Err(err) = self
      .lan
      .send(msg)
      .await
      .map_err(Error::from)
      .and_then(std::convert::identity)
    {
      tracing::error!("update lan game: {}", err);
      self
        .ws_send(OutgoingMessage::GameStartError(message::ErrorMessage::new(
          err,
        )))
        .await;
    } else {
      self
        .ws_send(OutgoingMessage::GameStarted(message::GameStarted {
          game_id,
          lan_game_name: { crate::lan::get_lan_game_name(game_id, player_session.player.id) },
        }))
        .await;
    }
  }
}

impl Actor for ControllerClient {}

#[async_trait]
impl Service for ControllerClient {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<()>) -> Result<Self, Self::Error> {
    let platform = registry.resolve::<PlatformActor>().await?;
    let config = platform.send(GetClientConfig).await?;
    Ok(Self {
      config,
      platform,
      nodes: registry.resolve().await?,
      lan: registry.resolve().await?,
      conn: None,
      conn_id: 0,
      ws_conn: None,
      current_session: None,
    })
  }
}

pub struct SendWs {
  pub conn_id: u64,
  pub message: OutgoingMessage,
}

impl SendWs {
  pub fn new(conn_id: u64, message: OutgoingMessage) -> Self {
    SendWs { conn_id, message }
  }

  pub async fn notify(self, addr: &Addr<ControllerClient>) -> Result<()> {
    addr.notify(self).await.map_err(Into::into)
  }
}

impl Message for SendWs {
  type Result = ();
}

#[async_trait]
impl Handler<SendWs> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    SendWs { conn_id, message }: SendWs,
  ) -> <SendWs as Message>::Result {
    if self.conn_id == conn_id {
      self.ws_send(message).await;
    }
  }
}

#[async_trait]
impl Handler<ControllerEvent> for ControllerClient {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    message: ControllerEvent,
  ) -> <ControllerEvent as Message>::Result {
    match message {
      ControllerEvent { id, data } => {
        if id != self.conn_id {
          return;
        }

        match data {
          ControllerEventData::Connected => {}
          ControllerEventData::ConnectionError(err) => {
            tracing::error!("connection error: {}", err);
            if let Some(stream) = self.conn.take() {
              ctx.spawn(async move {
                stream.shutdown().await.ok();
              });
            }
          }
          ControllerEventData::PlayerSessionUpdate(event) => match event {
            PlayerSessionUpdateEvent::Full(session) => {
              self.current_session.replace(session);
            }
            PlayerSessionUpdateEvent::Partial(update) => {
              if let Some(current) = self.current_session.as_mut() {
                current.game_id = update.game_id;
                current.status = update.status;
              } else {
                tracing::error!(
                  "PlayerSessionUpdateEvent emitted by there is no active player session."
                )
              }
            }
          },
          ControllerEventData::GameInfoUpdate(event) => match event.game_info {
            Some(game_info) => {
              tracing::debug!(game_id = game_info.game_id, "game info update");
            }
            None => {
              self.lan.notify(StopLanGame).await.ok();
            }
          },
          ControllerEventData::GameReceived(event) => {
            self.replace_lan_game(event).await;
          }
          ControllerEventData::SelectNode(node_id) => {
            if let Err(err) = self
              .nodes
              .send(SetActiveNode { node_id })
              .await
              .map_err(Error::from)
              .and_then(std::convert::identity)
            {
              tracing::error!("select active node: {}", err);
            }
          }
          ControllerEventData::Disconnected => {
            if let Some(stream) = self.conn.take() {
              ctx.spawn(async move {
                stream.shutdown().await.ok();
              });
            }
            self.ws_conn.take();
          }
        }
      }
    }
  }
}

#[async_trait]
impl Handler<UpdateNodes> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateNodes { nodes }: UpdateNodes,
  ) -> <UpdateNodes as Message>::Result {
    let mut ping_map = self
      .nodes
      .send(UpdateAddressesAndGetNodePingMap(UpdateNodes {
        nodes: nodes.clone(),
      }))
      .await??;
    let mut list = message::NodeList {
      nodes: Vec::with_capacity(nodes.len()),
    };
    for node in nodes {
      list.nodes.push(message::Node {
        id: node.id,
        name: node.name,
        location: node.location,
        country_id: node.country_id,
        ping: ping_map.remove(&node.id),
      })
    }
    self
      .ws_send(message::OutgoingMessage::ListNodes(list))
      .await;
    Ok(())
  }
}

#[async_trait]
impl Handler<WsEvent> for ControllerClient {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    message: WsEvent,
  ) -> <WsEvent as Message>::Result {
    match message {
      WsEvent::ConnectController(ConnectController { token }) => {
        self.conn_id = self.conn_id.wrapping_add(1);
        let stream = ControllerStream::new(
          ctx.addr(),
          self.platform.clone(),
          self.nodes.clone(),
          self.conn_id,
          &self.config.controller_domain,
          token,
        );
        self.conn.replace(stream.start());
      }
      WsEvent::WorkerError(_) => {}
    }
  }
}

#[async_trait]
impl Handler<SendFrame> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    message: SendFrame,
  ) -> <SendFrame as Message>::Result {
    if let Some(stream) = self.conn.as_ref() {
      stream.send(message).await??;
    } else {
      tracing::error!("frame dropped: {:?}", message.0.type_id);
      self.ws_conn.take();
    }
    Ok(())
  }
}

pub struct ReplaceWsSession(pub WsSession);

impl Message for ReplaceWsSession {
  type Result = ();
}

#[async_trait]
impl Handler<ReplaceWsSession> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    ReplaceWsSession(sess): ReplaceWsSession,
  ) -> <ReplaceWsSession as Message>::Result {
    if let Some(replaced) = self.ws_conn.replace(sess) {
      replaced
        .sender()
        .send_or_discard(OutgoingMessage::Disconnect(message::Disconnect {
          reason: message::DisconnectReason::Multi,
          message: "Another browser window took up the connection.".to_string(),
        }))
        .await;
    }
  }
}

#[async_trait]
impl Handler<LanEvent> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    message: LanEvent,
  ) -> <LanEvent as Message>::Result {
    match message {
      LanEvent::LanGameDisconnected => {
        self.lan.notify(StopLanGame).await.ok();
      }
      LanEvent::NodeStreamEvent(event) => match event {
        NodeStreamEvent::SlotClientStatusUpdate(update) => {
          self
            .ws_send(OutgoingMessage::GameSlotClientStatusUpdate(update.clone()))
            .await;
          if let Err(err) = self
            .lan
            .send(UpdateLanGamePlayerStatus {
              game_id: update.game_id,
              player_id: update.player_id,
              status: update.status,
            })
            .await
            .map_err(Error::from)
            .and_then(std::convert::identity)
          {
            tracing::error!(
              game_id = update.game_id,
              player_id = update.player_id,
              "update lan player status to {:?}: {}",
              update.status,
              err
            );
          }
        }
        NodeStreamEvent::GameInitialStatus(data) => {
          let game_id = data.game_id;
          tracing::debug!(game_id, "GameInitialStatus: {:?}", data.game_status);
          if let Err(err) = self
            .lan
            .send(UpdateLanGameStatus {
              game_id: data.game_id,
              status: data.game_status,
              updated_player_game_client_status_map: data.player_game_client_status_map,
            })
            .await
            .map_err(Error::from)
            .and_then(std::convert::identity)
          {
            tracing::error!(
              game_id,
              "update lan game status to {:?}: {}",
              data.game_status,
              err
            );
          }
        }
        NodeStreamEvent::GameStatusUpdate(update) => {
          let game_id = update.game_id;
          let game_status = update.status;
          self
            .ws_send(OutgoingMessage::GameStatusUpdate(update.clone()))
            .await;
          tracing::debug!(game_id, "GameStatusUpdate: {:?}", update.status);
          if let Err(err) = self
            .lan
            .send(UpdateLanGameStatus {
              game_id,
              status: game_status,
              updated_player_game_client_status_map: update.updated_player_game_client_status_map,
            })
            .await
            .map_err(Error::from)
            .and_then(std::convert::identity)
          {
            tracing::error!(
              game_id,
              "update lan game status to {:?}: {}",
              game_status,
              err
            );
          }
        }
        NodeStreamEvent::Disconnected => {
          self.lan.notify(StopLanGame).await.ok();
        }
      },
    }
  }
}

// impl State {
//   fn new(
//     platform: Addr<PlatformActor>,
//     nodes: NodeRegistryRef,
//     ws: Ws,
//     event_sender: ControllerEventSender,
//     stream_event_sender: Sender<ControllerStreamEvent>,
//   ) -> Self {
//     State {
//       id_counter: AtomicU64::new(0),
//       platform,
//       nodes,
//       ws,
//       event_sender,
//       stream_event_sender,
//       conn: RwLock::new(None),
//       current_game_info: RwLock::new(None),
//       current_session: RwLock::new(None),
//     }
//   }
//
//   // try send a frame
//   // if not connected, discard the frame
//   // if connected, but the send failed, send disconnect msg to the conn's ws connection
//   pub async fn send_frame_or_disconnect_ws(&self, frame: Frame) -> bool {
//     let senders = self
//       .conn
//       .read()
//       .as_ref()
//       .map(|conn| (conn.ws_sender.clone(), conn.stream.get_sender_cloned()));
//     if let Some((mut ws_sender, mut frame_sender)) = senders {
//       if let Err(_) = frame_sender.send(frame).await {
//         ws_sender
//           .send(OutgoingMessage::Disconnect(message::Disconnect {
//             reason: message::DisconnectReason::Unknown,
//             message: "Connection closed unexpectedly.".to_string(),
//           }))
//           .await
//           .ok();
//       } else {
//         return true;
//       }
//     }
//     false
//   }
//
//   async fn handle_ws_event(self: Arc<Self>, event: WsEvent) {
//     match event {
//       WsEvent::ConnectControllerEvent(connect) => {
//         let conn = ControllerConn::new(
//           self.id_counter.fetch_add(1, Ordering::SeqCst),
//           self.platform.clone(),
//           self.stream_event_sender.clone().into(),
//           self.nodes.clone(),
//           connect.sender,
//           connect.token,
//         )
//         .await;
//         let conn = match conn {
//           Ok(v) => v,
//           Err(err) => {
//             tracing::error!("connect to controller: {}", err);
//             return;
//           }
//         };
//         *self.conn.write() = Some(conn);
//       }
//       WsEvent::LobbyFrameEvent(frame) => {
//         self.send_frame_or_disconnect_ws(frame).await;
//       }
//       WsEvent::WorkerErrorEvent(err) => {
//         self
//           .event_sender
//           .clone()
//           .send_or_log_as_error(ControllerEvent::WsWorkerErrorEvent(err))
//           .await;
//       }
//     }
//   }
//
//   async fn handle_stream_event(self: Arc<Self>, event: ControllerStreamEvent) {
//     match event {
//       ControllerStreamEvent::ConnectedEvent => {
//         self
//           .event_sender
//           .clone()
//           .send_or_log_as_error(ControllerEvent::ConnectedEvent)
//           .await;
//       }
//       ControllerStreamEvent::DisconnectedEvent(id) => {
//         self.current_session.write().take();
//         self.remove_conn(id);
//         self
//           .event_sender
//           .clone()
//           .send_or_log_as_error(ControllerEvent::DisconnectedEvent)
//           .await;
//       }
//       ControllerStreamEvent::ConnectionErrorEvent(id, err) => {
//         tracing::error!("server connection: {}", err);
//         self.remove_conn(id);
//         self
//           .event_sender
//           .clone()
//           .send_or_log_as_error(ControllerEvent::DisconnectedEvent)
//           .await;
//       }
//       ControllerStreamEvent::PlayerSessionUpdateEvent(event) => match event {
//         PlayerSessionUpdateEvent::Full(session) => {
//           *self.current_session.write() = Some(session);
//         }
//         PlayerSessionUpdateEvent::Partial(update) => {
//           let mut guard = self.current_session.write();
//           if let Some(current) = guard.as_mut() {
//             current.game_id = update.game_id;
//             current.status = update.status;
//           } else {
//             tracing::error!(
//               "PlayerSessionUpdateEvent emitted by there is no active player session."
//             )
//           }
//         }
//       },
//       ControllerStreamEvent::GameInfoUpdateEvent(event) => {
//         let game_info = event.game_info;
//         *self.current_game_info.write() = game_info.clone();
//         self
//           .event_sender
//           .clone()
//           .send_or_log_as_error(ControllerEvent::GameInfoUpdateEvent(game_info))
//           .await;
//       }
//       ControllerStreamEvent::GameStartingEvent(event) => {
//         let game_id = event.game_id;
//         if let Err(err) = self.handle_game_start(game_id).await {
//           tracing::error!("get client info: {}", err);
//         }
//       }
//       ControllerStreamEvent::GameReceivedEvent(event) => {
//         let game_info = { self.current_game_info.read().clone() };
//         if let Some(game_info) = game_info {
//           self
//             .event_sender
//             .clone()
//             .send_or_log_as_error(ControllerEvent::GameStartedEvent(event, game_info))
//             .await;
//         } else {
//           tracing::error!(
//             node_id = event.node_id,
//             game_id = event.game_id,
//             "game info unavailable"
//           );
//         }
//       }
//     }
//   }
//
//   fn remove_conn(&self, id: u64) -> Option<ControllerConn> {
//     let mut guard = self.conn.write();
//     if let Some(current_id) = guard.as_ref().map(|conn| conn.id) {
//       if id == current_id {
//         return guard.take();
//       }
//     }
//     None
//   }
