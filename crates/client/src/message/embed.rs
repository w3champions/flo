use super::messages::{IncomingMessage, OutgoingMessage};
use super::stream::MessageStream;
use super::Session;
use crate::controller::{ControllerClient, ReplaceSession};
use crate::error::{Error, Result};
use crate::observer::{ObserverClient, WatchGame};
use crate::platform::Platform;
use crate::StartConfig;
pub use flo_platform::ClientPlatformInfo;
use flo_state::{async_trait, Addr, Registry};
use tokio::sync::mpsc;

pub struct FloEmbedClient {
  platform: Addr<Platform>,
  controller_client: Addr<ControllerClient>,
  observer_client: Addr<ObserverClient>,
  tx: mpsc::Sender<IncomingMessage>,
  rx: mpsc::Receiver<OutgoingMessage>,
  _registry: Registry<StartConfig>,
}

impl FloEmbedClient {
  pub fn handle(&self) -> FloEmbedClientHandle {
    FloEmbedClientHandle {
      tx: self.tx.clone(),
      platform: self.platform.clone(),
      _controller_client: self.controller_client.clone(),
      observer_client: self.observer_client.clone(),
    }
  }

  pub async fn recv(&mut self) -> Option<OutgoingMessage> {
    self.rx.recv().await
  }
}

#[derive(Clone)]
pub struct FloEmbedClientHandle {
  tx: mpsc::Sender<IncomingMessage>,
  platform: Addr<Platform>,
  _controller_client: Addr<ControllerClient>,
  observer_client: Addr<ObserverClient>,
}

impl FloEmbedClientHandle {
  pub async fn send(&self, msg: IncomingMessage) -> Result<()> {
    self
      .tx
      .send(msg)
      .await
      .map_err(|_| Error::EmbedMessageStreamBroken)?;
    Ok(())
  }

  pub async fn start_test_game(&self) -> Result<()> {
    use crate::platform::StartTestGame;
    self
      .platform
      .send(StartTestGame {
        name: "TEST".to_string(),
      })
      .await??;

    Ok(())
  }

  pub async fn watch(&self, token: String) -> Result<()> {
    self.observer_client.send(WatchGame { token }).await??;
    Ok(())
  }

  pub async fn get_client_platform_info(&self, force_reload: bool) -> Result<ClientPlatformInfo> {
    use crate::platform::GetClientPlatformInfo;
    let info = self
      .platform
      .send(GetClientPlatformInfo { force_reload })
      .await??;
    Ok(info)
  }
}

pub async fn start_embed(config: StartConfig) -> Result<FloEmbedClient> {
  tracing::info!("version: {}", crate::version::FLO_VERSION);

  let registry = Registry::with_data(config);
  let platform = registry.resolve().await?;
  let controller_client = registry.resolve().await?;
  let observer_client = registry.resolve().await?;

  let (outgoing_tx, outgoing_rx) = mpsc::channel(100);
  let (incoming_tx, incoming_rx) = mpsc::channel(100);

  let stream = EmbedMessageStream {
    tx: outgoing_tx,
    rx: incoming_rx,
  };

  let session = Session::new(
    platform.clone(),
    controller_client.clone(),
    observer_client.clone(),
    Box::new(stream),
  );

  controller_client.notify(ReplaceSession(session)).await?;

  Ok(FloEmbedClient {
    platform,
    controller_client,
    observer_client,
    tx: incoming_tx,
    rx: outgoing_rx,
    _registry: registry,
  })
}

struct EmbedMessageStream {
  tx: mpsc::Sender<OutgoingMessage>,
  rx: mpsc::Receiver<IncomingMessage>,
}

#[async_trait]
impl MessageStream for EmbedMessageStream {
  async fn send(&mut self, msg: OutgoingMessage) -> Result<()> {
    self
      .tx
      .send(msg)
      .await
      .map_err(|_| Error::EmbedMessageStreamBroken)
  }

  async fn recv(&mut self) -> Option<IncomingMessage> {
    self.rx.recv().await
  }

  async fn flush(&mut self) {}
}
