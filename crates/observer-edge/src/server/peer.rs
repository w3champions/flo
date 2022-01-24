use crate::broadcast::BroadcastReceiver;
use crate::error::{Error, Result};
use crate::game::stream::{GameStreamDataSnapshot, GameStreamEvent};
use bytes::Bytes;
use flo_net::{
  packet::{Frame, PacketTypeId},
  ping::{PingMsg, PingStream},
  stream::FloStream,
};
use futures::Stream;
use std::{
  collections::VecDeque,
  pin::Pin,
  task::{Context, Poll, Waker},
  time::Duration,
};
use tokio_stream::StreamExt;

const PING_INTERVAL: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(10);

pub struct GameStreamServer {
  game_id: i32,
  snapshot: Option<GameStreamDataSnapshot>,
  rx: BroadcastReceiver<GameStreamEvent>,
}

impl GameStreamServer {
  pub fn new(
    game_id: i32,
    snapshot: GameStreamDataSnapshot,
    rx: BroadcastReceiver<GameStreamEvent>,
  ) -> Self {
    Self {
      game_id,
      snapshot: Some(snapshot),
      rx,
    }
  }

  pub async fn run(mut self, mut transport: FloStream) -> Result<()> {
    let game_id = self.game_id;
    let mut ready_frames = ReadyFrameStream::new();

    if let Some(snapshot) = self.snapshot.take() {
      ready_frames.push_frames(&snapshot.frames);
      if snapshot.ended {
        ready_frames.finish()
      }
    }

    let mut ping = PingStream::interval(PING_INTERVAL, PING_TIMEOUT);
    ping.start();
    loop {
      tokio::select! {
        r = self.rx.recv() => {
          match r {
            Ok(event) => {
              match event {
                GameStreamEvent::Chunk { frames, ended } => {
                  ready_frames.push_frames(&frames);
                  if ended {
                    ready_frames.finish();
                  }
                },
              }
            }
            Err(err) => {
              use tokio::sync::broadcast::error::RecvError;
              if let RecvError::Lagged(n) = err {
                return Err(Error::ObserverPeerLagged(n))
              }
              transport.send_frame(Frame::new_empty(PacketTypeId::ObserverDataEnd)).await?;
              transport.flush().await?;
              break;
            }
          }
        }
        next = ready_frames.next() => {
          if let Some(frame) = next {
            transport.send_frame(frame).await?;
          } else {
            transport.send_frame(Frame::new_empty(PacketTypeId::ObserverDataEnd)).await?;
            transport.flush().await?;
            break;
          }
        }
        r = transport.recv_frame() => {
          match r {
            Ok(frame) => {
              if frame.type_id == PacketTypeId::Pong {
                ping.capture_pong(frame);
              }
            },
            Err(err) => {
              if let flo_net::error::Error::StreamClosed = err {
                break;
              } else {
                tracing::error!(game_id, "stream recv: {}", err);
              }
            }
          }
        }
        Some(next) = ping.next() => {
          match next {
            PingMsg::Ping(frame) => {
              transport.send_frame(frame).await?;
            },
            PingMsg::Timeout => {
              tracing::error!(game_id, "ping timeout");
              break;
            },
          }
        }
      }
    }
    Ok(())
  }
}

struct ReadyFrameStream {
  q: VecDeque<Frame>,
  w: Option<Waker>,
  finished: bool,
}

impl ReadyFrameStream {
  fn new() -> Self {
    Self {
      q: VecDeque::new(),
      w: None,
      finished: false,
    }
  }

  fn push_frames(&mut self, frames: &[Bytes]) {
    if self.finished {
      return;
    }

    for frame in frames {
      self
        .q
        .push_back(Frame::new_bytes(PacketTypeId::ObserverData, frame.clone()));
    }
    if !self.q.is_empty() {
      self.w.take().map(|w| w.wake());
    }
  }

  fn finish(&mut self) {
    self.finished = true;
    if !self.q.is_empty() {
      self.w.take().map(|w| w.wake());
    }
  }
}

impl Stream for ReadyFrameStream {
  type Item = Frame;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if let Some(frame) = self.q.pop_front() {
      return Poll::Ready(Some(frame));
    }

    if self.finished {
      return Poll::Ready(None);
    }

    match self.w {
      Some(ref w) => {
        if !w.will_wake(cx.waker()) {
          self.w.replace(cx.waker().clone());
        }
      }
      None => {
        self.w.replace(cx.waker().clone());
      }
    }

    Poll::Pending
  }
}
