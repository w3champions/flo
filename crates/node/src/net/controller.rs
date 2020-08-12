use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_constants::NODE_SOCKET_PORT;
use flo_net::listener::FloListener;
use flo_net::match_packet;
use flo_net::packet::{FloPacket, Frame};
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;

use crate::error::*;
use crate::session::SessionStore;

#[derive(Debug)]
pub struct ControllerHandler {
  current: Option<ControllerConn>,
}

impl ControllerHandler {
  pub fn new() -> Self {
    Self { current: None }
  }

  pub async fn serve(&mut self) -> Result<()> {
    let mut listener = FloListener::bind_v4(NODE_SOCKET_PORT).await?;

    while let Some(incoming) = listener.incoming().next().await {
      if let Ok(stream) = incoming {
        if let Ok(conn) = handshake(stream).await {
          self.current = Some(conn)
        }
      }
    }

    Ok(())
  }
}

async fn handshake(mut stream: FloStream) -> Result<ControllerConn> {
  use std::time::Duration;
  const RECV_TIMEOUT: Duration = Duration::from_secs(3);

  let connect: PacketControllerConnect = stream.recv_timeout(RECV_TIMEOUT).await?;

  if connect.secret != crate::env::Env::get().secret_key {
    stream
      .send(PacketControllerConnectReject {
        reason: ControllerConnectRejectReason::InvalidSecretKey.into(),
      })
      .await?;
    return Err(Error::InvalidSecret);
  }

  stream
    .send(PacketControllerConnectAccept {
      version: Some(crate::version::FLO_NODE_VERSION.into()),
    })
    .await?;

  Ok(ControllerConn::new(stream))
}

#[derive(Debug)]
struct ControllerConn {
  dropper: Arc<Notify>,
}

impl ControllerConn {
  fn new(stream: FloStream) -> Self {
    let dropper = Arc::new(Notify::new());

    tokio::spawn({
      let dropper = dropper.clone();
      async move {
        if let Err(e) = handle_stream(dropper, stream).await {
          tracing::debug!("handle_stream: {}", e);
        }
        tracing::debug!("exiting")
      }
      .instrument(tracing::debug_span!("worker"))
    });

    ControllerConn { dropper }
  }
}

impl Drop for ControllerConn {
  fn drop(&mut self) {
    tracing::debug!("controller connection drop.");
    self.dropper.notify();
  }
}

async fn handle_stream(dropper: Arc<Notify>, mut stream: FloStream) -> Result<()> {
  use futures::TryStreamExt;
  let (tx, mut rx) = mpsc::channel(5);

  loop {
    tokio::select! {
      _ = dropper.notified() => {
        break;
      }
      frame = stream.recv_frame() => {
        let frame = frame?;
        let sender = tx.clone();
        tokio::spawn(async move {
          if let Err(e) = handle_frame(sender, frame).await {
            tracing::error!("handle_frame: {}", e);
          }
        }.instrument(tracing::debug_span!("handle_frame_worker")));
      }
      frame = rx.recv() => {
        let frame = frame.expect("sender reference hold on stack");
        stream.send_frame(frame).await?;
      }
    }
  }
  Ok(())
}

async fn handle_frame(mut tx: mpsc::Sender<Frame>, frame: Frame) -> Result<()> {
  match_packet! {
    frame => {
      pkt = PacketControllerCreateGame => {
        match SessionStore::get().handle_controller_create_game(pkt) {
          Ok(frame) => {
            if let Err(e) = tx.send(frame).await {
              tracing::warn!("connection dropped")
            }
          },
          Err(e) => {
            tracing::error!("create game error: {:?}", e);
          }
        }
      }
    }
  }
  Ok(())
}
