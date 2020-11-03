mod stream;
use stream::ControllerStream;
mod ws;

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::{FloPacket, Frame};

pub use crate::controller::stream::GameReceivedEvent;
use crate::controller::stream::{
  ControllerStreamEvent, ControllerStreamEventSender, PlayerSessionUpdateEvent,
};
use crate::error::*;
use crate::node::{NodeInfo, NodeRegistry, NodeRegistryRef, PingUpdate};
use crate::platform::{PlatformActor, GetClientPlatformInfo, CalcMapChecksum, GetClientConfig};
use crate::types::{GameInfo, Slot};
use crate::types::{GameStatusUpdate, PlayerInfo, PlayerSession};

use crate::controller::ws::message::ClientUpdateSlotClientStatus;
use ws::message::{self, OutgoingMessage};
use ws::{Ws, WsEvent, WsMessageSender};
use flo_state::Addr;

pub type ControllerEventSender = Sender<ControllerEvent>;

#[derive(Debug)]
pub struct ControllerClient {
  state: Arc<State>,
  ws_event_handler: WsEventHandler,
  stream_event_handler: ControllerStreamEventHandler,
  node_ping_update_handler: NodePingUpdateHandler,
}

impl ControllerClient {
  pub async fn init(platform: Addr<PlatformActor>, sender: ControllerEventSender) -> Result<Self> {
    let (ping_sender, ping_receiver) = channel(1);
    let nodes = NodeRegistry::new(ping_sender).into_ref();

    let (ws_event_sender, ws_event_receiver) = channel(3);
    let ws = Ws::init(platform.clone(), ws_event_sender).await?;

    let (stream_event_sender, stream_event_receiver) = channel(1);

    let state = Arc::new(State::new(platform, nodes, ws, sender, stream_event_sender));

    let ws_event_handler = WsEventHandler::new(state.clone(), ws_event_receiver);
    let stream_event_handler =
      ControllerStreamEventHandler::new(state.clone(), stream_event_receiver);
    let node_ping_update_handler = NodePingUpdateHandler::new(state.clone(), ping_receiver);

    Ok(Self {
      state,
      ws_event_handler,
      stream_event_handler,
      node_ping_update_handler,
    })
  }

  pub fn handle(&self) -> ControllerClientHandle {
    ControllerClientHandle(self.state.clone())
  }
}

#[derive(Debug, Clone)]
pub struct ControllerClientHandle(Arc<State>);

impl ControllerClientHandle {
  // pub fn current_game_info(&self) -> Option<Arc<LocalGameInfo>> {
  //   self.0.current_game_info.read().clone()
  // }

  pub fn with_player_session<F, R>(&self, f: F) -> Option<R>
  where
    F: FnOnce(&PlayerSession) -> R,
  {
    if let Some(session) = self.0.current_session.read().as_ref() {
      Some(f(session))
    } else {
      None
    }
  }

  // pub fn node_registry(&self) -> NodeRegistryRef {
  //   self.0.nodes.clone()
  // }

  pub fn get_node(&self, id: i32) -> Option<Arc<NodeInfo>> {
    self.0.nodes.get_node(id)
  }

  pub async fn ws_send_update_slot_client_status(&self, msg: ClientUpdateSlotClientStatus) {
    self
      .0
      .ws
      .send_or_discard(OutgoingMessage::GameSlotClientStatusUpdate(msg))
      .await;
  }

  pub async fn ws_send_update_game_status(&self, msg: GameStatusUpdate) {
    self
      .0
      .ws
      .send_or_discard(OutgoingMessage::GameStatusUpdate(msg))
      .await;
  }
}

#[derive(Debug)]
struct WsEventHandler;

