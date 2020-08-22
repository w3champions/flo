use futures::future::BoxFuture;
use futures::FutureExt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub trait FloEvent: Sized + Send {
  const NAME: &'static str;

  fn description(&self) -> Option<String> {
    None
  }

  fn channel(buffer: usize) -> (Sender<Self>, Receiver<Self>) {
    channel(buffer)
  }
}

pub trait EventSenderExt<T> {
  fn send_or_log_as_error(&mut self, event: T) -> BoxFuture<()>;
  fn send_or_discard(&mut self, event: T) -> BoxFuture<()>;
}

#[derive(Debug)]
pub struct EventSender<T> {
  inner: Sender<T>,
  closed: Arc<AtomicBool>,
}

impl<T> Clone for EventSender<T> {
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      closed: self.closed.clone(),
    }
  }
}

impl<T> EventSender<T> {
  #[inline]
  fn new(inner: Sender<T>) -> Self {
    Self {
      inner,
      closed: Arc::new(AtomicBool::new(false)),
    }
  }

  #[inline]
  pub async fn send(&mut self, event: T) -> Result<(), EventSendError<T>> {
    let result = self.inner.send(event).await;

    if let Err(err) = result {
      return Err(EventSendError::new(err.0));
    }

    Ok(())
  }

  #[inline]
  pub fn close(&self) {
    self.closed.store(true, Ordering::SeqCst)
  }
}

impl<T> EventSenderExt<T> for EventSender<T>
where
  T: FloEvent,
{
  #[inline]
  fn send_or_log_as_error(&mut self, event: T) -> BoxFuture<()> {
    if self.closed.load(Ordering::SeqCst) {
      if let Some(description) = event.description() {
        tracing::error!("{} event discarded (closed): {}", T::NAME, description);
      } else {
        tracing::error!("{} event discarded (closed)", T::NAME);
      }
      return async {}.boxed();
    }
    self
      .inner
      .send(event)
      .map(|result| match result {
        Ok(()) => {}
        Err(err) => {
          let event = match TrySendError::from(err) {
            TrySendError::Closed(msg) => msg,
            TrySendError::Full(msg) => msg,
          };
          if let Some(description) = event.description() {
            tracing::error!("{} event discarded: {}", T::NAME, description);
          } else {
            tracing::error!("{} event discarded", T::NAME);
          }
        }
      })
      .boxed()
  }

  #[inline]
  fn send_or_discard(&mut self, event: T) -> BoxFuture<()> {
    if self.closed.load(Ordering::SeqCst) {
      tracing::debug!("{} discard: closed", T::NAME);
      return async {}.boxed();
    }
    self
      .inner
      .send(event)
      .map(|result| {
        if let Err(_) = result {
          tracing::debug!("{} discard: receiver dropped", T::NAME);
        }
      })
      .boxed()
  }
}

impl<T> From<Sender<T>> for EventSender<T> {
  fn from(inner: Sender<T>) -> Self {
    Self::new(inner)
  }
}

impl<T> EventSenderExt<T> for Sender<T>
where
  T: FloEvent,
{
  #[inline]
  fn send_or_log_as_error(&mut self, event: T) -> BoxFuture<()> {
    self
      .send(event)
      .map(|result| match result {
        Ok(()) => {}
        Err(err) => {
          let msg = match TrySendError::from(err) {
            TrySendError::Closed(msg) => msg,
            TrySendError::Full(msg) => msg,
          };
          if let Some(description) = msg.description() {
            tracing::error!("{} event discarded: {}", T::NAME, description);
          } else {
            tracing::error!("{} event discarded", T::NAME);
          }
        }
      })
      .boxed()
  }

  #[inline]
  fn send_or_discard(&mut self, event: T) -> BoxFuture<()> {
    self.send(event).map(|_result| ()).boxed()
  }
}

#[derive(Debug)]
pub struct EventFromSender<T, FromType> {
  inner: Sender<T>,
  _phantom: PhantomData<FromType>,
}

impl<T, FromType> Clone for EventFromSender<T, FromType> {
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      _phantom: PhantomData,
    }
  }
}

impl<T, FromType> EventSenderExt<FromType> for EventFromSender<T, FromType>
where
  T: FloEvent,
  T: From<FromType>,
{
  #[inline]
  fn send_or_log_as_error(&mut self, event: FromType) -> BoxFuture<()> {
    self.inner.send_or_log_as_error(T::from(event))
  }

  #[inline]
  fn send_or_discard(&mut self, event: FromType) -> BoxFuture<()> {
    self.inner.send_or_discard(T::from(event))
  }
}

impl<T, FromType> From<Sender<T>> for EventFromSender<T, FromType>
where
  T: FloEvent,
  T: From<FromType>,
{
  fn from(sender: Sender<T>) -> Self {
    Self {
      inner: sender,
      _phantom: PhantomData,
    }
  }
}

#[derive(Debug)]
pub struct EventSendError<T> {
  inner: T,
}

impl<T> EventSendError<T> {
  #[inline]
  fn new(inner: T) -> Self {
    EventSendError { inner }
  }

  #[inline]
  pub fn into_inner(self) -> T {
    self.inner
  }
}
