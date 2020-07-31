

use parking_lot::RwLock;
use std::sync::Arc;


use flo_config::ClientConfig;

use crate::error::{Result};
use crate::net::lobby::LobbyStream;
use crate::ws::WsSenderRef;

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
  stream: RwLock<Option<LobbyStream>>,
}

impl LobbyState {
  pub fn new(domain: &str) -> Self {
    LobbyState {
      domain: domain.to_string(),
      stream: RwLock::new(None),
    }
  }

  pub async fn connect(&self, ws_sender: WsSenderRef, token: String) -> Result<()> {
    let stream = LobbyStream::connect(&self.domain, ws_sender, token).await?;

    {
      *self.stream.write() = Some(stream)
    }

    Ok(())
  }
}
