use crate::packet::{Frame, FramePayload, PacketTypeId};
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
  interval: Duration,
  timeout: Duration,
}

#[derive(Clone, Copy)]
enum SleepReason {
  Ping,
  PongTimeout,
}

impl PingStream {
  pub const PING_TYPE_ID: PacketTypeId = PacketTypeId::Ping;
  pub const PONG_TYPE_ID: PacketTypeId = PacketTypeId::Pong;

  pub fn interval(interval: Duration, timeout: Duration) -> Self {
    Self {
      base_instant: Instant::now(),
      delay: None,
      delay_reason: SleepReason::Ping,
      waker: None,
      interval,
      timeout,
    }
  }

  pub fn start(&mut self) {
    self.delay_reason = SleepReason::Ping;
    self.delay = Box::pin(sleep(self.interval)).into();
    self.waker.take().map(|w| w.wake());
  }

  pub fn stop(&mut self) {
    self.delay_reason = SleepReason::Ping;
    self.delay.take();
  }

  pub fn started(&self) -> bool {
    self.delay.is_some()
  }

  pub fn capture_pong(&mut self, frame: Frame) -> Option<u32> {
    let payload = match frame.payload {
      FramePayload::Bytes(ref bytes) => {
        if let Some(slice) = bytes.get(0..4) {
          let mut bytes = [0; 4];
          bytes.copy_from_slice(slice);
          u32::from_be_bytes(bytes)
        } else {
          return None;
        }
      }
      FramePayload::W3GS { .. } => return None,
    };

    let d = if let Some(v) = self.now().checked_sub(payload) {
      v
    } else {
      return None;
    };
    if let Some(ref mut delay) = self.delay {
      delay
        .as_mut()
        .reset((Instant::now() + self.interval).into());
    } else {
      return None;
    }
    match self.delay_reason {
      SleepReason::Ping => {}
      SleepReason::PongTimeout => self.delay_reason = SleepReason::Ping,
    };
    Some(d)
  }

  fn now(&self) -> u32 {
    Instant::now()
      .saturating_duration_since(self.base_instant)
      .as_millis() as u32
  }

  fn get_ping_frame(&self) -> Frame {
    Frame::new(Self::PING_TYPE_ID, self.now().to_be_bytes())
  }
}

impl Stream for PingStream {
  type Item = PingMsg;

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
        PingMsg::Ping(self.get_ping_frame()),
        SleepReason::PongTimeout,
        self.timeout,
      ),
      SleepReason::PongTimeout => (PingMsg::Timeout, SleepReason::Ping, self.interval),
    };

    self.delay_reason = reason;
    self
      .delay
      .as_mut()
      .map(|delay| delay.as_mut().reset((Instant::now() + duration).into()))
      .unwrap();

    Poll::Ready(Some(msg))
  }
}

pub enum PingMsg {
  Ping(Frame),
  Timeout,
}
