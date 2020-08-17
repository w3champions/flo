mod stream;
mod ws;
pub use stream::{DisconnectReason, LobbyStream, PlayerSession, PlayerSessionUpdate, RejectReason};

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::{FloPacket, Frame};

use crate::error::*;
use crate::node::{NodeRegistry, NodeRegistryRef, PingUpdate};
use crate::platform::PlatformStateRef;

use crate::lobby::stream::{LobbyStreamEvent, LobbyStreamEventSender};
use ws::message::{self, OutgoingMessage};
use ws::{Ws, WsEvent, WsMessageSender};

pub type LobbyEventSender = Sender<LobbyEvent>;

#[derive(Debug)]
pub struct Lobby {
  state: Arc<LobbyState>,
  ws_event_handler: WsEventHandler,
  stream_event_handler: LobbyStreamEventHandler,
  node_ping_update_handler: NodePingUpdateHandler,
}

impl Lobby {
  pub async fn init(platform: PlatformStateRef, sender: LobbyEventSender) -> Result<Self> {
    let (ping_sender, ping_receiver) = channel(1);
    let nodes = NodeRegistry::new(ping_sender).into_ref();

    let (ws_event_sender, ws_event_receiver) = channel(3);
    let ws = Ws::init(platform.clone(), ws_event_sender).await?;

    let (stream_event_sender, stream_event_receiver) = channel(1);

    let state = Arc::new(LobbyState::new(
      platform,
      nodes,
      ws,
      sender,
      stream_event_sender,
    ));

    let ws_event_handler = WsEventHandler::new(state.clone(), ws_event_receiver);
    let stream_event_handler = LobbyStreamEventHandler::new(state.clone(), stream_event_receiver);
    let node_ping_update_handler = NodePingUpdateHandler::new(state.clone(), ping_receiver);

    Ok(Self {
      state,
      ws_event_handler,
      stream_event_handler,
      node_ping_update_handler,
    })
  }
}

#[derive(Debug)]
struct WsEventHandler;

