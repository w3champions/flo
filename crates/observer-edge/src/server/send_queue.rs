use crate::game::stream::GameStreamFrame;
use flo_net::packet::{Frame, PacketTypeId};
use futures::Stream;
use std::{
  collections::VecDeque,
  pin::Pin,
  task::{Context, Poll, Waker},
  time::{Duration, Instant, SystemTime},
};
use tokio_util::time::DelayQueue;

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
  next_seq: usize,
  delayed: DelayQueue<(usize, Frame)>,
  finished: bool,
  exhausted_waker: Option<Waker>,
  expired: VecDeque<(usize, Frame)>,
  base_timestamp_millis: i64,
  base_instant: Instant,
  last_instant: Option<Instant>,
  delay_millis: i64,
  fast_forwarding_millis: i64,
}

impl DelaySendQueue {
  const MAX_PRESEND_MILLIS: i64 = 20_000;

  pub fn new(delay_millis: i64) -> Self {
    let now = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .ok()
      .unwrap_or_default();
    let fast_forwarding_millis =
      (delay_millis as f64 / (flo_constants::OBSERVER_FAST_FORWARDING_SPEED - 1.0)).ceil() as i64;

    tracing::debug!("fast_forwarding_millis = {}", fast_forwarding_millis);

    Self {
      next_seq: 0,
      delayed: DelayQueue::new(),
      finished: false,
      exhausted_waker: None,
      expired: VecDeque::new(),
      base_timestamp_millis: now.as_millis() as i64,
      base_instant: Instant::now(),
      last_instant: None,
      delay_millis,
      fast_forwarding_millis,
    }
  }

  pub fn finish(&mut self) {
    self.finished = true;
  }

  pub fn push_frames(&mut self, frames: &[GameStreamFrame]) {
    if self.finished {
      return;
    }

    let now = self.base_timestamp_millis + (Instant::now() - self.base_instant).as_millis() as i64;
    for frame in frames {
      let fast_forwarding =
        now.saturating_sub(frame.approx_timestamp_millis) <= self.fast_forwarding_millis;
      let delay_millis = if fast_forwarding {
        (self.delay_millis as f64 / flo_constants::OBSERVER_FAST_FORWARDING_SPEED).floor() as _
      } else {
        self.delay_millis
      };

      let delay = (frame.approx_timestamp_millis + delay_millis)
        .saturating_sub(now)
        .saturating_sub(Self::MAX_PRESEND_MILLIS);

      // tracing::debug!(
      //   "frame: approx_timestamp_millis = {}, delay = {}",
      //   frame.approx_timestamp_millis,
      //   delay
      // );

      let delay = Duration::from_millis(std::cmp::max(0_i64, delay) as u64);
      let next_instant = match self.last_instant.take() {
        Some(last) => std::cmp::max(last, Instant::now() + delay),
        None => Instant::now() + delay,
      };
      self.last_instant.replace(next_instant);

      self.delayed.insert_at(
        (
          self.next_seq,
          Frame::new_bytes(PacketTypeId::ObserverData, frame.data.clone()),
        ),
        next_instant.into(),
      );
      self.next_seq = self.next_seq.saturating_add(1);
    }

    self.exhausted_waker.take().map(|w| w.wake());
  }
}

impl Stream for DelaySendQueue {
  type Item = Frame;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut expired = None;
    let mut exhausted = false;
    while let Poll::Ready(item) = self.delayed.poll_expired(cx) {
      match item {
        Some(Ok(item)) => {
          expired
            .get_or_insert_with(|| vec![])
            .push(item.into_inner());
        }
        Some(Err(err)) => {
          tracing::error!("poll_expired call: {}", err);
          self.finished = true;
          return Poll::Ready(None);
        }
        None => {
          exhausted = true;
          break;
        }
      }
    }

    if let Some(mut expired) = expired {
      expired.sort_by_key(|(idx, _)| *idx);
      self.expired.extend(expired);
    }

    if let Some((_, expired)) = self.expired.pop_front() {
      Poll::Ready(Some(expired))
    } else {
      if exhausted {
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
