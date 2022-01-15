use std::ops::{Deref, DerefMut};

use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};

pub struct BroadcastSender<E> {
  tx: broadcast::Sender<E>,
}

impl<E> BroadcastSender<E>
where
  E: Clone,
{
  pub fn channel() -> (Self, BroadcastReceiver<E>) {
    let (tx, rx) = broadcast::channel(16);
    (BroadcastSender { tx }, BroadcastReceiver { rx })
  }

  pub fn send(&self, event: E) -> bool {
    self.tx.send(event).is_ok()
  }

  pub fn subscribe(&self) -> BroadcastReceiver<E> {
    BroadcastReceiver {
      rx: self.tx.subscribe(),
    }
  }

  pub fn is_closed(&self) -> bool {
    self.tx.receiver_count() == 0
  }
}

pub struct BroadcastReceiver<E> {
  rx: broadcast::Receiver<E>,
}

impl<E> Deref for BroadcastReceiver<E> {
  type Target = broadcast::Receiver<E>;
  fn deref(&self) -> &Self::Target {
    &self.rx
  }
}

impl<E> DerefMut for BroadcastReceiver<E> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.rx   
  }
}

impl<E> BroadcastReceiver<E> {
  pub fn into_stream(self) -> impl Stream<Item = E>
  where
    E: Clone + Send + 'static,
  {
    BroadcastStream::new(self.rx).filter_map(|item| item.ok())
  }
}