impl WsEventHandler {
  fn new(state: Arc<State>, mut receiver: Receiver<WsEvent>) -> Self {
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
struct ControllerStreamEventHandler;

impl ControllerStreamEventHandler {
  fn new(state: Arc<State>, mut receiver: Receiver<ControllerStreamEvent>) -> Self {
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

    ControllerStreamEventHandler
  }
}

#[derive(Debug)]
struct NodePingUpdateHandler;

impl NodePingUpdateHandler {
  fn new(state: Arc<State>, mut receiver: Receiver<PingUpdate>) -> Self {
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
struct State {
  id_counter: AtomicU64,
  platform: Addr<PlatformActor>,
  nodes: NodeRegistryRef,
  ws: Ws,
  event_sender: ControllerEventSender,
  stream_event_sender: Sender<ControllerStreamEvent>,
  conn: RwLock<Option<LobbyConn>>,
  current_game_info: RwLock<Option<Arc<LocalGameInfo>>>,
  current_session: RwLock<Option<PlayerSession>>,
}

impl State {
  fn new(
    platform: Addr<PlatformActor>,
    nodes: NodeRegistryRef,
    ws: Ws,
    event_sender: ControllerEventSender,
    stream_event_sender: Sender<ControllerStreamEvent>,
  ) -> Self {
    State {
      id_counter: AtomicU64::new(0),
      platform,
      nodes,
      ws,
      event_sender,
      stream_event_sender,
      conn: RwLock::new(None),
      current_game_info: RwLock::new(None),
      current_session: RwLock::new(None),
    }
  }

  // try send a frame
  // if not connected, discard the frame
  // if connected, but the send failed, send disconnect msg to the conn's ws connection
  pub async fn send_frame_or_disconnect_ws(&self, frame: Frame) -> bool {
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
      } else {
        return true;
      }
    }
    false
  }

  async fn handle_ws_event(self: Arc<Self>, event: WsEvent) {
    match event {
      WsEvent::ConnectLobbyEvent(connect) => {
        let conn = LobbyConn::new(
          self.id_counter.fetch_add(1, Ordering::SeqCst),
          self.platform.clone(),
          self.stream_event_sender.clone().into(),
          self.nodes.clone(),
          connect.sender,
          connect.token,
        ).await;
        let conn = match conn {
          Ok(v) => v,
          Err(err) => {
            tracing::error!("connect to controller: {}", err);
            return
          }
        };
        *self.conn.write() = Some(conn);
      }
      WsEvent::LobbyFrameEvent(frame) => {
        self.send_frame_or_disconnect_ws(frame).await;
      }
      WsEvent::WorkerErrorEvent(err) => {
        self
          .event_sender
          .clone()
          .send_or_log_as_error(ControllerEvent::WsWorkerErrorEvent(err))
          .await;
      }
    }
  }

  async fn handle_stream_event(self: Arc<Self>, event: ControllerStreamEvent) {
    match event {
      ControllerStreamEvent::ConnectedEvent => {
        self
          .event_sender
          .clone()
          .send_or_log_as_error(ControllerEvent::ConnectedEvent)
          .await;
      }
      ControllerStreamEvent::DisconnectedEvent(id) => {
        self.current_session.write().take();
        self.remove_conn(id);
        self
          .event_sender
          .clone()
          .send_or_log_as_error(ControllerEvent::DisconnectedEvent)
          .await;
      }
      ControllerStreamEvent::ConnectionErrorEvent(id, err) => {
        tracing::error!("server connection: {}", err);
        self.remove_conn(id);
        self
          .event_sender
          .clone()
          .send_or_log_as_error(ControllerEvent::DisconnectedEvent)
          .await;
      }
      ControllerStreamEvent::PlayerSessionUpdateEvent(event) => match event {
        PlayerSessionUpdateEvent::Full(session) => {
          *self.current_session.write() = Some(session);
        }
        PlayerSessionUpdateEvent::Partial(update) => {
          let mut guard = self.current_session.write();
          if let Some(current) = guard.as_mut() {
            current.game_id = update.game_id;
            current.status = update.status;
          } else {
            tracing::error!(
              "PlayerSessionUpdateEvent emitted by there is no active player session."
            )
          }
        }
      },
      ControllerStreamEvent::GameInfoUpdateEvent(event) => {
        let game_info = event.game_info;
        *self.current_game_info.write() = game_info.clone();
        self
          .event_sender
          .clone()
          .send_or_log_as_error(ControllerEvent::GameInfoUpdateEvent(game_info))
          .await;
      }
      ControllerStreamEvent::GameStartingEvent(event) => {
        let game_id = event.game_id;
        if let Err(err) = self.handle_game_start(game_id).await {
          tracing::error!("get client info: {}", err);
        }
      }
      ControllerStreamEvent::GameReceivedEvent(event) => {
        let game_info = { self.current_game_info.read().clone() };
        if let Some(game_info) = game_info {
          self
            .event_sender
            .clone()
            .send_or_log_as_error(ControllerEvent::GameStartedEvent(event, game_info))
            .await;
        } else {
          tracing::error!(
            node_id = event.node_id,
            game_id = event.game_id,
            "game info unavailable"
          );
        }
      }
    }
  }

  fn remove_conn(&self, id: u64) -> Option<LobbyConn> {
    let mut guard = self.conn.write();
    if let Some(current_id) = guard.as_ref().map(|conn| conn.id) {
      if id == current_id {
        return guard.take();
      }
    }
    None
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
        use flo_net::proto::flo_connect::{PacketGamePlayerPingMapUpdateRequest, PingStatsInput};
        if let Some(frame) = (PacketGamePlayerPingMapUpdateRequest {
          game_id,
          ping_map: {
            let mut map = HashMap::new();
            map.insert(
              node_id,
              PingStatsInput {
                current: Some(ping),
                ..Default::default()
              },
            );
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

  async fn handle_game_start(self: Arc<Self>, game_id: i32) -> Result<()> {
    let info = { self.current_game_info.read().clone() };
    if let Some(info) = info {
      if info.game_id == game_id {
        let client_info = self.platform.send(GetClientPlatformInfo).await?.map_err(|_| Error::War3NotLocated)?;
        let version = client_info.version;
        let sha1 = self
          .platform
          .send(CalcMapChecksum {
            path: info.map_path.clone()
          })
          .await??.sha1;
        self
          .send_frame_or_disconnect_ws(
            flo_net::proto::flo_connect::PacketGameStartPlayerClientInfoRequest {
              game_id,
              war3_version: version,
              map_sha1: sha1.to_vec(),
            }
            .encode_as_frame()?,
          )
          .await;
      }
    }
    Ok(())
  }
}

#[derive(Debug)]
pub struct LobbyConn {
  id: u64,
  stream: ControllerStream,
  event_sender: ControllerStreamEventSender,
  ws_sender: WsMessageSender,
}

impl LobbyConn {
  async fn new(
    id: u64,
    platform: Addr<PlatformActor>,
    event_sender: ControllerStreamEventSender,
    nodes: NodeRegistryRef,
    ws_sender: WsMessageSender,
    token: String,
  ) -> Result<Self> {
    let domain = platform.send(GetClientConfig).await?.controller_domain;
    let stream = ControllerStream::new(
      id,
      &domain,
      ws_sender.clone(),
      event_sender.clone(),
      nodes.clone(),
      token,
    );

    Ok(Self {
      id,
      stream,
      event_sender,
      ws_sender,
    })
  }
}

impl Drop for LobbyConn {
  fn drop(&mut self) {
    self.event_sender.close();
  }
}

#[derive(Debug, Clone)]
pub struct LocalGameInfo {
  pub game_id: i32,
  pub random_seed: i32,
  pub node_id: Option<i32>,
  pub player_id: i32,
  pub map_path: String,
  pub map_sha1: [u8; 20],
  pub map_checksum: u32,
  pub players: HashMap<i32, PlayerInfo>,
  pub slots: Vec<Slot>,
  pub host_player: Option<PlayerInfo>,
}

impl LocalGameInfo {
  pub fn from_game_info(player_id: i32, game: &GameInfo) -> Result<Self> {
    Ok(Self {
      game_id: game.id,
      random_seed: game.random_seed,
      node_id: game.node.as_ref().map(|node| node.id),
      player_id,
      map_path: game.map.path.clone(),
      map_sha1: {
        if game.map.sha1.len() != 20 {
          return Err(Error::InvalidMapInfo);
        }
        let mut value = [0_u8; 20];
        value.copy_from_slice(&game.map.sha1[..]);
        value
      },
      map_checksum: game.map.checksum,
      players: game
        .slots
        .iter()
        .filter_map(|slot| slot.player.clone().map(|player| (player.id, player)))
        .collect(),
      slots: game.slots.clone(),
      host_player: game.created_by.clone(),
    })
  }
}

#[derive(Debug)]
pub enum ControllerEvent {
  ConnectedEvent,
  DisconnectedEvent,
  GameStartedEvent(GameReceivedEvent, Arc<LocalGameInfo>),
  GameInfoUpdateEvent(Option<Arc<LocalGameInfo>>),
  WsWorkerErrorEvent(Error),
}

impl FloEvent for ControllerEvent {
  const NAME: &'static str = "ControllerEvent";
}
