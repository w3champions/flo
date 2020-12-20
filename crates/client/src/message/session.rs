use super::message::{
  ClientInfo, ErrorMessage, IncomingMessage, MapList, MapPath, OutgoingMessage, War3Info,
};
use super::{ConnectController, MessageEvent};
use crate::controller::{ControllerClient, SendFrame};
use crate::error::{Error, Result};
use crate::message::MessageStream;
use crate::platform::{
  GetClientPlatformInfo, GetMapDetail, GetMapList, KillTestGame, Platform, PlatformStateError,
  Reload,
};
use flo_net::packet::FloPacket;
use flo_net::proto::flo_connect::{
  PacketGamePlayerPingMapSnapshotRequest, PacketGameSlotUpdateRequest, PacketGameStartRequest,
  PacketListNodesRequest,
};
use flo_platform::ClientPlatformInfo;
use flo_state::Addr;
use flo_task::{SpawnScope, SpawnScopeHandle};
use s2_grpc_utils::S2ProtoPack;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

#[derive(Debug)]
pub struct Session {
  scope: SpawnScope,
  tx: Sender<OutgoingMessage>,
}

impl Session {
  pub fn new(
    platform: Addr<Platform>,
    client: Addr<ControllerClient>,
    stream: MessageStream,
  ) -> Self {
    let (tx, rx) = channel(3);
    let scope = SpawnScope::new();
    let serve_state = Arc::new(Worker { platform, client });
    tokio::spawn(
      {
        let scope = scope.handle();
        async move {
          if let Err(e) = serve_stream(serve_state.clone(), rx, scope, stream).await {
            tracing::error!("serve stream: {}", e);
          } else {
            tracing::debug!("exiting");
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
    Self { scope, tx: tx }
  }

  pub fn sender(&self) -> WsSender {
    WsSender(self.tx.clone())
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
  state: Arc<Worker>,
  mut rx: Receiver<OutgoingMessage>,
  mut scope: SpawnScopeHandle,
  mut stream: MessageStream,
) -> Result<()> {
  let msg = state.get_client_info_message().await?;

  stream.send(msg).await?;

  let (reply_sender, mut receiver) = channel(3);

  loop {
    tokio::select! {
      _ = scope.left() => {
        rx.close();
        while let Some(msg) = rx.try_recv().ok() {
          stream.send(msg).await?;
        }
        stream.flush().await;
        break;
      }
      msg = rx.recv() => {
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

struct Worker {
  platform: Addr<Platform>,
  client: Addr<ControllerClient>,
}

impl Worker {
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
        self.handle_connect(connect.token).await?;
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
        self
          .send_frame::<PacketGameSlotUpdateRequest>(req.pack()?)
          .await?;
      }
      IncomingMessage::ListNodesRequest => {
        self.send_frame(PacketListNodesRequest {}).await?;
      }
      IncomingMessage::GameSelectNodeRequest(req) => {
        self.send_frame(req).await?;
      }
      IncomingMessage::GamePlayerPingMapSnapshotRequest(req) => {
        self
          .send_frame::<PacketGamePlayerPingMapSnapshotRequest>(req)
          .await?;
      }
      IncomingMessage::GameStartRequest(req) => {
        self.send_frame::<PacketGameStartRequest>(req).await?;
      }
      IncomingMessage::StartTestGame(msg) => {
        self.platform.send(msg).await??;
      }
      IncomingMessage::KillTestGame => {
        self.platform.notify(KillTestGame).await?;
      }
    }
    Ok(())
  }

  async fn get_client_info_message(&self) -> Result<OutgoingMessage> {
    let info = self
      .platform
      .send(GetClientPlatformInfo {
        force_reload: false,
      })
      .await?;
    Ok(OutgoingMessage::ClientInfo(ClientInfo {
      version: crate::version::FLO_VERSION_STRING.into(),
      war3_info: get_war3_info(info),
    }))
  }

  async fn handle_connect(&self, token: String) -> Result<()> {
    self
      .client
      .notify(MessageEvent::ConnectController(ConnectController { token }))
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
    let detail = self.platform.send(GetMapDetail { path }).await?;
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

  async fn send_frame<T: FloPacket>(&self, pkt: T) -> Result<()> {
    self
      .client
      .send(SendFrame(pkt.encode_as_frame()?))
      .await??;
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
    Error::TaskCancelled(anyhow::format_err!("websocket message dropped"))
  }
}
