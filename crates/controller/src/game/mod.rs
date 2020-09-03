pub mod db;
mod slots;
pub mod start;
pub mod token;
mod types;

use futures::{stream::FuturesUnordered, FutureExt, StreamExt};
use s2_grpc_utils::{S2ProtoEnum, S2ProtoPack};
use std::collections::HashMap;

use flo_net::packet::FloPacket;
use flo_net::proto;

use crate::error::*;
use crate::game::db::{CreateGameParams, LeaveGameParams};
use crate::node::conn::PlayerLeaveResponse;
use crate::node::PlayerToken;
use crate::state::event::{FloEventContext, GameSlotClientStatusUpdate, GameStatusUpdate};
use crate::state::{ControllerStateRef, LockedGameState, LockedPlayerState};
pub use slots::Slots;
pub use types::*;

pub async fn create_game(state: ControllerStateRef, params: CreateGameParams) -> Result<Game> {
  let player_id = params.player_id;
  let game = {
    let mut player_state = state.mem.lock_player_state(player_id).await;

    if player_state.joined_game_id().is_some() {
      return Err(Error::MultiJoin.into());
    }

    let game = state
      .db
      .exec(move |conn| crate::game::db::create(conn, params))
      .await?;
    state
      .mem
      .register_game(game.id, game.status, Some(player_id), &[player_id])
      .await;

    player_state.join_game(game.id);
    let update = player_state.get_session_update();
    if let Some(mut sender) = player_state.get_sender_cloned() {
      let next_game = game.clone().pack()?;
      sender.send(update).await.ok();
      sender
        .send({
          use flo_net::proto::flo_connect::*;
          PacketGameInfo {
            game: Some(next_game),
          }
        })
        .await
        .ok();
    }
    game
  };
  Ok(game)
}

pub async fn join_game(state: ControllerStateRef, game_id: i32, player_id: i32) -> Result<Game> {
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

    if game_guard.started() {
      return Err(Error::GameStarted);
    }

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
      let next_game = game.clone().pack()?;
      sender.send(update).await?;
      sender
        .send(proto::flo_connect::PacketGameInfo {
          game: Some(next_game),
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
            settings: Some(slot_info.slot.settings.clone().pack()?),
            ..Default::default()
          }
          .into(),
        }
      })
      .await
      .ok();
  }

  Ok(game)
}

