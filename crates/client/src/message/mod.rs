pub mod message;
mod session;

#[cfg(feature = "ws")]
mod ws;
#[cfg(feature = "ws")]
pub(crate) use ws::{WsMessageListener as MessageListener, WsMessageStream as MessageStream};

use crate::controller::{ControllerClient, ReplaceSession};
use crate::error::*;
use crate::observer::ObserverClient;
use crate::platform::Platform;
use crate::StartConfig;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message, RegistryRef, Service};
pub use session::Session;
use tracing_futures::Instrument;

pub struct Listener {
  platform: Addr<Platform>,
  controller_client: Addr<ControllerClient>,
  observer_client: Addr<ObserverClient>,
  listener: Option<MessageListener>,
  port: u16,
}

#[async_trait]
impl Actor for Listener {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let listener = if let Some(listener) = self.listener.take() {
      listener
    } else {
      return;
    };

    let worker = Worker {
      platform: self.platform.clone(),
      controller_client: self.controller_client.clone(),
      observer_client: self.observer_client.clone(),
    };
    ctx.spawn(
      {
        async move {
          if let Err(err) = worker.serve(listener).await {
            tracing::error!("serve: {}", err);
            worker
              .controller_client
              .notify(MessageEvent::WorkerError(err))
              .await
              .ok();
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
  }
}

#[async_trait]
impl Service<StartConfig> for Listener {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve().await?;
    let controller_client = registry.resolve().await?;
    let observer_client = registry.resolve().await?;

    let listener = MessageListener::bind(registry).await?;
    let port = listener.port();

    Ok(Listener {
      platform,
      controller_client,
      observer_client,
      listener: listener.into(),
      port,
    })
  }
}

struct Worker {
  platform: Addr<Platform>,
  controller_client: Addr<ControllerClient>,
  observer_client: Addr<ObserverClient>,
}

impl Worker {
  pub async fn serve(&self, mut listener: MessageListener) -> Result<()> {
    loop {
      let stream = if let Some(stream) = listener.accept().await? {
        stream
      } else {
        tracing::debug!("stream closed");
        return Ok(());
      };

      let session = Session::new(
        self.platform.clone(), 
        self.controller_client.clone(), 
        self.observer_client.clone(), 
        stream
      );

      self.controller_client.notify(ReplaceSession(session)).await?;
    }
  }
}

#[derive(Debug)]
pub enum MessageEvent {
  ConnectController(ConnectController),
  WorkerError(Error),
}

impl Message for MessageEvent {
  type Result = ();
}

#[derive(Debug)]
pub struct ConnectController {
  pub token: String,
}

impl Message for ConnectController {
  type Result = ();
}

impl From<tokio::sync::mpsc::error::SendError<MessageEvent>> for Error {
  fn from(_: tokio::sync::mpsc::error::SendError<MessageEvent>) -> Error {
    Error::TaskCancelled(anyhow::format_err!("WsEvent dropped"))
  }
}

#[derive(Debug)]
pub struct GetPort;

impl Message for GetPort {
  type Result = u16;
}

#[async_trait]
impl Handler<GetPort> for Listener {
  async fn handle(&mut self, _: &mut Context<Self>, _: GetPort) -> <GetPort as Message>::Result {
    self.port
  }
}
