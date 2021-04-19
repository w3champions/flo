use crate::constants::{GAME_PING_INTERVAL, GAME_PING_TIMEOUT};
use crate::error::*;
use flo_net::packet::{FloPacket, Frame};
use flo_net::proto::flo_common::{PacketPing, PacketPong};
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::future::Future;
use std::pin::Pin;
use std::task::Waker;
use std::time::{Duration, Instant};
use tokio::time::{sleep, Sleep};

pub struct PingStream {
  base_instant: Instant,
  delay: Option<Pin<Box<Sleep>>>,
  delay_reason: SleepReason,
  waker: Option<Waker>,
}

#[derive(Clone, Copy)]
enum SleepReason {
  Ping,
  PongTimeout,
}

impl PingStream {
  pub fn new() -> Self {
    Self {
      base_instant: Instant::now(),
      delay: None,
      delay_reason: SleepReason::Ping,
      waker: None,
    }
  }

  pub fn start(&mut self) {
    self.delay_reason = SleepReason::Ping;
    self.delay = Box::pin(sleep(Duration::from_secs(0))).into();
    self.waker.take().map(|w| w.wake());
  }

  pub fn started(&self) -> bool {
    self.delay.is_some()
  }

  pub fn capture_pong(&mut self, frame: Frame) -> Result<Option<u32>> {
    let pong: PacketPong = frame.decode()?;
    let d = if let Some(v) = self.now().checked_sub(pong.ms) {
      v
    } else {
      return Ok(None);
    };
    if let Some(ref mut delay) = self.delay {
      delay
        .as_mut()
        .reset((Instant::now() + GAME_PING_INTERVAL).into());
    } else {
      return Ok(None);
    }
    match self.delay_reason {
      SleepReason::Ping => {}
      SleepReason::PongTimeout => self.delay_reason = SleepReason::Ping,
    };
    Ok(Some(d))
  }

  fn now(&self) -> u32 {
    Instant::now()
      .saturating_duration_since(self.base_instant)
      .as_millis() as u32
  }

  fn get_ping_frame(&self) -> Result<Frame> {
    let frame = PacketPing { ms: self.now() }.encode_as_frame()?;
    Ok(frame)
  }
}

impl Stream for PingStream {
  type Item = Result<Msg>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if let Some(delay) = self.delay.as_mut() {
      futures::ready!(delay.as_mut().poll(cx));
    } else {
      if self.waker.as_ref().map(|w| w.will_wake(cx.waker())) != Some(true) {
        self.waker = Some(cx.waker().clone());
      }
      return Poll::Pending;
    };

    let (msg, reason, duration) = match self.delay_reason {
      SleepReason::Ping => (
        Msg::Ping(self.get_ping_frame()?),
        SleepReason::PongTimeout,
        GAME_PING_TIMEOUT,
      ),
      SleepReason::PongTimeout => (Msg::Timeout, SleepReason::Ping, GAME_PING_INTERVAL),
    };

    self.delay_reason = reason;
    self
      .delay
      .as_mut()
      .map(|delay| delay.as_mut().reset((Instant::now() + duration).into()))
      .unwrap();

    Poll::Ready(Some(Ok(msg)))
  }
}

pub enum Msg {
  Ping(Frame),
  Timeout,
}
