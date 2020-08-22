use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Notify;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::packet::Frame;
use flo_net::stream::FloStream;

use crate::error::*;

pub type ClientEventSender = Sender<ClientEvent>;

#[derive(Debug)]
pub struct ClientEvent {
  pub player_id: i32,
  pub data: ClientEventData,
}

#[derive(Debug)]
pub enum ClientEventData {
  Disconnected,
}

#[derive(Debug)]
pub struct Client {
  state: Arc<State>,
}

impl Client {
  pub fn new(event_sender: ClientEventSender, player_id: i32, stream: FloStream) -> Self {
    let dropper = Arc::new(Notify::new());
    let (frame_sender, frame_receiver) = channel(5);
    let state = Arc::new(State {
      event_sender,
      player_id,
      frame_sender,
      dropper,
    });

    tokio::spawn(
      Self::handle_stream(state.clone(), stream)
        .instrument(tracing::debug_span!("worker", player_id)),
    );

    Self { state }
  }

  async fn handle_stream(state: Arc<State>, stream: FloStream) {}
}

#[derive(Debug)]
struct State {
  event_sender: ClientEventSender,
  player_id: i32,
  frame_sender: Sender<Frame>,
  dropper: Arc<Notify>,
}
