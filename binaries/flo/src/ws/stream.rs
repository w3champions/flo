use async_tungstenite::tungstenite::{error::Error as WsError, Message as WsMessage};
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream, Stream, StreamExt};
use futures::SinkExt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::ws::message::{IncomingMessage, OutgoingMessage};

pub type WsStream = WebSocketStream<async_tungstenite::tokio::TokioAdapter<TcpStream>>;

#[derive(Debug)]
pub struct WsReceiver(SplitStream<WsStream>);

impl Deref for WsReceiver {
  type Target = SplitStream<WsStream>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for WsReceiver {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone)]
pub struct WsSenderRef(Arc<Mutex<SplitSink<WsStream, WsMessage>>>);

impl WsSenderRef {
  fn new(sink: SplitSink<WsStream, WsMessage>) -> Self {
    WsSenderRef(Arc::new(Mutex::new(sink)))
  }

  pub async fn send(&self, msg: OutgoingMessage) -> Result<()> {
    let msg = msg.serialize()?;
    let mut sink = self.0.lock().await;
    match sink.send(WsMessage::Text(msg)).await {
      Ok(_) => Ok(()),
      Err(e) => match e {
        WsError::ConnectionClosed | WsError::AlreadyClosed => Ok(()),
        e => Err(e.into()),
      },
    }
  }
}

pub trait WsStreamExt {
  fn split(self) -> (WsSenderRef, WsReceiver);
}

impl WsStreamExt for WsStream {
  fn split(self) -> (WsSenderRef, WsReceiver) {
    let (sink, stream) = StreamExt::split(self);
    (WsSenderRef::new(sink), WsReceiver(stream))
  }
}
