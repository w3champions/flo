use super::listener::{MessageListener, MessageStream};
use super::message::{IncomingMessage, OutgoingMessage};
use crate::error::Result;
use async_tungstenite::tokio::accept_hdr_async;
use async_tungstenite::tungstenite::Error as WsError;
use async_tungstenite::tungstenite::Message as WsMessage;
use async_tungstenite::WebSocketStream;
use flo_state::async_trait;
use futures::{SinkExt, StreamExt};
use http::{Request, Response};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

type InnerStream = WebSocketStream<async_tungstenite::tokio::TokioAdapter<TcpStream>>;

pub struct WsListener {
  transport: TcpListener,
}

impl WsListener {
  pub async fn bind(port: Option<u16>) -> Result<Self> {
    let transport =
      TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port.unwrap_or(0))).await?;
    Ok(WsListener { transport })
  }
}

#[async_trait]
impl MessageListener for WsListener {
  async fn accept(&mut self) -> Result<Option<Box<dyn MessageStream>>> {
    loop {
      let (stream, _) = self.transport.accept().await?;
      match accept_hdr_async(stream, check_origin).await {
        Ok(stream) => return Ok(Some(Box::new(WsStream(stream)))),
        Err(WsError::Http(_)) => continue,
        Err(e) => {
          tracing::error!("ws accept: {}", e);
          return Err(e.into());
        }
      };
    }
  }
}

pub struct WsStream(InnerStream);

#[async_trait]
impl MessageStream for WsStream {
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