pub async fn leave_game(state: ControllerStateRef, game_id: i32, player_id: i32) -> Result<()> {
  let player_guard = state.mem.lock_player_state(player_id).await;
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

  let game_guard = state
    .mem
    .lock_game_state(player_state_game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  match game_guard.status() {
    GameStatus::Preparing => leave_game_lobby(state, game_guard, player_guard).await?,
    GameStatus::Created | GameStatus::Running | GameStatus::Paused => {
      leave_game_abort(state, game_guard, player_guard).await?;
    }
    GameStatus::Ended => {
      tracing::error!(game_id, player_id, "player requested to leave a Ended game");
    }
    GameStatus::Terminated => {
      tracing::error!(
        game_id,
        player_id,
        "player requested to leave a Terminated game"
      );
    }
  }

  Ok(())
}

async fn leave_game_update_state_and_broadcast(
  state: ControllerStateRef,
  mut game_guard: LockedGameState,
  mut player_guard: LockedPlayerState,
  ended: bool,
  left_players: &[i32],
  recipient_players: &[i32],
) -> Result<()> {
  let game_id = game_guard.id();
  let player_id = player_guard.id();

  let broadcast_futures = FuturesUnordered::new();
  if ended {
    for removed_player_id in left_players.into_iter().cloned() {
      game_guard.remove_player(player_id);
      if removed_player_id == player_id {
        player_guard.leave_game(game_id);
        // send to self
        let update = player_guard.get_session_update();
        if let Some(mut sender) = player_guard.get_sender_cloned() {
          broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
        }
      } else {
        let mut other_player_guard = state.mem.lock_player_state(removed_player_id).await;
        other_player_guard.leave_game(game_id);
        // kick self
        let update = other_player_guard.get_session_update();
        if let Some(mut sender) = other_player_guard.get_sender_cloned() {
          broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
        }
      }
    }
    game_guard.close();
  } else {
    player_guard.leave_game(game_id);
    game_guard.remove_player(player_id);

    // send to self
    let update = player_guard.get_session_update();
    if let Some(mut sender) = player_guard.get_sender_cloned() {
      broadcast_futures.push(async move { sender.send(update).await.ok() }.boxed());
    }

    let broadcaster = state.mem.get_broadcaster(&recipient_players);
    broadcast_futures.push(
      async move {
        broadcaster
          .broadcast({
            use proto::flo_connect::*;
            PacketGamePlayerLeave {
              game_id,
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

  drop(game_guard);
  drop(player_guard);

  broadcast_futures.collect::<Vec<_>>().await;
  Ok(())
}

async fn leave_game_lobby(
  state: ControllerStateRef,
  game_guard: LockedGameState,
  player_guard: LockedPlayerState,
) -> Result<()> {
  let game_id = game_guard.id();
  let player_id = player_guard.id();

  let leave = state
    .db
    .exec(move |conn| crate::game::db::leave_lobby(conn, LeaveGameParams { player_id, game_id }))
    .await?;

  let recipient_player_ids: Vec<i32> = leave
    .slots
    .iter()
    .filter_map(|s| s.player.as_ref().map(|p| p.id))
    .collect();

  leave_game_update_state_and_broadcast(
    state,
    game_guard,
    player_guard,
    leave.game_ended,
    &leave.removed_players,
    &recipient_player_ids,
  )
  .await?;

  Ok(())
}

// Game has been created on node, force quit game
async fn leave_game_abort(
  state: ControllerStateRef,
  game_guard: LockedGameState,
  player_guard: LockedPlayerState,
) -> Result<()> {
  let game_id = game_guard.id();
  let player_id = player_guard.id();

  let node_id = if let Some(node_id) = game_guard.selected_node_id() {
    node_id
  } else {
    return Err(Error::GameNodeNotSelected);
  };

  // because node conn request could take 5s to timeout
  // drop game guard early to allow other player to send leave request
  drop(game_guard);

  // drop player guard to not block other player actions
  drop(player_guard);

  let conn = state.nodes.get_conn(node_id)?;

  match conn.player_leave(game_id, player_id).await {
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

  let game_guard = match state.mem.lock_game_state(game_id).await {
    Some(guard) => guard,
    None => {
      // game already ended
      return Ok(());
    }
  };

  let mut player_guard = state.mem.lock_player_state(player_id).await;

  let active_player_ids = state
    .db
    .exec(move |conn| {
      crate::game::db::leave_node(conn, game_id, player_id)?;
      crate::game::db::get_node_active_player_ids(conn, game_id)
    })
    .await?;

  game_guard
    .get_broadcaster()
    .broadcast({
      let mut pkt = flo_net::proto::flo_connect::PacketGameSlotClientStatusUpdate {
        game_id,
        player_id,
        ..Default::default()
      };
      pkt.set_status(SlotClientStatus::Left.into_proto_enum());
      pkt
    })
    .await
    .ok();
  player_guard.leave_game(game_id);

  leave_game_update_state_and_broadcast(
    state,
    game_guard,
    player_guard,
    false, // only change game status by node packet
    &[player_id],
    &active_player_ids,
  )
  .await?;

  Ok(())
}

pub async fn update_game_slot_settings(
  state: ControllerStateRef,
  game_id: i32,
  player_id: i32,
  slot_index: i32,
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
      let info = crate::game::db::get_slot_owner_info(conn, game_id, slot_index)?;
      if !info.is_slot_owner(player_id) {
        return Err(Error::GameSlotUpdateDenied);
      }
      crate::game::db::update_slot_settings(conn, game_id, slot_index, settings)
    })
    .await?;

  let index = slot_index as usize;
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
  state: ControllerStateRef,
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
pub async fn start_game(state: ControllerStateRef, game_id: i32, player_id: i32) -> Result<()> {
  let mut game_guard = state
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  if game_guard.started() {
    return Err(Error::GameStarted);
  }

  if game_guard.get_host_player() != Some(player_id) {
    return Err(Error::PlayerNotHost.into());
  }

  let game = state
    .db
    .exec(move |conn| crate::game::db::get(conn, game_id))
    .await?;

  if game.node.is_none() {
    return Err(Error::GameNodeNotSelected);
  };

  if game_guard.start() {
    game_guard
      .get_broadcaster()
      .broadcast(proto::flo_connect::PacketGameStarting { game_id })
      .await
      .ok();
  }

  Ok(())
}

pub async fn start_game_proceed(
  ctx: &FloEventContext,
  game_id: i32,
  map: HashMap<i32, proto::flo_connect::PacketGameStartPlayerClientInfoRequest>,
) -> Result<()> {
  let mut game_guard = ctx
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  let mut pass = true;

  {
    let mut version: Option<&str> = None;
    let mut sha1: Option<&[u8]> = None;
    for req in map.values() {
      if version.get_or_insert(&req.war3_version) != &req.war3_version {
        pass = false;
        break;
      }
      if sha1.get_or_insert(&req.map_sha1).as_ref() != &req.map_sha1 as &[u8] {
        pass = false;
        break;
      }
    }
  }

  if !pass {
    game_guard.start_game_reset();
    game_guard
      .get_broadcaster()
      .broadcast(proto::flo_connect::PacketGameStartReject {
        game_id,
        message: "Unable to start the game because the game and map version check failed."
          .to_string(),
        player_client_info_map: map,
      })
      .await
      .ok();
    return Ok(());
  }

  create_node_game(&ctx, game_guard).await?;
  Ok(())
}

pub async fn create_node_game(
  ctx: &FloEventContext,
  mut game_guard: LockedGameState,
) -> Result<()> {
  let game_id = game_guard.id();
  let game = ctx
    .db
    .exec(move |conn| crate::game::db::get_full(conn, game_id))
    .await?;

  let node_id = if let Some(id) = game.node.as_ref().map(|node| node.id) {
    id
  } else {
    return Err(Error::GameNodeNotSelected);
  };

  let node_conn = ctx.nodes.get_conn(node_id)?;

  let created = match node_conn.create_game(&game).await {
    Ok(created) => created,
    // failed, reply host player
    Err(err) => {
      if let Some(mut sender) = game_guard
        .get_host_player()
        .and_then(|player_id| ctx.mem.get_player_sender(player_id))
      {
        let pkt = match err {
          Error::NodeRequestTimeout => proto::flo_connect::PacketGameStartReject {
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
  game_guard.start_complete(&created);

  let token_map: HashMap<i32, PlayerToken> = created
    .player_tokens
    .into_iter()
    .map(|token| (token.player_id, token))
    .collect();

  game_guard
    .get_broadcaster()
    .broadcast_by::<_, proto::flo_connect::PacketGamePlayerToken>(|player_id| {
      if let Some(token) = token_map.get(&player_id) {
        Some(proto::flo_connect::PacketGamePlayerToken {
          node_id,
          game_id,
          player_id,
          player_token: token.to_vec(),
        })
      } else {
        tracing::error!(game_id, player_id, "player token was not found");
        None
      }
    })
    .await
    .ok();

  ctx
    .db
    .exec(move |conn| crate::game::db::update_created(conn, game_id, token_map))
    .await?;
  game_guard.set_status(GameStatus::Created);

  Ok(())
}

pub async fn start_game_set_timeout(ctx: &FloEventContext, game_id: i32) -> Result<()> {
  let mut game_guard = ctx
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;
  let state = game_guard.start_game_reset();
  game_guard
    .get_broadcaster()
    .broadcast(proto::flo_connect::PacketGameStartReject {
      game_id,
      message: "Some of the players didn't response in time.".to_string(),
      player_client_info_map: state.and_then(|state| state.get_map()).unwrap_or_default(),
    })
    .await
    .ok();
  Ok(())
}

pub async fn start_game_abort(ctx: &FloEventContext, game_id: i32) -> Result<()> {
  let mut game_guard = ctx
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;
  let state = game_guard.start_game_reset();

  ctx
    .db
    .exec(move |conn| crate::game::db::update_reset_created(conn, game_id))
    .await?;

  game_guard
    .get_broadcaster()
    .broadcast(proto::flo_connect::PacketGameStartReject {
      game_id,
      message: "Unable to start the game because of a internal error.".to_string(),
      player_client_info_map: state.and_then(|state| state.get_map()).unwrap_or_default(),
    })
    .await
    .ok();
  Ok(())
}

pub async fn update_game_slot_client_status(
  ctx: &FloEventContext,
  GameSlotClientStatusUpdate {
    player_id,
    game_id,
    status,
  }: GameSlotClientStatusUpdate,
) -> Result<()> {
  let game_guard = ctx
    .mem
    .lock_game_state(game_id)
    .await
    .ok_or_else(|| Error::GameNotFound)?;

  ctx
    .db
    .exec(move |conn| crate::game::db::update_slot_client_status(conn, game_id, player_id, status))
    .await?;

  game_guard
    .get_broadcaster()
    .broadcast({
      let mut pkt = proto::flo_connect::PacketGameSlotClientStatusUpdate {
        player_id,
        game_id,
        ..Default::default()
      };
      pkt.set_status(status.into_proto_enum());
      pkt
    })
    .await
    .ok();

  Ok(())
}

pub async fn bulk_update_game_status(
  ctx: &FloEventContext,
  updates: Vec<GameStatusUpdate>,
) -> Result<()> {
  let mut player_frames = HashMap::new();
  for item in &updates {
    let frame = item.to_packet().encode_as_frame()?;
    for player_id in item.updated_player_game_client_status_map.keys().cloned() {
      player_frames
        .entry(player_id)
        .or_insert_with(|| vec![])
        .push(frame.clone());
    }
  }

  ctx
    .db
    .exec(move |conn| -> Result<_> {
      crate::game::db::bulk_update_status(conn, &updates)?;
      Ok(updates)
    })
    .await?;

  // let f: FuturesUnordered<_> = updates.into_iter().map(|update| Ok(())).collect();
  //
  // if f.is_empty() {
  //   return Ok(());
  // }
  //
  // f.collect::<Vec<Result<_>>>().await;

  Ok(())
}
