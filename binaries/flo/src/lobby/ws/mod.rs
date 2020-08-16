mod handler;
pub mod message;
mod stream;

use futures::stream::TryStreamExt;
use http::{Request, Response};
use parking_lot::RwLock;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_net::packet::Frame;

use crate::error::*;
use crate::platform::PlatformStateRef;

use handler::WsHandlerRef;
use message::OutgoingMessage;
use stream::WsStream;

pub type WsMessageSender = Sender<OutgoingMessage>;
pub type WsEventSender = Sender<WsEvent>;

#[derive(Debug)]
pub struct Ws {
  dropper: WsDrop,
  sender: WsMessageSender,
  state: Arc<State>,
}

impl Ws {
  pub async fn init(platform: PlatformStateRef, event_sender: WsEventSender) -> Result<Self> {
    let port = platform.with_config(|c| c.local_port);
    let dropper = Arc::new(Notify::new());
    let (ws_sender, mut ws_receiver) = channel(3);
    let state = Arc::new(State {
      port,
      platform,
      event_sender: event_sender.clone(),
      handler: Arc::new(RwLock::new(None)),
      dropper: dropper.clone(),
    });
    tokio::spawn(
      {
        let state = state.clone();
        let mut event_sender = event_sender.clone();
        async move {
          if let Err(err) = state.serve().await {
            tracing::error!("serve: {}", err);
            event_sender.send(WsEvent::WorkerErrorEvent(err)).await.ok();
          }
        }
      }
      .instrument(tracing::debug_span!("worker")),
    );

    // forward ws messages to current handler
    tokio::spawn(
      {
        let state = state.clone();
        async move {
          loop {
            match ws_receiver.recv().await {
              Some(msg) => {
                let handler = { state.handler.read().as_ref().cloned() };
                if let Some(handler) = handler {
                  handler.send_or_discard(msg).await;
                }
              }
              None => break,
            }
          }
          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("ws_sender_worker")),
    );

    Ok(Self {
      dropper: WsDrop(dropper),
      sender: ws_sender,
      state,
    })
  }

  /// Send a websocket message
  /// messages will be discarded if client is not connected
  pub async fn send_or_discard(&self, msg: OutgoingMessage) {
    self.sender.clone().send(msg).await.ok();
  }

  pub fn disconnect_current<T: ToString>(&self, reason: message::DisconnectReason, message: T) {
    self.state.disconnect_current(reason, message)
  }
}

#[derive(Debug)]
struct WsDrop(Arc<Notify>);

impl Drop for WsDrop {
  fn drop(&mut self) {
    self.0.notify()
  }
}

#[derive(Debug)]
struct State {
  port: u16,
  platform: PlatformStateRef,
  event_sender: WsEventSender,
  handler: Arc<RwLock<Option<WsHandlerRef>>>,
  dropper: Arc<Notify>,
}

impl State {
  pub async fn serve(self: Arc<Self>) -> Result<()> {
    use async_tungstenite::tokio::accept_hdr_async;
    use async_tungstenite::tungstenite::Error as WsError;
    use tokio::net::TcpListener;

    let port = self.port;
    let mut listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;

    tracing::debug!("listen on {}", listener.local_addr()?);

    let mut incoming = listener.incoming();

    loop {
      let stream = tokio::select! {
        _ = self.dropper.notified() => {
          return Ok(())
        }
        stream = incoming.try_next() => {
          if let Some(stream) = stream? {
            stream
          } else {
            tracing::debug!("stream closed");
            return Ok(())
          }
        }
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
      {
        self.disconnect_current(
          message::DisconnectReason::Multi,
          "Another browser window took up the connection.",
        );
        *self.handler.write() = Some(WsHandlerRef::new(
          self.platform.clone(),
          self.event_sender.clone(),
          WsStream::new(stream),
        ));
      }
    }
  }

  pub fn disconnect_current<T: ToString>(&self, reason: message::DisconnectReason, message: T) {
    let handler = self.handler.write().take();
    if let Some(handler) = handler {
      let message = message.to_string();
      tokio::spawn(async move {
        handler
          .send_or_discard(message::OutgoingMessage::Disconnect(message::Disconnect {
            reason,
            message,
          }))
          .await;
      });
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
  if !flo_constants::CONNECT_ORIGINS.contains(&origin) {
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
  ConnectLobbyEvent(ConnectLobbyEvent),
  LobbyFrameEvent(Frame),
  WorkerErrorEvent(Error),
}

#[derive(Debug)]
pub struct ConnectLobbyEvent {
  pub token: String,
}

impl From<tokio::sync::mpsc::error::SendError<WsEvent>> for Error {
  fn from(_: tokio::sync::mpsc::error::SendError<WsEvent>) -> Error {
    tracing::debug!("WsEvent dropped");
    Error::TaskCancelled
  }
}
