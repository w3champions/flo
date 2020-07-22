use futures::stream::Stream;
use futures::task::Context;
use std::pin::Pin;
use std::sync::Arc;
use tokio::macros::support::Poll;
use tokio::sync::{mpsc, Mutex};
use tonic::Status;

use crate::game::Game;
use crate::player::PlayerRef;

type Tx = mpsc::Sender<Notification>;
type Rx = mpsc::Receiver<Notification>;

pub struct Notification;

pub struct NotificationStream {
  rx: Rx,
  closed: bool,
}

#[derive(Debug, Clone)]
pub struct NotificationSender {
  tx: Arc<Mutex<Tx>>,
}
pub struct ConnectState {
  pub player_id: i32,
  pub joined_game: Option<Game>,
}

impl Stream for NotificationStream {
  type Item = Result<Notification, Status>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.closed {
      return Poll::Ready(None);
    }

    let item = futures::ready!(Pin::new(&mut self.rx).poll_next(cx));
    match item {
      Some(notification) => Poll::Ready(Some(Ok(notification))),
      None => {
        self.closed = true;
        return Poll::Ready(None);
      }
    }
  }
}
