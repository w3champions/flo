pub mod messages;
mod session;
mod stream;
use flo_state::Message;
pub use session::Session;

use crate::error::Error;

pub mod embed;
#[cfg(feature = "ws")]
pub mod ws;

#[derive(Debug)]
pub enum MessageEvent {
  ConnectController(ConnectController),
  Disconnect,
  WorkerError(Error),
}

impl Message for MessageEvent {
  type Result = ();
}

#[derive(Debug)]
pub struct ConnectController {
  pub token: String,
}

impl Message for ConnectController {
  type Result = ();
}

impl From<tokio::sync::mpsc::error::SendError<MessageEvent>> for Error {
  fn from(_: tokio::sync::mpsc::error::SendError<MessageEvent>) -> Error {
    Error::TaskCancelled(anyhow::format_err!("WsEvent dropped"))
  }
}
