use std::{time::Duration, collections::VecDeque, task::{Waker, Context, Poll}, pin::Pin};
use crate::error::{Result, Error};
use bytes::Bytes;
use flo_net::{
  stream::FloStream,
  ping::{PingStream, PingMsg},
  packet::{Frame, PacketTypeId}
};
use futures::Stream;
use tokio_stream::StreamExt;
use crate::broadcast::BroadcastReceiver;
use crate::game::stream::{GameStreamEvent, GameStreamDataSnapshot};

const PING_INTERVAL: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(10);

pub struct GameStreamServer {
  game_id: i32,
  snapshot: Option<GameStreamDataSnapshot>,
  rx: BroadcastReceiver<GameStreamEvent>,
}

impl GameStreamServer {
  pub fn new(game_id: i32, snapshot: GameStreamDataSnapshot, rx: BroadcastReceiver<GameStreamEvent>) -> Self {
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
    }

    let mut ping = PingStream::interval(PING_INTERVAL, PING_TIMEOUT);
    ping.start();
    loop {
      tokio::select! {
        r = self.rx.recv() => {
          match r {
            Ok(event) => {
              match event {
                GameStreamEvent::Chunk { frames } => {
                  ready_frames.push_frames(&frames);
                },
              }
            }
            Err(err) => {
              use tokio::sync::broadcast::error::RecvError;
              if let RecvError::Lagged(n) = err {
                return Err(Error::ObserverPeerLagged(n))
              }
              transport.send_frame(Frame::new_empty(PacketTypeId::ObserverDataEnd)).await?;
              break;
            }
          }
        }
        Some(frame) = ready_frames.next() => {
          transport.send_frame(frame).await?;
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
}

impl ReadyFrameStream {
  fn new() -> Self {
    Self {
      q: VecDeque::new(),
      w: None
    }
  }

  fn push_frames(&mut self, frames: &[Bytes]) 
  {
    for frame in frames {
      self.q.push_back(Frame::new_bytes(PacketTypeId::ObserverData, frame.clone()));
    }
    if !self.q.is_empty() {
      self.w.take().map(|w| w.wake());
    }
  }
}

impl Stream for ReadyFrameStream {
    type Item = Frame;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
      if let Some(frame) = self.q.pop_front() {
        return Poll::Ready(Some(frame))
      }

      match self.w {
        Some(ref w) => {
          if !w.will_wake(cx.waker()) {
            self.w.replace(cx.waker().clone());
          }
        },
        None => {
          self.w.replace(cx.waker().clone());
        }
      }

      Poll::Pending
    }
}
