use super::messages::{IncomingMessage, OutgoingMessage};
use crate::error::Result;
use flo_state::async_trait;

#[async_trait]
pub trait MessageStream: Send + Sync + 'static {
  async fn send(&mut self, msg: OutgoingMessage) -> Result<()>;
  async fn recv(&mut self) -> Option<IncomingMessage>;
  async fn flush(&mut self);
}
