use parking_lot::RwLock;
use std::sync::Arc;

use flo_config::ClientConfig;
use flo_net::packet::FloPacket;

use crate::error::{Error, Result};
use crate::net::lobby::{LobbyStream, LobbyStreamSender};
use crate::ws::message::{Disconnect, DisconnectReason};
use crate::ws::{OutgoingMessage, WsSenderRef};

#[derive(Debug)]
pub struct NetState {
  lobby: LobbyState,
}

impl NetState {
  pub async fn init(config: &ClientConfig) -> Result<Self> {
    Ok(Self {
      lobby: LobbyState::new(&config.lobby_domain),
    })
  }

  pub fn into_ref(self) -> NetStateRef {
    Arc::new(self)
  }

  pub async fn connect_lobby(&self, ws_sender: WsSenderRef, token: String) -> Result<()> {
    Ok(self.lobby.connect(ws_sender, token).await?)
  }

  pub async fn lobby_send<T>(&self, packet: T) -> Result<()>
  where
    T: FloPacket,
  {
    let mut sender = self
      .lobby
      .get_sender_cloned()
      .ok_or_else(|| Error::ServerNotConnected)?;
    if let Err(_) = sender.send(packet.encode_as_frame()?).await {
      tracing::debug!("sender dropped");
      self.lobby.close().await;
      return Err(Error::ServerNotConnected);
    }
    Ok(())
  }
}

pub type NetStateRef = Arc<NetState>;

#[derive(Debug)]
pub enum NetEvent {
  Lobby,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ConnStatus {
  Idle,
  Connecting,
  Connected,
  Disconnected,
}

#[derive(Debug)]
struct LobbyState {
  domain: String,
  conn: RwLock<Option<LobbyConn>>,
}

#[derive(Debug)]
struct LobbyConn {
  stream: LobbyStream,
  ws_sender: WsSenderRef,
}

impl LobbyState {
  pub fn new(domain: &str) -> Self {
    LobbyState {
      domain: domain.to_string(),
      conn: RwLock::new(None),
    }
  }

  pub async fn connect(&self, ws_sender: WsSenderRef, token: String) -> Result<()> {
    let stream = LobbyStream::connect(&self.domain, ws_sender.clone(), token).await?;

    {
      *self.conn.write() = Some(LobbyConn { stream, ws_sender })
    }

    Ok(())
  }

  async fn close(&self) {
    let conn = self.conn.write().take();
    if let Some(conn) = conn {
      let r = conn
        .ws_sender
        .send(OutgoingMessage::Disconnect(Disconnect {
          reason: DisconnectReason::Unknown,
          message: "Server connection closed unexpectedly".to_string(),
        }))
        .await;
      if let Err(e) = r {
        tracing::debug!("send disconnect: {}", e);
      }
    }
  }

  fn get_sender_cloned(&self) -> Option<LobbyStreamSender> {
    self
      .conn
      .read()
      .as_ref()
      .map(|s| s.stream.get_sender_cloned())
  }
}
