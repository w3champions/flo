use flo_net::w3gs::W3GSPacket;
use futures::{Future, Stream};
use std::collections::VecDeque;
use std::{
  pin::Pin,
  task::{Context, Poll, Waker},
  time::{Duration, Instant},
};
use tokio::time::{sleep, Sleep};

pub struct SendQueue {
  sleep: Pin<Box<Sleep>>,
  speed: Option<f64>,
  packets: VecDeque<(W3GSPacket, Option<u64>)>,
  buffered_millis: u64,
  total_millis: u64,
  finished: bool,
  exhausted_waker: Option<Waker>,
  delayed: Option<W3GSPacket>,
  last_deadline: Option<Instant>,
}

impl SendQueue {
  pub fn new() -> Self {
    Self {
      sleep: Box::pin(sleep(Default::default())),
      packets: VecDeque::new(),
      buffered_millis: 0,
      total_millis: 0,
      speed: None,
      finished: false,
      exhausted_waker: None,
      delayed: None,
      last_deadline: None,
    }
  }

  pub fn set_speed(&mut self, speed: f64) {
    if speed == 1. {
      self.speed.take();
      return;
    }
    if speed > 0. {
      self.speed.replace(speed);
    }
  }

  pub fn speed(&self) -> f64 {
    self.speed.clone().unwrap_or(1.)
  }

  pub fn buffered_duration(&self) -> Duration {
    Duration::from_millis(self.buffered_millis)
  }

  pub fn total_millis(&self) -> u64 {
    self.total_millis
  }

  pub fn finish(&mut self) {
    self.finished = true;
  }

  pub fn push(&mut self, packet: W3GSPacket, increase_millis: Option<u64>) {
    if self.finished {
      return;
    }

    if let Some(millis) = increase_millis.clone() {
      self.buffered_millis += millis;
      self.total_millis += millis;
    }

    self.packets.push_back((packet, increase_millis));
    self.exhausted_waker.take().map(|w| w.wake());
  }
}

impl Stream for SendQueue {
  type Item = W3GSPacket;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    futures::ready!(self.sleep.as_mut().poll(cx));

    if let Some(pkt) = self.delayed.take() {
      return Poll::Ready(Some(pkt));
    }

    if let Some((pkt, ms)) = self.packets.pop_front() {
      if let Some(ms) = ms {
        self.buffered_millis = self.buffered_millis.saturating_sub(ms);
        let delay = Duration::from_millis(if let Some(f) = self.speed.clone() {
          if f > 0. {
            (ms as f64 / f).floor() as u64
          } else {
            ms
          }
        } else {
          ms
        });
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
          Poll::Ready(Some(pkt))
        } else {
          self.delayed.replace(pkt);
          Poll::Pending
        }
      } else {
        Poll::Ready(Some(pkt))
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
