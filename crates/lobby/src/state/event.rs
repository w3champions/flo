use bs_diesel_utils::ExecutorRef;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing_futures::Instrument;

use flo_event::*;

use crate::game::start::{GameStartEvent, GameStartEventData};
use crate::node::NodeRegistryRef;
use crate::state::{LobbyStateRef, MemStorageRef};

pub type FloLobbyEventSender = Sender<FloLobbyEvent>;

#[derive(Debug)]
pub enum FloLobbyEvent {
  PlayerStreamClosedEvent(PlayerStreamClosedEvent),
  GameStartEvent(GameStartEvent),
}

#[derive(Debug)]
pub struct PlayerStreamClosedEvent {
  pub sid: u64,
  pub player_id: i32,
}

impl FloEvent for FloLobbyEvent {
  const NAME: &'static str = "FloLobbyEvent";
}

pub struct FloEventContext {
  pub db: ExecutorRef,
  pub mem: MemStorageRef,
  pub nodes: NodeRegistryRef,
}

pub fn spawn_event_handler(ctx: FloEventContext, mut receiver: Receiver<FloLobbyEvent>) {
  tokio::spawn(
    async move {
      while let Some(event) = receiver.recv().await {
        match event {
          FloLobbyEvent::PlayerStreamClosedEvent(PlayerStreamClosedEvent { sid, player_id }) => {
            tracing::debug!(sid, player_id, "player stream closed");
            let mut guard = ctx.mem.lock_player_state(player_id).await;
            guard.remove_sender(sid);
          }
          FloLobbyEvent::GameStartEvent(GameStartEvent { game_id, data }) => match data {
            GameStartEventData::Timeout => {
              if let Err(err) = crate::game::start_game_set_timeout(&ctx, game_id).await {
                tracing::error!(game_id, "start game set timeout: {}", err);
              }
            }
            GameStartEventData::Done(map) => {
              if let Err(err) = crate::game::start_game_proceed(&ctx, game_id, map).await {
                tracing::error!(game_id, "start game proceed: {}", err);
                if let Err(err) = crate::game::start_game_abort(&ctx, game_id).await {
                  tracing::error!(game_id, "start game abort: {}", err);
                }
              }
            }
          },
        }
      }
      tracing::debug!("exiting");
    }
    .instrument(tracing::debug_span!("event_handler_worker")),
  );
}

impl LobbyStateRef {
  async fn send_event(&self, event: FloLobbyEvent) {
    self.event_sender.clone().send_or_log_as_error(event).await;
  }

  pub async fn emit_player_stream_closed(&self, sid: u64, player_id: i32) {
    self
      .send_event(FloLobbyEvent::PlayerStreamClosedEvent(
        PlayerStreamClosedEvent { sid, player_id },
      ))
      .await;
  }
}
