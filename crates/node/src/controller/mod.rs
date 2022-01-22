use futures::lock::Mutex;
use futures::stream::StreamExt;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing_futures::Instrument;

use flo_constants::NODE_CONTROLLER_PORT;
use flo_net::listener::FloListener;
use flo_net::packet::Frame;
use flo_net::proto::flo_node::*;
use flo_net::stream::FloStream;
use flo_net::try_flo_packet;
use flo_task::{SpawnScope, SpawnScopeHandle};

use crate::error::*;
use crate::state::GlobalStateRef;
use flo_net::ping::PingStream;

#[derive(Debug)]
pub struct ControllerServer {
  state: Arc<State>,
}

#[derive(Debug)]
struct State {
  g_state: GlobalStateRef,
  current: RwLock<Option<ControllerConn>>,
  frame_tx: Sender<Frame>,
  frame_rx: Mutex<Receiver<Frame>>,
}

impl ControllerServer {
  pub fn new(g_state: GlobalStateRef) -> ControllerServer {
    let (frame_tx, frame_rx) = channel(crate::constants::CONTROLLER_SENDER_BUF_SIZE);
    let state = Arc::new(State {
      g_state,
      current: RwLock::new(None),
      frame_tx,
      frame_rx: Mutex::new(frame_rx),
    });
    Self { state }
  }

  pub fn handle(&self) -> ControllerServerHandle {
    ControllerServerHandle {
      state: self.state.clone(),
    }
  }

  pub async fn serve(&mut self) -> Result<()> {
    let mut listener = FloListener::bind_v4(NODE_CONTROLLER_PORT).await?;

    while let Some(incoming) = listener.incoming().next().await {
      if let Ok(stream) = incoming {
        if let Ok(conn) = self.handshake(stream).await {
          self.state.current.write().replace(conn);
        }
      }
    }

    Ok(())
  }

  async fn handshake(&self, mut stream: FloStream) -> Result<ControllerConn> {
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

    Ok(ControllerConn::new(self.state.clone(), stream))
  }
}

#[derive(Debug, Clone)]
pub struct ControllerServerHandle {
  state: Arc<State>,
}

impl ControllerServerHandle {
  fn new(state: Arc<State>) -> Self {
    Self { state }
  }

  /// Sends a frame to the controller
  /// If the controller is disconnected and the send buf is full,
  /// block until the connection is restored.
  pub async fn send(&self, frame: Frame) -> Result<(), Frame> {
    self
      .state
      .frame_tx
      .clone()
      .send(frame)
      .await
      .map_err(|err| err.0)
  }
}

#[derive(Debug)]
struct ControllerConn {
  _scope: SpawnScope,
}

impl ControllerConn {
  fn new(state: Arc<State>, stream: FloStream) -> Self {
    let scope = SpawnScope::new();

    tokio::spawn({
      let scope = scope.handle();
      async move {
        if let Err(e) = handle_stream(state, stream, scope).await {
          tracing::debug!("handle_stream: {}", e);
        }
        tracing::debug!("exiting")
      }
      .instrument(tracing::debug_span!("worker"))
    });

    ControllerConn { _scope: scope }
  }
}

async fn handle_stream(
  state: Arc<State>,
  mut stream: FloStream,
  mut scope: SpawnScopeHandle,
) -> Result<()> {
  let mut rx = state.frame_rx.lock().await;
  loop {
    tokio::select! {
      _ = scope.left() => {
        break;
      }
      frame = stream.recv_frame() => {
        let frame = frame?;
        let state = state.clone();
        tokio::spawn(async move {
          if let Err(e) = handle_frame(&state, frame).await {
            tracing::error!("handle_frame: {}", e);
          }
        }.instrument(tracing::debug_span!("handle_frame_worker")));
      }
      next = rx.recv() => {
        if let Some(frame) = next {
          stream.send_frame_timeout(frame).await?;
        } else {
          break;
        }
      }
    }
  }
  Ok(())
}

async fn handle_frame(state: &Arc<State>, mut frame: Frame) -> Result<()> {
  let tx = &state.frame_tx;
  if frame.type_id == PingStream::PING_TYPE_ID {
    frame.type_id = PingStream::PONG_TYPE_ID;
    tx.send(frame).await.ok();
    return Ok(());
  }

  try_flo_packet! {
    frame => {
      pkt: PacketControllerCreateGame => {
        let frame = state.g_state.handle_controller_create_game(ControllerServerHandle::new(state.clone()), pkt)?;
        flo_log::result_ok!("create game", tx.send(frame).await);
      }
      pkt: PacketControllerUpdateSlotStatus => {
        let frame = state.g_state.handle_controller_update_slot_client_status(pkt).await?;
        flo_log::result_ok!("update slot status", tx.send(frame).await);
      }
    }
  }
  Ok(())
}
