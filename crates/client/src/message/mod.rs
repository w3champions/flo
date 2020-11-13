mod listener;
pub mod message;
mod session;
mod ws;

use crate::controller::{ControllerClient, ReplaceSession};
use crate::error::*;
use crate::platform::{GetClientConfig, PlatformActor};
use crate::StartConfig;
use flo_state::{async_trait, Actor, Addr, Context, Handler, Message, RegistryRef, Service};
pub use listener::{MessageListener, MessageStream};
pub use session::Session;
use std::net::{Ipv4Addr, SocketAddrV4};
use tokio::net::TcpListener;
use tracing_futures::Instrument;

pub struct Listener {
  platform: Addr<PlatformActor>,
  client: Addr<ControllerClient>,
  listener: Option<Box<dyn MessageListener>>,
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
      client: self.client.clone(),
    };
    ctx.spawn(
      {
        async move {
          if let Err(err) = worker.serve(listener).await {
            tracing::error!("serve: {}", err);
            worker
              .client
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
    let client = registry.resolve().await?;

    let port = if registry.data().dynamic_port {
      0
    } else {
      platform.send(GetClientConfig).await?.local_port
    };

    let listener = if let Some(v) = registry {};

    let sock_addr = listener.local_addr()?;
    tracing::debug!("listen on {}", sock_addr);

    Ok(Listener {
      platform,
      client,
      listener: listener.into(),
      port: sock_addr.port(),
    })
  }
}

struct Worker {
  platform: Addr<PlatformActor>,
  client: Addr<ControllerClient>,
}

impl Worker {
  pub async fn serve(&self, mut listener: Box<dyn MessageListener>) -> Result<()> {
    loop {
      let stream = if let Some(stream) = listener.accept().await? {
        stream
      } else {
        tracing::debug!("stream closed");
        return Ok(());
      };

      let session = Session::new(self.platform.clone(), self.client.clone(), stream);

      self.client.notify(ReplaceSession(session)).await?;
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
