use tokio::sync::mpsc::{Receiver, Sender};

use flo_event::*;

use crate::client::ClientEvent;
use crate::game::GameEvent;

pub type NodeEventSender = Sender<NodeEvent>;

#[derive(Debug)]
pub enum NodeEvent {
  Shutdown,
  Game(GameEvent),
  Client(ClientEvent),
}

impl FloEvent for NodeEvent {
  const NAME: &'static str = "NodeEvent";
}

impl From<GameEvent> for NodeEvent {
  fn from(inner: GameEvent) -> Self {
    Self::Game(inner)
  }
}

impl From<ClientEvent> for NodeEvent {
  fn from(inner: ClientEvent) -> Self {
    Self::Client(inner)
  }
}

pub async fn handle_events(event_receiver: Receiver<NodeEvent>) {}
