use crate::game::stream::GameStreamFrame;
use flo_net::packet::{Frame, PacketTypeId};
use futures::{Future, Stream};
use std::{
  collections::VecDeque,
  pin::Pin,
  task::{Context, Poll, Waker},
  time::{Duration, Instant, SystemTime},
};
use tokio::time::{sleep, Sleep};

pub trait GameStreamSendQueue: Stream<Item = Frame> + Unpin + Send + 'static {
  fn push_frames(&mut self, frames: &[GameStreamFrame]);
  fn finish(&mut self);
}

pub struct NoDelaySendQueue {
  q: VecDeque<Frame>,
  w: Option<Waker>,
  finished: bool,
}

impl NoDelaySendQueue {
  pub fn new() -> Self {
    Self {
      q: VecDeque::new(),
      w: None,
      finished: false,
    }
  }

  fn push_frames(&mut self, frames: &[GameStreamFrame]) {
    if self.finished {
      return;
    }

    for frame in frames {
      self.q.push_back(Frame::new_bytes(
        PacketTypeId::ObserverData,
        frame.data.clone(),
      ));
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

impl Stream for NoDelaySendQueue {
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

impl GameStreamSendQueue for NoDelaySendQueue {
  fn push_frames(&mut self, frames: &[GameStreamFrame]) {
    NoDelaySendQueue::push_frames(self, frames)
  }
  fn finish(&mut self) {
    NoDelaySendQueue::finish(self)
  }
}

pub struct DelaySendQueue {
  sleep: Pin<Box<Sleep>>,
  finished: bool,
  frames: VecDeque<(GameStreamFrame, u64)>,
  delay_forwarding_millis: u64,
  late_start_millis: u64,
  last_frame_millis: u64,
  time_millis: u64,
  exhausted_waker: Option<Waker>,
  delayed: Option<Frame>,
  last_deadline: Option<Instant>,
}

impl DelaySendQueue {
  const MAX_PRESEND_MILLIS: u64 = 20_000;

  pub fn new(initial_arrival_time_millis: i64, delay_millis: i64) -> Self {
    let late_start_millis = (SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .ok()
      .unwrap_or_default()
      .as_millis() as u64)
      .saturating_sub(delay_millis as u64)
      .saturating_sub(initial_arrival_time_millis as _);

    let delay_forwarding_millis =
      (delay_millis as f64 / (flo_constants::OBSERVER_FAST_FORWARDING_SPEED - 1.0)).ceil() as u64;

    tracing::debug!(
      "fast_forwarding_millis = {}, late_start_millis = {}",
      delay_forwarding_millis,
      late_start_millis
    );

    Self {
      finished: false,
      exhausted_waker: None,
      delay_forwarding_millis,
      late_start_millis,
      sleep: Box::pin(sleep(Default::default())),
      frames: VecDeque::new(),
      last_frame_millis: 0,
      time_millis: 0,
      delayed: None,
      last_deadline: None,
    }
  }

  pub fn finish(&mut self) {
    self.finished = true;
  }

  pub fn push_frames(&mut self, frames: &[GameStreamFrame]) {
    if self.finished {
      return;
    }

    for frame in frames {
      let time_increment_ms = if self.last_frame_millis == 0 {
        0
      } else {
        (frame.approx_timestamp_millis as u64).saturating_sub(self.last_frame_millis)
      };
      self.last_frame_millis = frame.approx_timestamp_millis as u64;
      self.time_millis += time_increment_ms;

      let time = self.time_millis.saturating_sub(self.late_start_millis);
      let frame_delay = if time < Self::MAX_PRESEND_MILLIS {
        0
      } else {
        let fast_forwarding = time <= (Self::MAX_PRESEND_MILLIS + self.delay_forwarding_millis);
        if fast_forwarding {
          (time_increment_ms as f64 / flo_constants::OBSERVER_FAST_FORWARDING_SPEED).floor() as u64
        } else {
          time_increment_ms
        }
      };

      // tracing::debug!(
      //   "frame: approx_timestamp_millis = {}, time_increment_ms = {}, frame_delay = {}",
      //   frame.approx_timestamp_millis,
      //   time_increment_ms,
      //   frame_delay
      // );

      self.frames.push_back((frame.clone(), frame_delay))
    }

    self.exhausted_waker.take().map(|w| w.wake());
  }
}

impl Stream for DelaySendQueue {
  type Item = Frame;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    futures::ready!(self.sleep.as_mut().poll(cx));

    if let Some(pkt) = self.delayed.take() {
      return Poll::Ready(Some(pkt));
    }

    if let Some((frame, ms)) = self.frames.pop_front() {
      if ms > 0 {
        let delay = Duration::from_millis(ms);
        let now = Instant::now();
        let last_tick_cost = if let Some(last) = self.last_deadline.take() {
          now.checked_duration_since(last).unwrap_or_default()
        } else {
          Duration::default()
        };
        let deadline = now
          + if last_tick_cost < delay {
            delay - last_tick_cost
          } else {
            Duration::default()
          };
        self.last_deadline.replace(deadline);
        self.sleep.as_mut().reset(deadline.into());
        if let Poll::Ready(_) = self.sleep.as_mut().poll(cx) {
          Poll::Ready(Some(Frame::new_bytes(
            PacketTypeId::ObserverData,
            frame.data,
          )))
        } else {
          self
            .delayed
            .replace(Frame::new_bytes(PacketTypeId::ObserverData, frame.data));
          Poll::Pending
        }
      } else {
        Poll::Ready(Some(Frame::new_bytes(
          PacketTypeId::ObserverData,
          frame.data,
        )))
      }
    } else {
      if self.finished {
        return Poll::Ready(None);
      }

      if !self
        .exhausted_waker
        .as_ref()
        .map(|w| w.will_wake(cx.waker()))
        .unwrap_or_default()
      {
        self.exhausted_waker.replace(cx.waker().clone());
      }
      Poll::Pending
    }
  }
}

impl GameStreamSendQueue for DelaySendQueue {
  fn push_frames(&mut self, frames: &[GameStreamFrame]) {
    DelaySendQueue::push_frames(self, frames)
  }

  fn finish(&mut self) {
    DelaySendQueue::finish(self)
  }
}
