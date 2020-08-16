use async_tungstenite::tungstenite::{error::Error as WsError, Message as WsMessage};
use async_tungstenite::WebSocketStream;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use super::message::{IncomingMessage, OutgoingMessage};
use crate::error::Result;

type InnerStream = WebSocketStream<async_tungstenite::tokio::TokioAdapter<TcpStream>>;

#[derive(Debug)]
pub struct WsStream(InnerStream);

impl WsStream {
  pub fn new(stream: InnerStream) -> Self {
    Self(stream)
  }

  pub async fn send(&mut self, msg: OutgoingMessage) -> Result<()> {
    let msg = msg.serialize()?;
    match self.0.send(WsMessage::Text(msg)).await {
      Ok(_) => Ok(()),
      Err(err) => Err(err.into()),
    }
  }

  pub async fn recv(&mut self) -> Option<IncomingMessage> {
    loop {
      let msg = match self.0.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(err)) => {
          tracing::debug!("ws recv error: {}", err);
          break None;
        }
        None => break None,
      };

      let msg = match msg {
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

  pub async fn flush(&mut self) {
    tokio::time::timeout(Duration::from_secs(3), self.0.flush())
      .await
      .ok()
      .take();
  }
}
