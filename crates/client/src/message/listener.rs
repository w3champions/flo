use super::message::{IncomingMessage, OutgoingMessage};
use crate::error::Result;
use flo_state::async_trait;

#[async_trait]
pub trait MessageStream: Send {
  async fn send(&mut self, msg: OutgoingMessage) -> Result<()>;
  async fn recv(&mut self) -> Option<IncomingMessage>;
  async fn flush(&mut self);
}

#[async_trait]
pub trait MessageListener: Send {
  async fn accept(&mut self) -> Result<Option<Box<dyn MessageStream>>>;
}
