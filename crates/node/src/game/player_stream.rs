use flo_net::packet::Frame;
use flo_net::stream::FloStream;

use crate::error::*;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{error::TrySendError, Sender};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct PlayerStream {
  id: u64,
  player_id: i32,
  stream: FloStream,
  ct: CancellationToken,
}

impl PlayerStream {
  pub fn new(player_id: i32, stream: FloStream) -> Self {
    static ID_GEN: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::from(0));

    let stream = Self {
      id: ID_GEN.fetch_add(1, Ordering::Relaxed),
      player_id,
      stream,
      ct: CancellationToken::new(),
    };
    stream
  }

  pub fn id(&self) -> u64 {
    self.id
  }

  pub fn player_id(&self) -> i32 {
    self.player_id
  }

  pub fn get_mut(&mut self) -> &mut FloStream {
    &mut self.stream
  }

  pub fn token(&self) -> CancellationToken {
    self.ct.clone()
  }
}

impl Into<FloStream> for PlayerStream {
  fn into(self) -> FloStream {
    self.stream
  }
}

pub enum PlayerStreamCmd {
  Send(Frame),
  SetDelay(Option<Duration>),
}

#[derive(Debug, Clone)]
pub struct PlayerStreamHandle {
  stream_id: u64,
  tx: Sender<PlayerStreamCmd>,
  ct: CancellationToken,
}

impl PlayerStreamHandle {
  pub fn new(stream: &PlayerStream, tx: Sender<PlayerStreamCmd>) -> Self {
    Self {
      stream_id: stream.id(),
      tx,
      ct: stream.ct.clone(),
    }
  }

  pub fn stream_id(&self) -> u64 {
    self.stream_id
  }

  pub fn close(&self) {
    self.ct.cancel();
  }

  pub async fn send(&self, frame: Frame) -> Result<()> {
    self
      .tx
      .send(PlayerStreamCmd::Send(frame))
      .await
      .map_err(|_| Error::Cancelled)
  }

  pub fn set_delay(&self, value: Option<Duration>) -> Result<()> {
    self
      .tx
      .try_send(PlayerStreamCmd::SetDelay(value))
      .map_err(|_| Error::Cancelled)
  }

  pub fn try_send(&self, frame: Frame) -> Result<(), TrySendError<Frame>> {
    self
      .tx
      .try_send(PlayerStreamCmd::Send(frame))
      .map_err(|err| match err {
        TrySendError::Full(PlayerStreamCmd::Send(frame)) => TrySendError::Full(frame),
        TrySendError::Closed(PlayerStreamCmd::Send(frame)) => TrySendError::Closed(frame),
        _ => unreachable!(),
      })
  }
}
