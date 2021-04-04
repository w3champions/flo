use crate::constants::{GAME_PING_INTERVAL, GAME_PING_TIMEOUT};
use crate::error::*;
use flo_net::packet::{Frame, PacketTypeId};
use flo_net::w3gs::{frame_to_w3gs, w3gs_to_frame};
use flo_w3gs::packet::{Packet, PacketPayload};
use flo_w3gs::protocol::ping::{PingFromHost, PongToHost};
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::future::Future;
use std::pin::Pin;
use std::task::Waker;
use std::time::{Duration, Instant};
use tokio::time::{delay_for, Delay};

pub struct PingStream {
  base_instant: Instant,
  delay: Option<Delay>,
  delay_reason: DelayReason,
  waker: Option<Waker>,
}

#[derive(Clone, Copy)]
enum DelayReason {
  Ping,
  PongTimeout,
}

impl PingStream {
  pub fn new() -> Self {
    Self {
      base_instant: Instant::now(),
      delay: None,
      delay_reason: DelayReason::Ping,
      waker: None,
    }
  }

  pub fn start(&mut self) {
    self.delay_reason = DelayReason::Ping;
    self.delay = delay_for(Duration::from_secs(0)).into();
    self.waker.take().map(|w| w.wake());
  }

  pub fn started(&self) -> bool {
    self.delay.is_some()
  }

  pub fn is_pong_frame(frame: &Frame) -> bool {
    frame.type_id == PacketTypeId::W3GS
      && frame.payload.subtype_id() == Some(PongToHost::PACKET_TYPE_ID.into())
  }

  pub fn capture_pong(&mut self, frame: Frame) -> Result<Option<u32>> {
    let pong: PongToHost = frame_to_w3gs(frame)?.decode_simple()?;
    let d = if let Some(v) = self.now().checked_sub(pong.payload()) {
      v
    } else {
      return Ok(None);
    };
    if let Some(ref mut delay) = self.delay {
      delay.reset((Instant::now() + GAME_PING_INTERVAL).into());
    } else {
      return Ok(None);
    }
    match self.delay_reason {
      DelayReason::Ping => {}
      DelayReason::PongTimeout => self.delay_reason = DelayReason::Ping,
    };
    Ok(Some(d))
  }

  fn now(&self) -> u32 {
    Instant::now()
      .saturating_duration_since(self.base_instant)
      .as_millis() as u32
  }

  fn get_ping_frame(&self) -> Result<Frame> {
    let pkt = Packet::simple(PingFromHost::with_payload(self.now()))?;
    Ok(w3gs_to_frame(pkt))
  }
}

impl Stream for PingStream {
  type Item = Result<Msg>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if let Some(ref mut delay) = self.delay.as_mut() {
      futures::ready!(Pin::new(delay).poll(cx));
    } else {
      if self.waker.as_ref().map(|w| w.will_wake(cx.waker())) != Some(true) {
        self.waker = Some(cx.waker().clone());
      }
      return Poll::Pending;
    };

    let (msg, reason, duration) = match self.delay_reason {
      DelayReason::Ping => (
        Msg::Ping(self.get_ping_frame()?),
        DelayReason::PongTimeout,
        GAME_PING_TIMEOUT,
      ),
      DelayReason::PongTimeout => (Msg::Timeout, DelayReason::Ping, GAME_PING_INTERVAL),
    };

    self.delay_reason = reason;
    self
      .delay
      .as_mut()
      .map(|delay| delay.reset((Instant::now() + duration).into()))
      .unwrap();

    Poll::Ready(Some(Ok(msg)))
  }
}

pub enum Msg {
  Ping(Frame),
  Timeout,
}
