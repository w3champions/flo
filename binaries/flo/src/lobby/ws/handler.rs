use async_tungstenite::tungstenite::Message as WsMessage;

use futures::TryStreamExt;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_net::packet::{FloPacket, Frame};
use flo_net::proto::flo_connect::{
  PacketGamePlayerPingMapSnapshotRequest, PacketGameSelectNodeRequest, PacketGameSlotUpdateRequest,
  PacketListNodesRequest,
};

use super::message::{
  ClientInfo, ErrorMessage, IncomingMessage, MapDetail, MapForceOwned, MapList, MapPath,
  MapPlayerOwned, OutgoingMessage, War3Info,
};
use super::stream::WsStream;
use super::{ConnectLobbyEvent, WsEvent, WsEventSender};
use crate::error::{Error, Result};
use crate::platform::PlatformStateRef;

#[derive(Debug, Clone)]
pub struct WsHandlerRef {
  state: Arc<State>,
}

impl WsHandlerRef {
  pub fn new(platform: PlatformStateRef, event_sender: WsEventSender, stream: WsStream) -> Self {
    let (ws_sender, ws_receiver) = channel(3);
    let dropper = Arc::new(Notify::new());
    let serve_state = Arc::new(ServeState {
      platform,
      event_sender,
    });
    tokio::spawn(
      {
        let dropper = dropper.clone();
        async move {
          if let Err(e) = serve_stream(serve_state.clone(), ws_receiver, dropper, stream).await {
            tracing::error!("serve stream: {}", e);
          } else {
            tracing::debug!("exiting");
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
    Self {
      state: Arc::new(State { ws_sender, dropper }),
    }
  }

  pub async fn send_or_discard(&self, msg: OutgoingMessage) -> bool {
    self.state.ws_sender.clone().send(msg).await.ok().is_some()
  }
}

#[derive(Debug)]
struct State {
  ws_sender: Sender<OutgoingMessage>,
  dropper: Arc<Notify>,
}

impl Drop for State {
  fn drop(&mut self) {
    self.dropper.notify()
  }
}

async fn serve_stream(
  state: Arc<ServeState>,
  mut ws_receiver: Receiver<OutgoingMessage>,
  dropper: Arc<Notify>,
  mut stream: WsStream,
) -> Result<()> {
  let msg = state.get_client_info_message();

  stream.send(msg).await?;

  let (reply_sender, mut receiver) = channel(3);

  loop {
    tokio::select! {
      _ = dropper.notified() => {
        stream.flush().await;
        break;
      }
      msg = ws_receiver.recv() => {
        if let Some(msg) = msg {
          stream.send(msg).await?;
        } else {
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
  platform: PlatformStateRef,
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
        self.handle_slot_update(req).await?;
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
    }
    Ok(())
  }

  fn get_client_info_message(&self) -> OutgoingMessage {
    OutgoingMessage::ClientInfo(ClientInfo {
      version: crate::version::FLO_VERSION_STRING.into(),
      war3_info: get_war3_info(self.platform.clone()),
    })
  }

  async fn handle_connect(&self, token: String) -> Result<()> {
    self
      .event_sender
      .clone()
      .send(WsEvent::ConnectLobbyEvent(ConnectLobbyEvent { token }))
      .await?;
    Ok(())
  }

  async fn handle_reload_client_info(&self, mut sender: Sender<OutgoingMessage>) -> Result<()> {
    match self.platform.reload().await {
      Ok(_) => sender.send(self.get_client_info_message()),
      Err(e) => sender.send(OutgoingMessage::ReloadClientInfoError(ErrorMessage {
        message: e.to_string(),
      })),
    }
    .await?;
    Ok(())
  }

  async fn handle_map_list(&self, mut sender: Sender<OutgoingMessage>) -> Result<()> {
    let value = self.platform.get_map_list().await;
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
      .with_storage(move |storage| -> Result<_> {
        use flo_w3map::W3Map;
        let (map, checksum) = W3Map::open_storage_with_checksum(storage, &path)?;
        let (width, height) = map.dimension();
        Ok(MapDetail {
          path,
          sha1: checksum.get_sha1_hex_string(),
          crc32: checksum.crc32,
          name: map.name().to_string(),
          author: map.author().to_string(),
          description: map.description().to_string(),
          width,
          height,
          preview_jpeg_base64: base64::encode(map.render_preview_jpeg()),
          suggested_players: map.suggested_players().to_string(),
          num_players: map.num_players(),
          players: map
            .get_players()
            .into_iter()
            .map(|p| MapPlayerOwned {
              name: p.name.to_string(),
              r#type: p.r#type,
              race: p.race,
              flags: p.flags,
            })
            .collect(),
          forces: map
            .get_forces()
            .into_iter()
            .map(|f| MapForceOwned {
              name: f.name.to_string(),
              flags: f.flags,
              player_set: f.player_set,
            })
            .collect(),
        })
      })
      .await;
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
}

fn get_war3_info(platform: PlatformStateRef) -> War3Info {
  let info = platform.map(|info| War3Info {
    located: true,
    version: info.version.clone().into(),
    error: None,
  });
  match info {
    Ok(info) => info,
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
