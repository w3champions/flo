use flo_net::packet::Frame;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::time::error::Error;
use tokio_util::time::delay_queue::{self, DelayQueue};

#[derive(Debug, Clone)]
pub enum DelayedFrame {
  In(Frame),
  Out(Frame),
}

pub struct DelayedFrameStream {
  duration: Option<Duration>,
  waker: Option<Waker>,
  next_id: usize,
  ordered_ids: VecDeque<(usize, delay_queue::Key)>,
  delay_queue: DelayQueue<(usize, DelayedFrame)>,
}

impl DelayedFrameStream {
  pub fn new(duration: Option<Duration>) -> Self {
    Self {
      duration: duration.map(|v| v / 2),
      waker: None,
      next_id: 0,
      ordered_ids: VecDeque::new(),
      delay_queue: DelayQueue::new(),
    }
  }

  pub fn recv_expired<'a, 'b>(
    &'a mut self,
    buf: &'b mut VecDeque<DelayedFrame>,
  ) -> ExpiredFuture<'a, 'b> {
    ExpiredFuture { owner: self, buf }
  }

  #[must_use]
  pub fn set_delay(&mut self, delay: Duration) -> Option<Vec<DelayedFrame>> {
    let set_value = delay / 2;
    if self.duration == Some(set_value) {
      return None;
    }

    if self.duration.is_none() {
      self.waker.take().map(|w| w.wake());
    }

    self.duration.replace(set_value);
    self.remove_all()
  }

  #[must_use]
  pub fn remove_delay(&mut self) -> Option<Vec<DelayedFrame>> {
    self.duration.take();
    self.remove_all()
  }

  pub fn enabled(&self) -> bool {
    self.duration.is_some()
  }

  pub fn insert(&mut self, frame: DelayedFrame) {
    if let Some(duration) = self.duration {
      let id = self.next_id;
      let key = self.delay_queue.insert((id, frame), duration);
      self.next_id = self.next_id.wrapping_add(1);
      self.ordered_ids.push_back((id, key));
    } else {
      tracing::warn!("delayed frame dropped.");
    }
  }

  fn remove_all(&mut self) -> Option<Vec<DelayedFrame>> {
    if self.ordered_ids.is_empty() {
      None
    } else {
      let mut items = Vec::with_capacity(self.ordered_ids.len());
      for (_, key) in &self.ordered_ids {
        items.push(self.delay_queue.remove(key).into_inner().1)
      }
      self.delay_queue.clear();
      self.ordered_ids.clear();
      self.next_id = 0;
      Some(items)
    }
  }
}

pub struct ExpiredFuture<'a, 'b> {
  owner: &'a mut DelayedFrameStream,
  buf: &'b mut VecDeque<DelayedFrame>,
}

impl<'a, 'b> Future for ExpiredFuture<'a, 'b> {
  type Output = Result<(), Error>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    if self.owner.duration.is_none() {
      if !self
        .owner
        .waker
        .as_ref()
        .map(|w| w.will_wake(cx.waker()))
        .unwrap_or(false)
      {
        self.owner.waker = Some(cx.waker().clone());
      }
      return Poll::Pending;
    };

    match self.owner.delay_queue.poll_expired(cx)? {
      Poll::Ready(Some(expired)) => {
        self.buf.clear();
        let (expired_id, item) = expired.into_inner();
        while let Some((id, key)) = self.owner.ordered_ids.pop_front() {
          if id != expired_id {
            let (_, item) = self.owner.delay_queue.remove(&key).into_inner();
            self.buf.push_back(item);
            continue;
          } else {
            self.buf.push_back(item);
            break;
          }
        }
        Poll::Ready(Ok(()))
      }
      Poll::Ready(None) => Poll::Pending,
      Poll::Pending => Poll::Pending,
    }
  }
}
