use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use flo_net::packet::Frame;
use flo_net::stream::FloStream;

use crate::error::*;

#[derive(Debug)]
pub struct PeerStream {
  player_id: i32,
  receiver: Receiver<PeerCommand>,
  stream: FloStream,
  closed: bool,
}

impl PeerStream {
  pub fn new(
    player_id: i32,
    stream: FloStream,
  ) -> Result<(PeerStream, PeerHandle), (FloStream, Error)> {
    let (sender, receiver) = channel(crate::constants::PEER_COMMAND_CHANNEL_SIZE);
    let stream = Self {
      player_id,
      receiver,
      stream,
      closed: false,
    };
    Ok((stream, PeerHandle { sender }))
  }

  pub fn player_id(&self) -> i32 {
    self.player_id
  }

  pub fn get_mut(&mut self) -> &mut FloStream {
    &mut self.stream
  }
}

impl Into<FloStream> for PeerStream {
  fn into(self) -> FloStream {
    self.stream
  }
}

impl Stream for PeerStream {
  type Item = Result<PeerMessage>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.closed {
      return Poll::Ready(None);
    }

    // outgoing
    match Pin::new(&mut self.receiver).poll_next(cx) {
      Poll::Ready(Some(cmd)) => {
        return Poll::Ready(Some(Ok(match cmd {
          PeerCommand::Send(frame) => PeerMessage::Outgoing(frame),
          PeerCommand::StartPing => PeerMessage::StartPing,
        })));
      }
      _ => {}
    }

    // incoming
    let result: Option<_> = futures::ready!(Pin::new(&mut self.stream).poll_next(cx));
    Poll::Ready(match result {
      Some(Ok(frame)) => Some(Ok(PeerMessage::Incoming(frame))),
      Some(Err(e)) => {
        self.closed = true;
        cx.waker().wake_by_ref();
        Some(Err(e.into()))
      }
      None => None,
    })
  }
}

#[derive(Debug)]
pub enum PeerMessage {
  Outgoing(Frame),
  Incoming(Frame),
  StartPing,
}

#[derive(Debug)]
pub struct PeerHandle {
  sender: Sender<PeerCommand>,
}

impl PeerHandle {
  pub async fn send_frame(&mut self, frame: Frame) -> Result<(), Frame> {
    self
      .sender
      .send(PeerCommand::Send(frame))
      .await
      .map_err(|err| {
        if let PeerCommand::Send(frame) = err.0 {
          frame
        } else {
          unreachable!()
        }
      })
  }

  pub async fn start_ping(&mut self) {
    self.sender.send(PeerCommand::StartPing).await.ok();
  }
}

enum PeerCommand {
  Send(Frame),
  StartPing,
}
