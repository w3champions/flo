mod stream;
#[cfg(test)]
mod stream_test;

pub use crate::controller::stream::GameReceivedEvent;
use crate::controller::stream::{ControllerEvent, ControllerEventData, PlayerSessionUpdateEvent};
pub use crate::controller::stream::{ControllerStream, SendFrame};
use crate::error::*;
use crate::lan::{
  KillLanGame, Lan, LanEvent, ReplaceLanGame, StopLanGame, UpdateLanGamePlayerStatus,
  UpdateLanGameStatus,
};
use crate::message::message::{self, OutgoingMessage};
use crate::message::ConnectController;
use crate::message::{MessageEvent, Session};
use crate::node::stream::NodeStreamEvent;
use crate::node::{
  GetNode, NodeRegistry, SetActiveNode, UpdateAddressesAndGetNodePingMap, UpdateNodes,
};
use crate::platform::{GetClientConfig, Platform};
use crate::types::PlayerSession;
use crate::StartConfig;
use flo_config::ClientConfig;
use flo_net::packet::FloPacket;
use flo_net::packet::Frame;
use flo_state::{
  async_trait, Actor, Addr, Container, Context, Handler, Message, RegistryRef, Service,
};
use std::sync::Arc;

pub struct ControllerClient {
  config: ClientConfig,
  platform: Addr<Platform>,
  nodes: Addr<NodeRegistry>,
  lan: Addr<Lan>,
  conn: Option<Container<ControllerStream>>,
  conn_id: u64,
  ws_conn: Option<Session>,
  current_session: Option<PlayerSession>,
  initial_token: Option<String>,
  mute_list: Vec<i32>,
}

impl ControllerClient {
  fn connect(&mut self, ctx: &mut Context<Self>, token: String) {
    self.conn_id = self.conn_id.wrapping_add(1);
    let stream = ControllerStream::new(
      ctx.addr(),
      self.platform.clone(),
      self.nodes.clone(),
      self.conn_id,
      &self.config.controller_host,
      token,
    );
    self.conn.replace(stream.start());
  }

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

  async fn send_frame(&mut self, frame: Frame) -> Result<()> {
    if let Some(stream) = self.conn.as_mut() {
      stream.send(SendFrame(frame)).await??;
      Ok(())
    } else {
      Err(Error::ControllerDisconnected)
    }
  }
}

#[async_trait]
impl Actor for ControllerClient {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    if let Some(token) = self.initial_token.take() {
      self.connect(ctx, token);
    }
  }
}

#[async_trait]
impl Service<StartConfig> for ControllerClient {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve::<Platform>().await?;
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
      initial_token: registry.data().token.clone(),
      mute_list: vec![],
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
              tracing::info!(
                player_id = session.player.id,
                "player session replaced: game_id = {:?}",
                session.game_id
              );
              self.current_session.replace(session);
            }
            PlayerSessionUpdateEvent::Partial(update) => {
              if let Some(current) = self.current_session.as_mut() {
                current.game_id = update.game_id;
                current.status = update.status;
                tracing::info!(
                  player_id = current.player.id,
                  "player session updated: game_id = {:?}",
                  current.game_id
                );
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
              self.lan.notify(KillLanGame).await.ok();
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
impl Handler<MessageEvent> for ControllerClient {
  async fn handle(
    &mut self,
    ctx: &mut Context<Self>,
    message: MessageEvent,
  ) -> <MessageEvent as Message>::Result {
    match message {
      MessageEvent::ConnectController(ConnectController { token }) => {
        self.connect(ctx, token);
      }
      MessageEvent::WorkerError(_) => {}
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

pub struct ReplaceSession(pub Session);

impl Message for ReplaceSession {
  type Result = ();
}

#[async_trait]
impl Handler<ReplaceSession> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    ReplaceSession(sess): ReplaceSession,
  ) -> <ReplaceSession as Message>::Result {
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
      LanEvent::LanGameDisconnected { game_id } => {
        self.lan.notify(StopLanGame { game_id }).await.ok();
      }
      LanEvent::NodeStreamEvent { game_id, inner } => match inner {
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
          self.lan.notify(StopLanGame { game_id }).await.ok();
        }
      },
    }
  }
}

pub struct UpdateMuteList {
  pub mute_list: Vec<i32>,
}

impl Message for UpdateMuteList {
  type Result = ();
}

#[async_trait]
impl Handler<UpdateMuteList> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UpdateMuteList { mute_list }: UpdateMuteList,
  ) -> <UpdateMuteList as Message>::Result {
    self.mute_list = mute_list;
  }
}

pub struct GetMuteList;

impl Message for GetMuteList {
  type Result = Vec<i32>;
}

#[async_trait]
impl Handler<GetMuteList> for ControllerClient {
  async fn handle(&mut self, _: &mut Context<Self>, _: GetMuteList) -> Vec<i32> {
    self.mute_list.clone()
  }
}

pub struct MutePlayer {
  pub player_id: i32,
}

impl Message for MutePlayer {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<MutePlayer> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    MutePlayer { player_id }: MutePlayer,
  ) -> Result<()> {
    self
      .send_frame(
        flo_net::proto::flo_connect::PacketPlayerMuteAddRequest { player_id }.encode_as_frame()?,
      )
      .await?;
    Ok(())
  }
}

pub struct UnmutePlayer {
  pub player_id: i32,
}

impl Message for UnmutePlayer {
  type Result = Result<()>;
}

#[async_trait]
impl Handler<UnmutePlayer> for ControllerClient {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    UnmutePlayer { player_id }: UnmutePlayer,
  ) -> Result<()> {
    self
      .send_frame(
        flo_net::proto::flo_connect::PacketPlayerMuteRemoveRequest { player_id }
          .encode_as_frame()?,
      )
      .await?;
    Ok(())
  }
}
