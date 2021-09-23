use bytes::Bytes;
use futures::stream::Stream;
use std::collections::VecDeque;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

const BUFFER_SIZE: usize = 10;

pub fn channel(initial_parts: Vec<Bytes>) -> (GamePartSender, GamePartStream) {
  let (tx, rx) = mpsc::channel(BUFFER_SIZE);
  (
    GamePartSender { tx },
    GamePartStream {
      ready: initial_parts.into_iter().collect(),
      rx,
      done: false,
    },
  )
}

pub struct GamePartStream {
  ready: VecDeque<Bytes>,
  rx: mpsc::Receiver<Item>,
  done: bool,
}

#[derive(Debug)]
pub struct GamePartSender {
  tx: mpsc::Sender<Item>,
}

impl GamePartSender {
  pub fn send_or_drop<T: Into<Item>>(&self, item: T) -> bool {
    self.tx.try_send(item.into()).is_ok()
  }
}

#[derive(Debug)]
pub enum Item {
  Parts(Vec<Bytes>),
  Part(Bytes),
}

impl From<Bytes> for Item {
  fn from(v: Bytes) -> Self {
    Item::Part(v)
  }
}

impl From<Vec<Bytes>> for Item {
  fn from(v: Vec<Bytes>) -> Self {
    Item::Parts(v)
  }
}

impl Stream for GamePartStream {
  type Item = Bytes;

  fn poll_next(
    mut self: std::pin::Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<Self::Item>> {
    if self.done {
      return Poll::Ready(None);
    }

    if let Some(item) = self.ready.pop_front() {
      return Poll::Ready(Some(item));
    }

    if let Some(item) = futures::ready!(self.rx.poll_recv(cx)) {
      match item {
        Item::Parts(mut parts) => {
          if parts.is_empty() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
          }
          let item = parts.remove(0);
          for item in parts {
            self.ready.push_back(item);
          }
          return Poll::Ready(Some(item));
        }
        Item::Part(part) => {
          return Poll::Ready(Some(part));
        }
      }
    } else {
      self.done = true;
      return Poll::Ready(None);
    }
  }
}
