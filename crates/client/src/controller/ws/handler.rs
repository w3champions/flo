use s2_grpc_utils::S2ProtoPack;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

use flo_net::packet::FloPacket;
use flo_net::proto::flo_connect::{
  PacketGamePlayerPingMapSnapshotRequest, PacketGameSelectNodeRequest, PacketGameSlotUpdateRequest,
  PacketGameStartRequest, PacketListNodesRequest,
};
use flo_task::{SpawnScope, SpawnScopeHandle};

use super::message::{
  ClientInfo, ErrorMessage, IncomingMessage, MapList, MapPath, OutgoingMessage, War3Info,
};
use super::stream::WsStream;
use super::{ConnectLobbyEvent, WsEvent, WsEventSender};
use crate::error::{Error, Result};
use crate::platform::{PlatformActor, PlatformStateError, GetClientPlatformInfo, GetMapList, GetMapDetail, Reload};
use flo_state::Addr;
use flo_platform::ClientPlatformInfo;

#[derive(Debug)]
pub struct WsHandler {
  scope: SpawnScope,
  ws_sender: Sender<OutgoingMessage>,
}

impl WsHandler {
  pub fn new(platform: Addr<PlatformActor>, event_sender: WsEventSender, stream: WsStream) -> Self {
    let (ws_sender, ws_receiver) = channel(3);
    let scope = SpawnScope::new();
    let serve_state = Arc::new(ServeState {
      platform,
      event_sender,
    });
    tokio::spawn(
      {
        let scope = scope.handle();
        async move {
          if let Err(e) = serve_stream(serve_state.clone(), ws_receiver, scope, stream).await {
            tracing::error!("serve stream: {}", e);
          } else {
            tracing::debug!("exiting");
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
    Self { scope, ws_sender }
  }

  pub fn sender(&self) -> WsSender {
    WsSender(self.ws_sender.clone())
  }
}

#[derive(Debug, Clone)]
pub struct WsSender(Sender<OutgoingMessage>);

impl WsSender {
  pub async fn send_or_discard(&self, msg: OutgoingMessage) {
    self.0.clone().send(msg).await.ok();
  }
}

async fn serve_stream(
  state: Arc<ServeState>,
  mut ws_receiver: Receiver<OutgoingMessage>,
  mut scope: SpawnScopeHandle,
  mut stream: WsStream,
) -> Result<()> {
  let msg = state.get_client_info_message().await?;

  stream.send(msg).await?;

  let (reply_sender, mut receiver) = channel(3);

  loop {
    tokio::select! {
      _ = scope.left() => {
        ws_receiver.close();
        while let Some(msg) = ws_receiver.try_recv().ok() {
          stream.send(msg).await?;
        }
        stream.flush().await;
        break;
      }
      msg = ws_receiver.recv() => {
        if let Some(msg) = msg {
          stream.send(msg).await?;
        } else {
          tracing::debug!("ws sender dropped");
          break;
        }
      }
      msg = receiver.recv() => {
        let msg = msg.expect("sender hold on stack");
        stream.send(msg).await?;
      }
      next = stream.recv() => {
        let msg = if let Some(msg) = next {
          msg
        } else {
          tracing::debug!("stream closed");
          break;
        };

        state.handle_message(&reply_sender, msg).await?;
      }
    }
  }

  Ok(())
}

#[derive(Debug)]
struct ServeState {
  platform: Addr<PlatformActor>,
  event_sender: WsEventSender,
}

impl ServeState {
  async fn handle_message(
    &self,
    reply_sender: &Sender<OutgoingMessage>,
    msg: IncomingMessage,
  ) -> Result<()> {
    match msg {
      IncomingMessage::ReloadClientInfo => {
        self.handle_reload_client_info(reply_sender.clone()).await?;
      }
      IncomingMessage::Connect(connect) => {
        self
          .handle_connect(reply_sender.clone(), connect.token)
          .await?;
      }
      IncomingMessage::ListMaps => {
        self.handle_map_list(reply_sender.clone()).await?;
      }
      IncomingMessage::GetMapDetail(payload) => {
        self
          .handle_get_map_detail(reply_sender.clone(), payload)
          .await?;
      }
      IncomingMessage::GameSlotUpdateRequest(req) => {
        self.handle_slot_update(req.pack()?).await?;
      }
      IncomingMessage::ListNodesRequest => {
        self.handle_list_nodes_request().await?;
      }
      IncomingMessage::GameSelectNodeRequest(req) => {
        self.handle_select_node(req).await?;
      }
      IncomingMessage::GamePlayerPingMapSnapshotRequest(req) => {
        self.handle_player_ping_map_snapshot_request(req).await?;
      }
      IncomingMessage::GameStartRequest(req) => {
        self.handle_game_start_request(req).await?;
      }
    }
    Ok(())
  }

  async fn get_client_info_message(&self) -> Result<OutgoingMessage> {
    let info = self.platform.send(GetClientPlatformInfo).await?;
    Ok(OutgoingMessage::ClientInfo(ClientInfo {
      version: crate::version::FLO_VERSION_STRING.into(),
      war3_info: get_war3_info(info),
    }))
  }

  async fn handle_connect(&self, sender: Sender<OutgoingMessage>, token: String) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::ConnectLobbyEvent(ConnectLobbyEvent {
        sender,
        token,
      }))
      .await?;
    Ok(())
  }

  async fn handle_reload_client_info(&self, mut sender: Sender<OutgoingMessage>) -> Result<()> {
    match self.platform.send(Reload).await? {
      Ok(_) => sender.send(self.get_client_info_message().await?),
      Err(e) => sender.send(OutgoingMessage::ReloadClientInfoError(ErrorMessage {
        message: e.to_string(),
      })),
    }
    .await?;
    Ok(())
  }

  async fn handle_map_list(&self, mut sender: Sender<OutgoingMessage>) -> Result<()> {
    let value = self.platform.send(GetMapList).await?;
    match value {
      Ok(value) => {
        sender
          .send(OutgoingMessage::ListMaps(MapList { data: value }))
          .await?
      }
      Err(e) => {
        sender
          .send(OutgoingMessage::ListMapsError(ErrorMessage::new(e)))
          .await?
      }
    }
    Ok(())
  }

  async fn handle_get_map_detail(
    &self,
    mut sender: Sender<OutgoingMessage>,
    MapPath { path }: MapPath,
  ) -> Result<()> {
    let detail = self
      .platform
      .send(GetMapDetail {
        path
      })
      .await?;
    match detail {
      Ok(detail) => sender.send(OutgoingMessage::GetMapDetail(detail)).await?,
      Err(e) => {
        sender
          .send(OutgoingMessage::GetMapDetailError(ErrorMessage::new(e)))
          .await?;
      }
    }
    Ok(())
  }

  async fn handle_slot_update(&self, req: PacketGameSlotUpdateRequest) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::LobbyFrameEvent(req.encode_as_frame()?))
      .await?;
    Ok(())
  }

