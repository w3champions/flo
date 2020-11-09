pub mod message;
mod session;
mod stream;

use crate::controller::{ControllerClient, ReplaceWsSession};
use crate::error::*;
use crate::platform::{GetClientConfig, PlatformActor};
use flo_state::{async_trait, Actor, Addr, Context, Message, RegistryRef, Service};
use futures::stream::TryStreamExt;
use http::{Request, Response};
pub use session::WsSession;
use std::net::{Ipv4Addr, SocketAddrV4};
use stream::WsStream;
use tracing_futures::Instrument;

pub struct WsListener {
  platform: Addr<PlatformActor>,
  client: Addr<ControllerClient>,
}

#[async_trait]
impl Actor for WsListener {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    let worker = Worker {
      platform: self.platform.clone(),
      client: self.client.clone(),
    };
    ctx.spawn(
      {
        async move {
          if let Err(err) = worker.serve().await {
            tracing::error!("serve: {}", err);
            worker.client.notify(WsEvent::WorkerError(err)).await.ok();
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );
  }
}

#[async_trait]
impl Service for WsListener {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<()>) -> Result<Self, Self::Error> {
    let platform = registry.resolve().await?;
    let client = registry.resolve().await?;
    Ok(WsListener { platform, client })
  }
}

struct Worker {
  platform: Addr<PlatformActor>,
  client: Addr<ControllerClient>,
}

impl Worker {
  pub async fn serve(&self) -> Result<()> {
    use async_tungstenite::tokio::accept_hdr_async;
    use async_tungstenite::tungstenite::Error as WsError;
    use tokio::net::TcpListener;

    let port = self.platform.send(GetClientConfig).await?.local_port;
    let mut listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;

    tracing::debug!("listen on {}", listener.local_addr()?);

    let mut incoming = listener.incoming();

    loop {
      let stream = if let Some(stream) = incoming.try_next().await? {
        stream
      } else {
        tracing::debug!("stream closed");
        return Ok(());
      };

      let _addr = stream.peer_addr()?;
      let stream = match accept_hdr_async(stream, check_origin).await {
        Ok(stream) => stream,
        Err(WsError::Http(_)) => continue,
        Err(e) => {
          tracing::error!("{}", e);
          return Err(e.into());
        }
      };

      let session = WsSession::new(
        self.platform.clone(),
        self.client.clone(),
        WsStream::new(stream),
      );

      self.client.notify(ReplaceWsSession(session)).await?;
    }
  }
}

fn check_origin(
  req: &Request<()>,
  res: Response<()>,
) -> Result<Response<()>, Response<Option<String>>> {
  use http::StatusCode;
  let origin = req.headers().get(http::header::ORIGIN).ok_or_else(|| {
    Response::builder()
      .status(StatusCode::BAD_REQUEST)
      .body(None)
      .unwrap()
  })?;
  let origin = origin.to_str().map_err(|_| {
    Response::builder()
      .status(StatusCode::BAD_REQUEST)
      .body(None)
      .unwrap()
  })?;
  if !flo_constants::CLIENT_ORIGINS.contains(&origin) {
    return Err(
      Response::builder()
        .status(StatusCode::FORBIDDEN)
        .body(None)
        .unwrap(),
    );
  }
  Ok(res)
}

#[derive(Debug)]
pub enum WsEvent {
  ConnectController(ConnectController),
  WorkerError(Error),
}

impl Message for WsEvent {
  type Result = ();
}

#[derive(Debug)]
pub struct ConnectController {
  pub token: String,
}

impl Message for ConnectController {
  type Result = ();
}

impl From<tokio::sync::mpsc::error::SendError<WsEvent>> for Error {
  fn from(_: tokio::sync::mpsc::error::SendError<WsEvent>) -> Error {
    Error::TaskCancelled(anyhow::format_err!("WsEvent dropped"))
  }
}
