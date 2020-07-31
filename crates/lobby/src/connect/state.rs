use futures::future::FutureExt;
use futures::future::{abortable, AbortHandle};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio::time::timeout;
use tonic::Status;
use tracing_futures::Instrument;

use flo_net::packet::*;
use flo_net::proto::flo_connect::*;

use crate::connect::send_buf::PlayerSendBuf;
use crate::error::{Error, Result};
use crate::game::Game;
use crate::state::MemStorageRef;

const SEND_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Message {
  Frame(Frame),
  Frames(Vec<Frame>),
  Broken,
}

type Tx = mpsc::Sender<Message>;
pub type PlayerReceiver = mpsc::Receiver<Message>;

#[derive(Debug, Clone)]
pub struct PlayerSenderRef {
  player_id: i32,
  tx: Tx,
  state: Arc<State>,
  abort: Arc<Abort>,
}

#[derive(Debug)]
struct State {
  buf: Mutex<PlayerSendBuf>,
  flush_notify: Notify,
}

#[derive(Debug)]
struct Abort {
  handle: AbortHandle,
}

impl Drop for Abort {
  fn drop(&mut self) {
    self.handle.abort();
  }
}

impl PlayerSenderRef {
  pub fn new(player_id: i32) -> (Self, PlayerReceiver) {
    let (tx, rx) = mpsc::channel(5);
    let buf = Mutex::new(PlayerSendBuf::new(player_id, None));
    let flush_notify = Notify::new();
    let state = Arc::new(State { buf, flush_notify });
    let worker = {
      let mut tx = tx.clone();
      let state = state.clone();
      async move {
        loop {
          state.flush_notify.notified().await;
          tracing::debug!("worker activated");

          let frames = { state.buf.lock().take_frames() };

          let frames = {
            match frames {
              Ok(frames) => frames,
              Err(e) => {
                tracing::error!("take frames: {}", e);
                tx.send(Message::Broken).await.ok();
                break;
              }
            }
          };
          if let Some(frames) = frames {
            match tx.send(Message::Frames(frames)).await {
              Ok(_) => {}
              Err(e) => {
                tracing::debug!("rx closed");
                break;
              }
            }
          }
        }
      }
    };

    let (f, handle) = abortable(worker);
    tokio::spawn(
      f.map(|res| match res {
        Ok(_) => tracing::debug!("exited"),
        Err(_) => tracing::debug!("aborted"),
      })
      .instrument(tracing::debug_span!("worker", player_id)),
    );

    let abort = Arc::new(Abort { handle });

    (
      PlayerSenderRef {
        player_id,
        tx,
        state,
        abort,
      },
      rx,
    )
  }

  pub fn player_id(&self) -> i32 {
    self.player_id
  }

  pub fn with_buf<F>(&mut self, f: F)
  where
    F: FnOnce(&mut PlayerSendBuf),
  {
    let mut lock = self.state.buf.lock();
    f(&mut lock);
    self.state.flush_notify.notify();
  }

  pub async fn disconnect_multi(&mut self) {
    self.disconnect(LobbyDisconnectReason::Multi).await.ok();
  }

  #[tracing::instrument]
  async fn disconnect(&mut self, reason: LobbyDisconnectReason) -> Result<()> {
    timeout(
      SEND_TIMEOUT,
      self.tx.send(Message::Frame(
        PacketLobbyDisconnect {
          reason: reason.into(),
        }
        .encode_as_frame()?,
      )),
    )
    .await
    .map_timeout_err(
      || {
        tracing::debug!("channel timeout");
      },
      |_| {
        tracing::debug!("channel broken");
      },
    )
    .ok();
    Ok(())
  }

  pub fn ptr_eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.state, &other.state)
  }
}

#[derive(Debug)]
pub struct ConnectState {
  pub player_id: i32,
  pub joined_game: Option<Game>,
}