  async fn handle_list_nodes_request(&self) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::LobbyFrameEvent(
        PacketListNodesRequest {}.encode_as_frame()?,
      ))
      .await?;
    Ok(())
  }

  async fn handle_select_node(&self, req: PacketGameSelectNodeRequest) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::LobbyFrameEvent(req.encode_as_frame()?))
      .await?;
    Ok(())
  }

  async fn handle_player_ping_map_snapshot_request(
    &self,
    req: PacketGamePlayerPingMapSnapshotRequest,
  ) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::LobbyFrameEvent(req.encode_as_frame()?))
      .await?;
    Ok(())
  }

  async fn handle_game_start_request(&self, req: PacketGameStartRequest) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::LobbyFrameEvent(req.encode_as_frame()?))
      .await?;
    Ok(())
  }
}

fn get_war3_info(info: Result<ClientPlatformInfo, PlatformStateError>) -> War3Info {
  match info {
    Ok(info) => War3Info {
      located: true,
      version: info.version.clone().into(),
      error: None,
    },
    Err(e) => War3Info {
      located: false,
      version: None,
      error: Some(e),
    },
  }
}

impl From<tokio::sync::mpsc::error::SendError<OutgoingMessage>> for Error {
  fn from(_: tokio::sync::mpsc::error::SendError<OutgoingMessage>) -> Error {
    tracing::debug!("OutgoingMessage dropped");
    Error::TaskCancelled
  }
}
