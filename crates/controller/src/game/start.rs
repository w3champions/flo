use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::Notify;
use tokio::time::delay_for;
use tracing_futures::Instrument;

use flo_event::*;
use flo_net::proto::flo_connect::PacketGameStartPlayerClientInfoRequest;

use crate::error::*;
use crate::state::event::FloLobbyEvent;

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct GameStartEvent {
  pub game_id: i32,
  pub data: GameStartEventData,
}

impl From<GameStartEvent> for FloLobbyEvent {
  fn from(event: GameStartEvent) -> Self {
    FloLobbyEvent::GameStartEvent(event)
  }
}

impl FloEvent for GameStartEvent {
  const NAME: &'static str = "GameStartEvent";
}

pub type EventSender = EventFromSender<FloLobbyEvent, GameStartEvent>;

#[derive(Debug)]
pub enum GameStartEventData {
  Timeout,
  Done(HashMap<i32, PacketGameStartPlayerClientInfoRequest>),
}

#[derive(Debug)]
pub struct StartGameState {
  player_sender: Sender<(i32, PacketGameStartPlayerClientInfoRequest)>,
  state: Arc<State>,
}

impl Drop for StartGameState {
  fn drop(&mut self) {
    self.state.dropper.notify();
  }
}

impl StartGameState {
  pub fn new(event_sender: EventSender, game_id: i32, player_ids: Vec<i32>) -> Self {
    let dropper = Arc::new(Notify::new());
    let state = Arc::new(State {
      game_id,
      dropper,
      event_sender,
      player_ack_map: RwLock::new(Some(
        player_ids
          .into_iter()
          .map(|player_id| (player_id, ClientInfoAck::Pending))
          .collect(),
      )),
    });

    let (player_sender, mut player_receiver) = channel(1);

    tokio::spawn(
      {
        let state = state.clone();
        async move {
          let mut timeout = delay_for(TIMEOUT);
          loop {
            tokio::select! {
              _ = state.dropper.notified() => {
                tracing::debug!("dropped");
                break;
              }
              _ = &mut timeout => {
                state.event_sender.clone().send_or_discard(GameStartEvent {
                  game_id,
                  data: GameStartEventData::Timeout
                }).await;
                tracing::debug!("timeout");
                break;
              }
              ack = player_receiver.recv() => {
                if let Some((player_id, pkt)) = ack {
                  tracing::debug!(player_id, "ack");
                  match state.ack_player_and_check(player_id, pkt).await {
                    Ok(done) => {
                      if done {
                        tracing::debug!(player_id, "ack done");
                        break;
                      }
                    },
                    Err(err) => {
                      tracing::debug!(player_id, "ack failed: {}", err);
                      break;
                    }
                  }
                } else {
                  tracing::debug!("sender dropped");
                  break;
                }
              }
            }
          }
          tracing::debug!("exiting")
        }
      }
      .instrument(tracing::debug_span!("worker", game_id)),
    );

    StartGameState {
      player_sender,
      state,
    }
  }

  pub fn ack_player(&self, player_id: i32, pkt: PacketGameStartPlayerClientInfoRequest) {
    let mut sender = self.player_sender.clone();
    tokio::spawn(async move {
      sender.send((player_id, pkt)).await.ok();
    });
  }

  pub fn get_map(&self) -> Option<HashMap<i32, PacketGameStartPlayerClientInfoRequest>> {
    self.state.player_ack_map.read().as_ref().map(|map| {
      map
        .iter()
        .filter_map(|(player_id, ack)| {
          if let ClientInfoAck::Received(ref pkt) = ack {
            Some((*player_id, pkt.clone()))
          } else {
            None
          }
        })
        .collect()
    })
  }
}

#[derive(Debug)]
pub enum ClientInfoAck {
  Pending,
  Received(PacketGameStartPlayerClientInfoRequest),
}

#[derive(Debug)]
struct State {
  game_id: i32,
  dropper: Arc<Notify>,
  event_sender: EventSender,
  player_ack_map: RwLock<Option<HashMap<i32, ClientInfoAck>>>,
}

impl State {
  async fn ack_player_and_check(
    &self,
    player_id: i32,
    pkt: PacketGameStartPlayerClientInfoRequest,
  ) -> Result<bool> {
    let done_map = {
      let mut guard = self.player_ack_map.write();
      let done = if let Some(map) = guard.as_mut() {
        map.insert(player_id, ClientInfoAck::Received(pkt));

        if map.values().all(|ack| {
          if let ClientInfoAck::Received(_) = ack {
            true
          } else {
            false
          }
        }) {
          true
        } else {
          false
        }
      } else {
        // already done
        false
      };
      if done {
        if let Some(map) = guard.take() {
          map
            .into_iter()
            .filter_map(|(player_id, ack)| {
              if let ClientInfoAck::Received(pkt) = ack {
                Some((player_id, pkt))
              } else {
                None
              }
            })
            .collect::<HashMap<i32, PacketGameStartPlayerClientInfoRequest>>()
            .into()
        } else {
          None
        }
      } else {
        None
      }
    };

    let done = done_map.is_some();
    if let Some(map) = done_map {
      self
        .event_sender
        .clone()
        .send_or_discard(GameStartEvent {
          game_id: self.game_id,
          data: GameStartEventData::Done(map),
        })
        .await;
    }

    Ok(done)
  }
}
