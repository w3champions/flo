use crate::error::*;
use crate::game::state::GameActor;
use crate::game::{GameStatus, SlotClientStatus};
use crate::node::{messages as node_messages, PlayerLeaveResponse};
use crate::player::state::sender::PlayerFrames;
use crate::state::ActorMapExt;
use diesel::prelude::*;
use flo_net::packet::FloPacket;
use flo_net::proto;
use flo_state::{async_trait, Context, Handler, Message};
use s2_grpc_utils::S2ProtoEnum;
use std::collections::BTreeMap;

pub struct PlayerLeave {
  pub player_id: i32,
}

impl Message for PlayerLeave {
  type Result = Result<PlayerLeaveResult>;
}

#[derive(Default)]
pub struct PlayerLeaveResult {
  pub game_ended: bool,
}

#[async_trait]
impl Handler<PlayerLeave> for GameActor {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    PlayerLeave { player_id }: PlayerLeave,
  ) -> Result<PlayerLeaveResult> {
    let game_id = self.game_id;
    let result = match self.status {
      GameStatus::Preparing => leave_game_lobby(self, game_id, player_id).await?,
      GameStatus::Created | GameStatus::Running | GameStatus::Paused => {
        if let Some(node_id) = self.selected_node_id.clone() {
          leave_game_abort(self, game_id, player_id, node_id).await?
        } else {
          tracing::error!(game_id, "PlayerLeave: node not selected");
          PlayerLeaveResult::default()
        }
      }
      GameStatus::Ended => {
        tracing::error!(game_id, player_id, "player requested to leave a Ended game");
        PlayerLeaveResult::default()
      }
      GameStatus::Terminated => {
        tracing::error!(
          game_id,
          player_id,
          "player requested to leave a Terminated game"
        );
        PlayerLeaveResult::default()
      }
    };

    self
      .player_reg
      .player_leave_game(player_id, self.game_id)
      .await?;

    Ok(result)
  }
}

#[tracing::instrument(skip(state))]
async fn leave_game_lobby(
  state: &mut GameActor,
  game_id: i32,
  player_id: i32,
) -> Result<PlayerLeaveResult> {
  let leave = state
    .db
    .exec(move |conn| crate::game::db::remove_player(conn, game_id, player_id))
    .await?;

  let recipient_player_ids: Vec<i32> = leave
    .slots
    .iter()
    .filter_map(|s| s.player.as_ref().map(|p| p.id))
    .collect();

  broadcast(
    state,
    game_id,
    player_id,
    leave.game_ended,
    &leave.removed_players,
    &recipient_player_ids,
  )
  .await?;

  Ok(PlayerLeaveResult {
    game_ended: leave.game_ended,
  })
}

// Game has been created on node, force quit game
#[tracing::instrument(skip(state))]
async fn leave_game_abort(
  state: &mut GameActor,
  game_id: i32,
  player_id: i32,
  node_id: i32,
) -> Result<PlayerLeaveResult> {
  let active_player_ids = state
    .db
    .exec(move |conn| {
      conn.transaction(|| {
        crate::game::db::leave_node(conn, game_id, player_id)?;
        crate::game::db::get_node_active_player_ids(conn, game_id)
      })
    })
    .await?;

  let res = state
    .nodes
    .send_to(
      node_id,
      node_messages::NodePlayerLeave { game_id, player_id },
    )
    .await;

  match res {
    Ok(deferred) => {
      let res = deferred.await.or_cancelled();
      match res {
        Ok(PlayerLeaveResponse::Accepted(_)) => {}
        Ok(PlayerLeaveResponse::Rejected(reason)) => {
          tracing::error!(
            game_id,
            node_id,
            player_id,
            "force leave node rejected: {:?}",
            reason
          );
        }
        Err(err) => {
          tracing::error!(
            game_id,
            node_id,
            player_id,
            "force leave node error: {}",
            err
          );
        }
      }
    }
    Err(err) => {
      tracing::error!(
        game_id,
        node_id,
        player_id,
        "force leave node error: {:?}",
        err
      );
    }
  };

  let frame = {
    let mut pkt = flo_net::proto::flo_connect::PacketGameSlotClientStatusUpdate {
      game_id,
      player_id,
      ..Default::default()
    };
    pkt.set_status(SlotClientStatus::Left.into_proto_enum());
    pkt
  }
  .encode_as_frame()?;
  state
    .player_reg
    .broadcast(active_player_ids.clone(), frame)
    .await?;

  broadcast(
    state,
    game_id,
    player_id,
    false, // only change game status by node packet
    &[player_id],
    &active_player_ids,
  )
  .await?;

  Ok(PlayerLeaveResult { game_ended: false })
}

async fn broadcast(
  state: &mut GameActor,
  game_id: i32,
  player_id: i32,
  ended: bool,
  left_players: &[i32],
  recipient_players: &[i32],
) -> Result<()> {
  if ended {
    state
      .player_reg
      .players_leave_game(left_players.to_vec(), game_id)
      .await?;
  } else {
    let mut frame_map = BTreeMap::<i32, PlayerFrames>::new();

    state
      .player_reg
      .player_leave_game(player_id, game_id)
      .await?;

    let frame_player_leave = proto::flo_connect::PacketGamePlayerLeave {
      game_id,
      player_id,
      reason: proto::flo_connect::PlayerLeaveReason::Left.into(),
    }
    .encode_as_frame()?;

    for id in recipient_players {
      frame_map.insert(*id, frame_player_leave.clone().into());
    }

    state.player_reg.broadcast_map(frame_map).await?;
  }
  Ok(())
}
