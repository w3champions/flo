use tokio::sync::mpsc::{Receiver, Sender};

use flo_event::*;

use crate::controller::ControllerServerHandle;
use crate::error::*;
use crate::state::GlobalStateRef;

pub type GlobalEventSender = Sender<GlobalEvent>;

// Infrequent events that can be processed offline quickly
#[derive(Debug)]
pub enum GlobalEvent {
  // A game has ended, remove the session from global state
  GameEnded(i32),
  // Shutdown,
}

impl FloEvent for GlobalEvent {
  const NAME: &'static str = "NodeEvent";
}

#[derive(Debug)]
pub struct FloNodeEventContext {
  pub state: GlobalStateRef,
  pub ctrl: ControllerServerHandle,
}

pub async fn handle_global_events(
  ctx: FloNodeEventContext,
  mut event_receiver: Receiver<GlobalEvent>,
) -> Result<()> {
  while let Some(event) = event_receiver.recv().await {
    match event {
      GlobalEvent::GameEnded(game_id) => {
        tracing::debug!(game_id, "game ended: {}", game_id);
        ctx.state.end_game(game_id);
      } // GlobalEvent::Shutdown => {}
    }
  }
  Ok(())
}
