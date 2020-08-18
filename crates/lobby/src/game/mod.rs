pub mod db;
mod slots;
pub mod token;
mod types;

use futures::{stream::FuturesUnordered, FutureExt, StreamExt};
use s2_grpc_utils::S2ProtoPack;

use flo_net::proto;

use crate::error::*;
use crate::game::db::{LeaveGameParams, UpdateGameSlotSettingsParams};
use crate::state::LobbyStateRef;
pub use slots::Slots;
pub use types::*;

pub async fn join_game(state: LobbyStateRef, game_id: i32, player_id: i32) -> Result<Game> {
  use crate::game::db::JoinGameParams;

  let params = JoinGameParams { game_id, player_id };

  let game = {
    let mut player_guard = state.mem.lock_player_state(player_id).await;
    if player_guard.joined_game_id().is_some() {
      return Err(Error::MultiJoin.into());
    }

    let mut game_guard = state
      .mem
      .lock_game_state(params.game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;
    let game = state
      .db
      .exec(move |conn| {
        let id = params.game_id;
        crate::game::db::join(conn, params)?;
        crate::game::db::get_full(conn, id)
      })
      .await
      .map_err(Error::from)?;

    player_guard.join_game(game.id);
    game_guard.add_player(player_id);
    let update = player_guard.get_session_update();
    if let Some(mut sender) = player_guard.get_sender_cloned() {
      let next_game = game.clone().into_packet();
      sender.send(update).await?;
      sender
        .send(proto::flo_connect::PacketGameInfo {
          game: next_game.into(),
        })
        .await?;
    }
    game
  };

  {
    let slot_info = game
      .get_player_slot_info(player_id)
      .ok_or_else(|| Error::PlayerSlotNotFound)?;
    let player: proto::flo_connect::PlayerInfo = slot_info.player.clone().pack()?;

    // send notification to other players in this game
    let mut players = game.get_player_ids();
    players.retain(|id| *id != player_id);
    state
      .mem
      .get_broadcaster(&players)
      .broadcast({
        use proto::flo_connect::*;
        PacketGamePlayerEnter {
          game_id: game.id,
          slot_index: slot_info.slot_index as i32,
          slot: Slot {
            player: Some(player),
            settings: Some(slot_info.slot.settings.clone().into_packet()),
          }
          .into(),
        }
      })
      .await
      .ok();
  }

  Ok(game)
}

pub async fn leave_game(state: LobbyStateRef, game_id: i32, player_id: i32) -> Result<()> {
  let mut player_guard = state.mem.lock_player_state(player_id).await;

  let player_state_game_id = if let Some(id) = player_guard.joined_game_id() {
    id
  } else {
    return Ok(());
  };

  if player_state_game_id != game_id {
    tracing::warn!("player joined game id mismatch: player_id = {}, player_state_game_id = {}, params.game_id = {}", 
        player_id,
        player_state_game_id,
        game_id
      );
  }

  let mut game_guard = state
    .mem
    .lock_game_state(player_state_game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  let leave = state
    .db
    .exec(move |conn| {
      crate::game::db::leave(
        conn,
        LeaveGameParams {
          player_id,
          game_id: player_state_game_id,
        },
      )
    })
    .await?;

  let broadcast_futures = FuturesUnordered::new();
  if leave.game_ended {
    for removed_player_id in leave.removed_players {
      game_guard.remove_player(player_id);
      if removed_player_id == player_id {
        player_guard.leave_game();
        // send to self
        let update = player_guard.get_session_update();
        if let Some(mut sender) = player_guard.get_sender_cloned() {
          broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
        }
      } else {
        let mut other_player_guard = state.mem.lock_player_state(removed_player_id).await;
        other_player_guard.leave_game();
        // kick self
        let update = other_player_guard.get_session_update();
        if let Some(mut sender) = other_player_guard.get_sender_cloned() {
          broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
        }
      }
    }
    game_guard.close();
  } else {
    player_guard.leave_game();
    game_guard.remove_player(player_id);

    // send to self
    let update = player_guard.get_session_update();
    if let Some(mut sender) = player_guard.get_sender_cloned() {
      broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
    }

    let player_ids: Vec<i32> = leave
      .slots
      .iter()
      .filter_map(|s| s.player.as_ref().map(|p| p.id))
      .collect();

    let broadcaster = state.mem.get_broadcaster(&player_ids);
    broadcast_futures.push(
      async move {
        broadcaster
          .broadcast({
            use proto::flo_connect::*;
            PacketGamePlayerLeave {
              game_id: player_state_game_id,
              player_id,
              reason: PlayerLeaveReason::Left.into(),
            }
          })
          .await
          .ok()
      }
      .boxed(),
    );
  }

  broadcast_futures.collect::<Vec<_>>().await;

  Ok(())
}

pub async fn update_game_slot_settings(
  state: LobbyStateRef,
  game_id: i32,
  player_id: i32,
  settings: SlotSettings,
) -> Result<Vec<Slot>> {
  let game_guard = state
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  if !game_guard.has_player(player_id) {
    return Err(Error::PlayerNotInGame.into());
  }

  let slots = state
    .db
    .exec(move |conn| {
      crate::game::db::update_slot_settings(
        conn,
        UpdateGameSlotSettingsParams {
          game_id,
          player_id,
          settings,
        },
      )
    })
    .await?;

  let index = slots
    .iter()
    .position(|s| {
      s.player
        .as_ref()
        .map(|p| p.id == player_id)
        .unwrap_or(false)
    })
    .ok_or_else(|| Error::PlayerSlotNotFound)?;

  let slot_index = index as i32;
  let settings: proto::flo_connect::SlotSettings = slots[index].settings.clone().pack()?;

  let players = game_guard.players().to_vec();
  drop(game_guard);

  state
    .mem
    .get_broadcaster(&players)
    .broadcast(proto::flo_connect::PacketGameSlotUpdate {
      game_id,
      slot_index,
      slot_settings: settings.into(),
    })
    .await
    .ok();

  Ok(slots)
}

pub async fn select_game_node(
  state: LobbyStateRef,
  game_id: i32,
  player_id: i32,
  node_id: Option<i32>,
) -> Result<()> {
  let mut game_guard = state
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  if game_guard.get_host_player() != Some(player_id) {
    return Err(Error::PlayerNotHost.into());
  }
  state
    .db
    .exec(move |conn| crate::game::db::select_node(conn, game_id, node_id))
    .await?;

  game_guard.select_node(node_id);

  let players = game_guard.players().to_vec();
  drop(game_guard);

  state
    .mem
    .get_broadcaster(&players)
    .broadcast(proto::flo_connect::PacketGameSelectNode { game_id, node_id })
    .await
    .ok();

  Ok(())
}

#[tracing::instrument(skip(state))]
pub async fn start_game(state: LobbyStateRef, game_id: i32, player_id: i32) -> Result<()> {
  let game_guard = state
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  if game_guard.get_host_player() != Some(player_id) {
    return Err(Error::PlayerNotHost.into());
  }

  let game = state
    .db
    .exec(move |conn| crate::game::db::get_full(conn, game_id))
    .await?;

  let node_id = if let Some(id) = game.node.as_ref().and_then(|node| node.get_node_id()) {
    id
  } else {
    return Err(Error::GameNodeNotSelected);
  };

  let node_conn = state.nodes.get_conn(node_id)?;

  let created = match node_conn.create_game(&game).await {
    Ok(created) => created,
    // failed, reply host player
    Err(err) => {
      if let Some(mut sender) = state.mem.get_player_sender(player_id) {
        let pkt = match err {
          Error::GameCreateTimeout => proto::flo_connect::PacketGameStartReject {
            game_id,
            message: format!("Create game timeout."),
            ..Default::default()
          },
          Error::GameCreateReject(reason) => {
            use proto::flo_node::ControllerCreateGameRejectReason;
            proto::flo_connect::PacketGameStartReject {
              game_id,
              message: match reason {
                ControllerCreateGameRejectReason::Unknown => {
                  format!("Create game request rejected.")
                }
                ControllerCreateGameRejectReason::GameExists => format!("Game already started."),
                ControllerCreateGameRejectReason::PlayerBusy => {
                  format!("Create game request rejected: Player busy.")
                }
                ControllerCreateGameRejectReason::Maintenance => {
                  format!("Create game request rejected: Server Maintenance.")
                }
              },
              ..Default::default()
            }
          }
          err => {
            tracing::error!("node create game: {}", err);
            proto::flo_connect::PacketGameStartReject {
              game_id,
              message: format!("Internal error."),
              ..Default::default()
            }
          }
        };
        sender.send(pkt).await.ok();
      }
      return Ok(());
    }
  };

  let players = game_guard.players().to_vec();
  drop(game_guard);

  // let mut senders = state.mem.get_player_senders(&players);
  // for sender in senders.values_mut() {
  //   sender.with_buf(|buf| buf.set_node_update(game_id, node_id))
  // }

  Ok(())
}
