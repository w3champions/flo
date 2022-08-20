use flo_net::packet::Frame;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use tokio::time::{sleep, Sleep};

#[derive(Debug, Clone)]
pub enum DelayedFrame {
  In(Frame),
  Out(Frame),
}

pub struct DelayedFrameStream {
  sleep: Pin<Box<Sleep>>,
  frames: VecDeque<(DelayedFrame, Duration)>,
  duration: Duration,
  waker: Option<Waker>,
  delayed: Option<DelayedFrame>,
  last_deadline: Option<Instant>,
}

impl DelayedFrameStream {
  pub fn new(duration: Option<Duration>) -> Self {
    Self {
      duration: duration.map(|v| v / 2).unwrap_or(Duration::ZERO),
      sleep: Box::pin(sleep(Duration::ZERO)),
      frames: VecDeque::new(),
      waker: None,
      delayed: None,
      last_deadline: None,
    }
  }

  pub fn recv_expired<'a, 'b>(
    &'a mut self,
    buf: &'b mut VecDeque<DelayedFrame>,
  ) -> ExpiredFuture<'a, 'b> {
    ExpiredFuture { owner: self, buf }
  }

  pub fn set_delay(&mut self, delay: Duration) {
    let set_value = delay / 2;
    if self.duration == set_value {
      return
    }

    self.reset_all(self.duration, set_value);
    self.duration = set_value;
    self.sleep.as_mut().reset(Instant::now().into());
    self.waker.take().map(|w| w.wake());
  }

  #[must_use]
  pub fn remove_delay(&mut self) -> Option<Vec<DelayedFrame>> {
    self.duration = Duration::ZERO;
    if self.frames.is_empty() {
      None
    } else {
      let mut items = Vec::with_capacity(self.frames.len());
      while let Some((frame, _)) = self.frames.pop_front() {
        items.push(frame);
      }
      Some(items)
    }
  }

  pub fn enabled(&self) -> bool {
    self.duration > Duration::ZERO
  }

  pub fn insert(&mut self, frame: DelayedFrame) {
    self.frames.push_back((frame, self.duration));
    self.waker.take().map(|w| w.wake());
  }

  fn reset_all(&mut self, old: Duration, new: Duration) {
    for (_, duration) in self.frames.iter_mut() {
      if old > new { // reduce
        *duration = duration.saturating_sub(old.saturating_sub(new));
      } else if old < new { // increase
        *duration = duration.saturating_add(new.saturating_sub(old));
      }
    }
  }
}

pub struct ExpiredFuture<'a, 'b> {
  owner: &'a mut DelayedFrameStream,
  buf: &'b mut VecDeque<DelayedFrame>,
}

impl<'a, 'b> Future for ExpiredFuture<'a, 'b> {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    futures::ready!(self.owner.sleep.as_mut().poll(cx));

    if let Some(frame) = self.owner.delayed.take() {
      self.buf.push_back(frame);
      return Poll::Ready(());
    }

    if let Some((frame, delay)) = self.owner.frames.pop_front() {
      if delay > Duration::ZERO {
        let now = Instant::now();
        let last_tick_cost = if let Some(last) = self.owner.last_deadline.take() {
          now.checked_duration_since(last).unwrap_or_default()
        } else {
          Duration::default()
        };
        let deadline = now + delay.saturating_sub(last_tick_cost);
        self.owner.last_deadline.replace(deadline);
        self.owner.sleep.as_mut().reset(deadline.into());
        if let Poll::Ready(_) = self.owner.sleep.as_mut().poll(cx) {
          self.buf.push_back(frame);
          Poll::Ready(())
        } else {
          self.owner.delayed.replace(frame);
          Poll::Pending
        }
      } else {
        self.buf.push_back(frame);
        Poll::Ready(())
      }
    } else {
      if !self
        .owner
        .waker
        .as_ref()
        .map(|w| w.will_wake(cx.waker()))
        .unwrap_or_default()
      {
        self.owner.waker.replace(cx.waker().clone());
      }
      Poll::Pending
    }
  }
}
