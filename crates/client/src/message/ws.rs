use super::message::{IncomingMessage, OutgoingMessage};
use super::stream::MessageStream;
use super::Session;
use crate::controller::{ControllerClient, ReplaceSession};
use crate::error::{Error, Result};
use crate::message::MessageEvent;
use crate::observer::{ObserverClient, WatchGame};
use crate::platform::Platform;
use crate::StartConfig;
use async_tungstenite::tokio::accept_hdr_async;
use async_tungstenite::tungstenite::Error as WsError;
use async_tungstenite::tungstenite::Message as WsMessage;
use async_tungstenite::WebSocketStream;
use flo_state::{
  async_trait, Actor, Addr, Context, Handler, Message, Registry, RegistryRef, Service,
};
use futures::{SinkExt, StreamExt};
use http::{Request, Response};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tracing_futures::Instrument;

pub struct FloWsClient {
  _registry: Registry<StartConfig>,
  port: u16,
}

impl FloWsClient {
  pub fn port(&self) -> u16 {
    self.port
  }

  pub async fn start_test_game(&self) -> Result<()> {
    use crate::platform::StartTestGame;
    let platform = self._registry.resolve::<Platform>().await?;

    platform
      .send(StartTestGame {
        name: "TEST".to_string(),
      })
      .await??;

    Ok(())
  }

  pub async fn watch(&self, token: String) -> Result<()> {
    let obs = self._registry.resolve::<ObserverClient>().await?;

    obs.send(WatchGame { token }).await??;

    Ok(())
  }

  pub async fn serve(self) {
    futures::future::pending().await
  }
}

pub async fn start_ws(config: StartConfig) -> Result<FloWsClient> {
  tracing::info!("version: {}", crate::version::FLO_VERSION);

  let registry = Registry::with_data(config);
  let listener = registry.resolve::<WsListener>().await?;

  Ok(FloWsClient {
    port: listener.send(GetPort).await?,
    _registry: registry,
  })
}

pub struct WsListener {
  platform: Addr<Platform>,
  controller_client: Addr<ControllerClient>,
  observer_client: Addr<ObserverClient>,
  listener: Option<WsMessageListener>,
  port: u16,
}

#[async_trait]
impl Actor for WsListener {
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
impl Service<StartConfig> for WsListener {
  type Error = Error;

  async fn create(registry: &mut RegistryRef<StartConfig>) -> Result<Self, Self::Error> {
    let platform = registry.resolve().await?;
    let controller_client = registry.resolve().await?;
    let observer_client = registry.resolve().await?;

    let listener = WsMessageListener::bind(registry).await?;
    let port = listener.port();

    Ok(WsListener {
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
  pub async fn serve(&self, mut listener: WsMessageListener) -> Result<()> {
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
        Box::new(stream),
      );

      self
        .controller_client
        .notify(ReplaceSession(session))
        .await?;
    }
  }
}

#[derive(Debug)]
pub struct GetPort;

impl Message for GetPort {
  type Result = u16;
}

#[async_trait]
impl Handler<GetPort> for WsListener {
  async fn handle(&mut self, _: &mut Context<Self>, _: GetPort) -> <GetPort as Message>::Result {
    self.port
  }
}

type InnerStream = WebSocketStream<async_tungstenite::tokio::TokioAdapter<TcpStream>>;

pub struct WsMessageListener {
  transport: TcpListener,
  port: u16,
}

impl WsMessageListener {
  pub async fn bind(_registry: &mut RegistryRef<StartConfig>) -> Result<Self> {
    #[cfg(feature = "worker")]
    let port = None;

    #[cfg(not(feature = "worker"))]
    let port = {
      use crate::platform::{GetClientConfig, Platform};
      let platform = _registry.resolve::<Platform>().await?;
      Some(platform.send(GetClientConfig).await?.local_port)
    };

    let transport =
      TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port.unwrap_or(0))).await?;
    let port = transport.local_addr()?.port();

    tracing::debug!("listen on {}", port);

    Ok(WsMessageListener { transport, port })
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

impl WsMessageListener {
  pub async fn accept(&mut self) -> Result<Option<WsMessageStream>> {
    loop {
      let (stream, _) = self.transport.accept().await?;
      match accept_hdr_async(stream, check_origin).await {
        Ok(stream) => return Ok(Some(WsMessageStream(stream))),
        Err(WsError::Http(_)) => continue,
        Err(e) => {
          tracing::error!("ws accept: {}", e);
          return Err(e.into());
        }
      };
    }
  }
}

pub struct WsMessageStream(InnerStream);

#[async_trait]
impl MessageStream for WsMessageStream {
  async fn send(&mut self, msg: OutgoingMessage) -> Result<()> {
    let msg = msg.serialize()?;
    match self.0.send(WsMessage::Text(msg)).await {
      Ok(_) => Ok(()),
      Err(err) => Err(err.into()),
    }
  }

  async fn recv(&mut self) -> Option<IncomingMessage> {
    loop {
      let msg = match self.0.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(err)) => {
          tracing::debug!("ws recv error: {}", err);
          break None;
        }
        None => break None,
      };

      match msg {
        WsMessage::Text(text) => match serde_json::from_str::<IncomingMessage>(&text) {
          Ok(msg) => break Some(msg),
          Err(err) => {
            tracing::error!("malformed websocket message: {}, text: {}", err, text);
            break None;
          }
        },
        WsMessage::Binary(data) => match serde_json::from_slice::<IncomingMessage>(&data) {
          Ok(msg) => break Some(msg),
          Err(err) => {
            tracing::error!("malformed websocket message: {}", err);
            break None;
          }
        },
        WsMessage::Ping(_data) => continue,
        WsMessage::Pong(_data) => continue,
        WsMessage::Close(_frame) => break None,
      };
    }
  }

  async fn flush(&mut self) {
    tokio::time::timeout(Duration::from_secs(3), self.0.flush())
      .await
      .ok()
      .take();
  }
}

#[cfg(feature = "worker")]
fn check_origin(
  _req: &Request<()>,
  res: Response<()>,
) -> Result<Response<()>, Response<Option<String>>> {
  Ok(res)
}

#[cfg(not(feature = "worker"))]
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
