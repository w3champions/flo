use crate::broadcast::BroadcastReceiver;
use crate::error::{Error, Result};
use crate::game::stream::{GameStreamDataSnapshot, GameStreamEvent};
use flo_net::{
  packet::{Frame, PacketTypeId},
  ping::{PingMsg, PingStream},
  stream::FloStream,
};
use std::time::Duration;
use tokio_stream::StreamExt;
use super::send_queue::{GameStreamSendQueue, NoDelaySendQueue, DelaySendQueue};

const PING_INTERVAL: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(10);

pub struct GameStreamServer {
  game_id: i32,
  initial_arrival_time_millis: i64,
  delay_secs: Option<i64>,
  snapshot: Option<GameStreamDataSnapshot>,
  rx: BroadcastReceiver<GameStreamEvent>,
}

impl GameStreamServer {
  pub fn new(
    game_id: i32,
    delay_secs: Option<i64>,
    snapshot: GameStreamDataSnapshot,
    rx: BroadcastReceiver<GameStreamEvent>,
  ) -> Self {
    Self {
      game_id,
      initial_arrival_time_millis: snapshot.initial_arrival_time_millis,
      delay_secs,
      snapshot: Some(snapshot),
      rx,
    }
  }

  pub async fn run(mut self, mut transport: FloStream) -> Result<()> {
    let game_id = self.game_id;
    let mut send_queue: Box<dyn GameStreamSendQueue> = if let Some(delay_secs) = self.delay_secs {
      Box::new(DelaySendQueue::new(self.initial_arrival_time_millis, delay_secs * 1000))
    } else {
      Box::new(NoDelaySendQueue::new())
    };

    if let Some(snapshot) = self.snapshot.take() {
      send_queue.push_frames(&snapshot.frames);
      if snapshot.ended {
        send_queue.finish()
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
                  send_queue.push_frames(&frames);
                  if ended {
                    send_queue.finish();
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
        next = send_queue.next() => {
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