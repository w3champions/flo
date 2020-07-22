use async_tungstenite::tungstenite::Message as WsMessage;
use async_tungstenite::WebSocketStream;
use futures::future::{abortable, AbortHandle};
use futures::{SinkExt, TryStreamExt};
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing_futures::Instrument;

use crate::error::{Error, Result};
use crate::state::FloStateRef;
use crate::ws::message;
use crate::ws::message::{
  ErrorMessage, IncomingMessage, MapDetail, MapForceOwned, MapList, MapPath, MapPlayerOwned,
  OutgoingMessage,
};
use crate::ws::stream::{WsSenderRef, WsStream, WsStreamExt};

#[derive(Debug)]
pub struct WsHandler {
  abort_handle: AbortHandle,
}

impl WsHandler {
  pub fn new(g: FloStateRef, stream: WsStream) -> Self {
    let state = StateRef::new(g);
    let (f, abort_handle) = abortable(
      async move {
        if let Err(e) = state.serve_stream(stream).await {
          tracing::error!("websocket stream: {}", e);
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
    tokio::spawn(f);
    Self { abort_handle }
  }
}

impl Drop for WsHandler {
  fn drop(&mut self) {
    self.abort_handle.abort();
  }
}

#[derive(Debug, Clone)]
struct StateRef(Arc<RwLock<State>>);

impl StateRef {
  async fn serve_stream(&self, stream: WsStream) -> Result<()> {
    let (tx, mut rx) = stream.split();
    tx.send(self.get_client_info_message()).await?;

    let broken_notify = Arc::new(Notify::new());

    loop {
      tokio::select! {
        _ = broken_notify.notified() => {
          break;
        }
        next = rx.try_next() => {
          let msg = if let Some(msg) = next? {
            msg
          } else {
            break;
          };

          if let WsMessage::Close(_) = msg {
            break;
          } else {
            self.handle_message(broken_notify.clone(), tx.clone(), msg);
          }
        }
      }
    }

    Ok(())
  }

  fn handle_message(&self, broken_notify: Arc<Notify>, tx: WsSenderRef, msg: WsMessage) {
    let msg = match msg {
      WsMessage::Text(text) => match serde_json::from_str::<IncomingMessage>(&text) {
        Ok(msg) => Some(msg),
        Err(err) => {
          tracing::error!("malformed websocket message: {}, text: {}", err, text);
          None
        }
      },
      WsMessage::Binary(data) => match serde_json::from_slice::<IncomingMessage>(&data) {
        Ok(msg) => Some(msg),
        Err(err) => {
          tracing::error!("malformed websocket message: {}", err);
          None
        }
      },
      WsMessage::Ping(data) => {
        // tokio::spawn(Self::handle_ping(tx.clone(), data));
        None
      }
      WsMessage::Pong(data) => {
        // tokio::spawn(Self::handle_pong(tx.clone(), data));
        None
      }
      WsMessage::Close(_) => unreachable!(),
    };
    if let Some(msg) = msg {
      match msg {
        IncomingMessage::ReloadClientInfo => {
          Self::spawn_handler(
            broken_notify.clone(),
            self.clone().handle_reload_client_info(tx.clone()),
          );
        }
        IncomingMessage::Connect(connect) => Self::spawn_handler(
          broken_notify.clone(),
          self.clone().handle_connect(tx.clone(), connect.token),
        ),
        IncomingMessage::ListMaps => Self::spawn_handler(
          broken_notify.clone(),
          self.clone().handle_map_list(tx.clone()),
        ),
        IncomingMessage::GetMapDetail(payload) => Self::spawn_handler(
          broken_notify.clone(),
          self.clone().handle_get_map_detail(tx.clone(), payload),
        ),
        msg => {
          tracing::warn!("unknown incoming message: {:?}", msg);
        }
      }
    }
  }

  fn spawn_handler<F>(broken_notify: Arc<Notify>, f: F)
  where
    F: std::future::Future<Output = Result<()>> + Send + 'static,
  {
    tokio::spawn(async move {
      match f.await {
        Ok(_) => {}
        Err(e) => {
          tracing::error!("handler: {}", e);
          broken_notify.notify();
        }
      }
    });
  }

  async fn handle_connect(self, tx: WsSenderRef, token: String) -> Result<()> {
    match self
      .get_flo_state()
      .net
      .connect_lobby(tx.clone(), token)
      .await
    {
      Ok(_) => {}
      Err(err) => {
        tx.send(OutgoingMessage::ConnectRejected(ErrorMessage::new(
          match err {
            Error::ConnectionRequestRejected(reason) => format!("server rejected: {:?}", reason),
            other => other.to_string(),
          },
        )))
        .await?
      }
    }
    Ok(())
  }

  async fn handle_ping(tx: WsSenderRef, data: Vec<u8>) -> Result<()> {
    Ok(())
  }

  async fn handle_pong(tx: WsSenderRef, data: Vec<u8>) -> Result<()> {
    Ok(())
  }

  async fn handle_reload_client_info(self, tx: WsSenderRef) -> Result<()> {
    let g = self.get_flo_state();
    match g.platform.reload(&g.config).await {
      Ok(_) => tx.send(self.get_client_info_message()),
      Err(e) => tx.send(OutgoingMessage::ReloadClientInfoError(ErrorMessage {
        message: e.to_string(),
      })),
    }
    .await?;
    Ok(())
  }

  async fn handle_map_list(self, tx: WsSenderRef) -> Result<()> {
    let value = self.get_flo_state().platform.get_map_list().await;
    match value {
      Ok(value) => {
        tx.send(OutgoingMessage::ListMaps(MapList { data: value }))
          .await?
      }
      Err(e) => {
        tx.send(OutgoingMessage::ListMapsError(ErrorMessage::new(e)))
          .await?
      }
    }
    Ok(())
  }

  async fn handle_get_map_detail(self, tx: WsSenderRef, MapPath { path }: MapPath) -> Result<()> {
    let detail = self
      .get_flo_state()
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
      Ok(detail) => tx.send(OutgoingMessage::GetMapDetail(detail)).await?,
      Err(e) => {
        tx.send(OutgoingMessage::GetMapDetailError(ErrorMessage::new(e)))
          .await?;
      }
    }
    Ok(())
  }

  fn get_flo_state(&self) -> FloStateRef {
    self.0.read().g.clone()
  }

  fn get_client_info_message(&self) -> OutgoingMessage {
    OutgoingMessage::ClientInfo(message::ClientInfo {
      version: crate::version::FLO_VERSION_STRING.into(),
      war3_info: self.0.read().g.get_war3_info(),
    })
  }

  fn get_port(&self) -> u16 {
    let state = self.0.read();
    state.g.config.local_port.clone()
  }
}

impl StateRef {
  fn new(g: FloStateRef) -> Self {
    StateRef(Arc::new(RwLock::new(State { g })))
  }
}

#[derive(Debug)]
struct State {
  g: FloStateRef,
}
