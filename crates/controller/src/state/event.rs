use bs_diesel_utils::ExecutorRef;
use s2_grpc_utils::{S2ProtoEnum, S2ProtoUnpack};
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing_futures::Instrument;

use flo_event::*;

use crate::game::start::{GameStartEvent, GameStartEventData};
use crate::game::{NodeGameStatus, SlotClientStatus};
use crate::node::NodeRegistryRef;
use crate::state::{ControllerStateRef, MemStorageRef};

pub type FloControllerEventSender = Sender<FloControllerEvent>;

#[derive(Debug)]
pub enum FloControllerEvent {
  PlayerStreamClosedEvent(PlayerStreamClosedEvent),
  GameStartEvent(GameStartEvent),
  GameSlotClientStatusUpdate(GameSlotClientStatusUpdate),
  GameStatusUpdate(Vec<GameStatusUpdate>),
}

#[derive(Debug)]
pub struct PlayerStreamClosedEvent {
  pub sid: u64,
  pub player_id: i32,
}

impl FloEvent for FloControllerEvent {
  const NAME: &'static str = "FloControllerEvent";
}

pub struct FloEventContext {
  pub db: ExecutorRef,
  pub mem: MemStorageRef,
  pub nodes: NodeRegistryRef,
}

pub fn spawn_event_handler(ctx: FloEventContext, mut receiver: Receiver<FloControllerEvent>) {
  tokio::spawn(
    async move {
      while let Some(event) = receiver.recv().await {
        match event {
          FloControllerEvent::PlayerStreamClosedEvent(PlayerStreamClosedEvent {
            sid,
            player_id,
          }) => {
            tracing::debug!(sid, player_id, "player stream closed");
            let mut guard = ctx.mem.lock_player_state(player_id).await;
            guard.remove_closed_sender(sid);
          }
          FloControllerEvent::GameStartEvent(GameStartEvent { game_id, data }) => match data {
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
          FloControllerEvent::GameSlotClientStatusUpdate(update) => {
            let game_id = update.game_id;
            let player_id = update.player_id;
            tracing::debug!(
              game_id,
              player_id,
              "update game slot client status: {:?}",
              update
            );
            if let Err(err) = crate::game::update_game_slot_client_status(&ctx, update).await {
              tracing::error!(
                game_id,
                player_id,
                "update game slot client status: {}",
                err
              );
            }
          }
          FloControllerEvent::GameStatusUpdate(updates) => {
            tracing::debug!("bulk update game status: {:?}", updates);
            if let Err(err) = crate::game::bulk_update_game_status(&ctx, updates).await {
              tracing::error!("bulk update game status: {}", err);
            }
          }
        }
      }
      tracing::debug!("exiting");
    }
    .instrument(tracing::debug_span!("event_handler_worker")),
  );
}

impl ControllerStateRef {
  async fn send_event(&self, event: FloControllerEvent) {
    self.event_sender.clone().send_or_log_as_error(event).await;
  }

  pub async fn emit_player_stream_closed(&self, sid: u64, player_id: i32) {
    self
      .send_event(FloControllerEvent::PlayerStreamClosedEvent(
        PlayerStreamClosedEvent { sid, player_id },
      ))
      .await;
  }
}

#[derive(Debug, S2ProtoUnpack)]
#[s2_grpc(message_type(flo_net::proto::flo_node::PacketClientUpdateSlotClientStatus))]
pub struct GameSlotClientStatusUpdate {
  pub player_id: i32,
  pub game_id: i32,
  #[s2_grpc(proto_enum)]
  pub status: SlotClientStatus,
}

#[derive(Debug)]
pub struct GameStatusUpdate {
  pub game_id: i32,
  pub status: NodeGameStatus,
  pub updated_player_game_client_status_map: HashMap<i32, SlotClientStatus>,
}

impl GameStatusUpdate {
  pub fn to_packet(&self) -> flo_net::proto::flo_node::PacketNodeGameStatusUpdate {
    let mut pkt = flo_net::proto::flo_node::PacketNodeGameStatusUpdate {
      game_id: self.game_id,
      ..Default::default()
    };
    pkt.set_status(self.status.into_proto_enum());
    for (id, status) in &self.updated_player_game_client_status_map {
      pkt.insert_updated_player_game_client_status_map(*id, status.into_proto_enum());
    }
    pkt
  }
}

impl From<flo_net::proto::flo_node::PacketNodeGameStatusUpdate> for GameStatusUpdate {
  fn from(pkt: flo_net::proto::flo_node::PacketNodeGameStatusUpdate) -> Self {
    GameStatusUpdate {
      game_id: pkt.game_id,
      status: NodeGameStatus::unpack_enum(pkt.status()),
      updated_player_game_client_status_map: pkt
        .updated_player_game_client_status_map
        .into_iter()
        .map(|(k, v)| {
          (
            k,
            flo_net::proto::flo_connect::SlotClientStatus::from_i32(v)
              .map(|v| SlotClientStatus::unpack_enum(v))
              .unwrap_or(SlotClientStatus::Pending),
          )
        })
        .collect(),
    }
  }
}