impl WsEventHandler {
  fn new(state: Arc<LobbyState>, mut receiver: Receiver<WsEvent>) -> Self {
    tokio::spawn(
      {
        async move {
          loop {
            if let Some(evt) = receiver.recv().await {
              state.clone().handle_ws_event(evt).await;
            } else {
              tracing::debug!("receiver dropped");
              break;
            }
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    WsEventHandler
  }
}

#[derive(Debug)]
struct LobbyStreamEventHandler;

impl LobbyStreamEventHandler {
  fn new(state: Arc<LobbyState>, mut receiver: Receiver<LobbyStreamEvent>) -> Self {
    tokio::spawn(
      {
        async move {
          loop {
            if let Some(evt) = receiver.recv().await {
              state.clone().handle_stream_event(evt).await;
            } else {
              tracing::debug!("receiver dropped");
              break;
            }
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    LobbyStreamEventHandler
  }
}

#[derive(Debug)]
struct NodePingUpdateHandler;

impl NodePingUpdateHandler {
  fn new(state: Arc<LobbyState>, mut receiver: Receiver<PingUpdate>) -> Self {
    tokio::spawn(
      {
        async move {
          loop {
            if let Some(update) = receiver.recv().await {
              state.clone().handle_ping_update(update).await;
            } else {
              tracing::debug!("receiver dropped");
              break;
            }
          }
          tracing::debug!("exiting");
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    NodePingUpdateHandler
  }
}

#[derive(Debug)]
struct LobbyState {
  id_counter: AtomicU64,
  platform: PlatformStateRef,
  nodes: NodeRegistryRef,
  ws: Ws,
  event_sender: LobbyEventSender,
  stream_event_sender: Sender<LobbyStreamEvent>,
  conn: RwLock<Option<LobbyConn>>,
}

impl LobbyState {
  fn new(
    platform: PlatformStateRef,
    nodes: NodeRegistryRef,
    ws: Ws,
    event_sender: LobbyEventSender,
    stream_event_sender: Sender<LobbyStreamEvent>,
  ) -> Self {
    LobbyState {
      id_counter: AtomicU64::new(0),
      platform,
      nodes,
      ws,
      event_sender,
      stream_event_sender,
      conn: RwLock::new(None),
    }
  }

  // try send a frame
  // if not connected, discard the frame
  // if connected, but the send failed, send disconnect msg to the conn's ws connection
  pub async fn send_frame_or_disconnect_ws(&self, frame: Frame) {
    let senders = self
      .conn
      .read()
      .as_ref()
      .map(|conn| (conn.ws_sender.clone(), conn.stream.get_sender_cloned()));
    if let Some((mut ws_sender, mut frame_sender)) = senders {
      if let Err(_) = frame_sender.send(frame).await {
        ws_sender
          .send(OutgoingMessage::Disconnect(message::Disconnect {
            reason: message::DisconnectReason::Unknown,
            message: "Connection closed unexpectedly.".to_string(),
          }))
          .await
          .ok();
      }
    }
  }

  async fn handle_ws_event(self: Arc<Self>, event: WsEvent) {
    match event {
      WsEvent::ConnectLobbyEvent(connect) => {
        *self.conn.write() = Some(LobbyConn::new(
          self.id_counter.fetch_add(1, Ordering::SeqCst),
          self.platform.clone(),
          self.stream_event_sender.clone().into(),
          self.nodes.clone(),
          connect.sender,
          connect.token,
        ));
      }
      WsEvent::LobbyFrameEvent(frame) => {
        self.send_frame_or_disconnect_ws(frame).await;
      }
      WsEvent::WorkerErrorEvent(err) => {
        self
          .event_sender
          .clone()
          .send_or_log_as_error(LobbyEvent::WsWorkerErrorEvent(err))
          .await;
      }
    }
  }

  async fn handle_stream_event(self: Arc<Self>, event: LobbyStreamEvent) {
    match event {
      LobbyStreamEvent::ConnectedEvent => {}
      LobbyStreamEvent::DisconnectedEvent(id) => {
        let mut guard = self.conn.write();
        if let Some(current_id) = guard.as_ref().map(|conn| conn.id) {
          if id == current_id {
            guard.take();
          }
        }
      }
      LobbyStreamEvent::ConnectionErrorEvent(err) => {
        tracing::error!("server connection: {}", err);
      }
      LobbyStreamEvent::GameInfoUpdateEvent(_) => {}
    }
  }

  async fn handle_ping_update(self: Arc<Self>, update: PingUpdate) {
    let node_id = update.node_id;
    let ping = update.ping.clone();

    let state = { self.conn.read().as_ref() }.and_then(|conn| {
      let ws_sender = conn.ws_sender.clone();
      let stream_state = conn
        .stream
        .current_game_id()
        .map(|game_id| (game_id, conn.stream.get_sender_cloned()));
      Some((ws_sender, stream_state))
    });

    let (mut ws_sender, stream_state) = if let Some((ws_sender, stream_state)) = state {
      (ws_sender, stream_state)
    } else {
      return;
    };

    ws_sender.send(OutgoingMessage::PingUpdate(update)).await.ok(/* browser window closed */);

    // we assume failed pings are all temporary
    // only upload succeed pings
    if let Some(ping) = ping {
      // upload ping update if joined a game
      if let Some((game_id, mut frame_sender)) = stream_state {
        use flo_net::proto::flo_connect::PacketGamePlayerPingMapUpdateRequest;
        if let Some(frame) = (PacketGamePlayerPingMapUpdateRequest {
          game_id,
          ping_map: {
            let mut map = HashMap::new();
            map.insert(node_id, ping);
            map
          },
        })
        .encode_as_frame()
        .ok()
        {
          let r = frame_sender.send(frame).await;
          if let Err(_) = r {
            tracing::debug!("conn frame sender dropped");
          }
        }
      }
    }
  }
}

#[derive(Debug)]
pub struct LobbyConn {
  id: u64,
  stream: LobbyStream,
  event_sender: LobbyStreamEventSender,
  ws_sender: WsMessageSender,
}

impl LobbyConn {
  fn new(
    id: u64,
    platform: PlatformStateRef,
    event_sender: LobbyStreamEventSender,
    nodes: NodeRegistryRef,
    ws_sender: WsMessageSender,
    token: String,
  ) -> Self {
    let domain = platform.with_config(|c| c.lobby_domain.clone());
    let stream = LobbyStream::new(
      id,
      &domain,
      ws_sender.clone(),
      event_sender.clone(),
      nodes.clone(),
      token,
    );

    Self {
      id,
      stream,
      event_sender,
      ws_sender,
    }
  }
}

impl Drop for LobbyConn {
  fn drop(&mut self) {
    self.event_sender.close();
  }
}

#[derive(Debug)]
pub enum LobbyEvent {
  WsWorkerErrorEvent(Error),
}

impl FloEvent for LobbyEvent {
  const NAME: &'static str = "LobbyEvent";
}
