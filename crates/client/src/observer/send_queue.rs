use flo_net::w3gs::W3GSPacket;
use futures::Stream;
use std::{
  pin::Pin,
  task::{Context, Poll, Waker},
  time::{Duration, Instant},
};
use tokio_util::time::DelayQueue;

pub struct SendQueue {
  base_instant: Option<Instant>,
  next_seq: usize,
  speed: Option<f64>,
  delayed: DelayQueue<(usize, W3GSPacket)>,
  finished: bool,
  exhausted_waker: Option<Waker>,
}

impl SendQueue {
  pub fn new() -> Self {
    Self {
      base_instant: None,
      next_seq: 0,
      delayed: DelayQueue::new(),
      speed: None,
      finished: false,
      exhausted_waker: None,
    }
  }

  pub fn set_speed(&mut self, speed: f64) {
    if speed == 1. {
      self.speed.take();
      return
    }
    if speed > 0. {
      self.speed.replace(speed);
    }
  }

  pub fn speed(&self) -> f64 {
    self.speed.clone().unwrap_or(1.)
  }

  pub fn buffered_duration(&self) -> Duration {
    self.base_instant.clone().and_then(|t| {
      t.checked_duration_since(Instant::now())
    }).unwrap_or_default()
  }

  pub fn finish(&mut self) {
    self.finished = true;
  }

  pub fn push(&mut self, packet: W3GSPacket, increase_millis: Option<u64>) {
    if self.finished {
      return;
    }

    if let Some(increase_millis) = increase_millis {
      if let Some(instant) = self.base_instant.take() {
        let delay = Duration::from_millis(if let Some(f) = self.speed.clone() {
          if f > 0. {
            (increase_millis as f64 / f).round() as u64
          } else {
            increase_millis
          }
        } else {
          increase_millis
        });
        self
          .delayed
          .insert_at((self.next_seq, packet), (instant + delay).into());
        self.base_instant.replace(instant + delay);
      } else {
        let now = Instant::now();
        self
          .delayed
          .insert_at((self.next_seq, packet), now.into());
        self.base_instant.replace(now);
      }
    } else {
      match self.base_instant.clone() {
        Some(instant) => {
          self
            .delayed
            .insert_at((self.next_seq, packet), instant.into());
        }
        None => {
          let now = Instant::now();
          self.base_instant.replace(now);
        }
      }
    }

    self.exhausted_waker.take().map(|w| w.wake());
    self.next_seq = self.next_seq.saturating_add(1);
  }
}

impl Stream for SendQueue {
  type Item = Vec<(usize, W3GSPacket)>;

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
          if self.finished {
            return Poll::Ready(None);
          }
          exhausted = true;
          break;
        }
      }
    }

    if let Some(mut expired) = expired {
      expired.sort_by_key(|(idx, _)| *idx);

      

      Poll::Ready(Some(expired))
    } else {
      if exhausted {
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
