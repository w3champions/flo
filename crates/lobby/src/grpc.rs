use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::net::{Ipv4Addr, SocketAddrV4};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use flo_grpc::game::*;
use flo_grpc::lobby::flo_lobby_server::*;
use flo_grpc::lobby::*;
use flo_net::proto::flo_connect;

use crate::error::{Error, Result};
use crate::game::db::LeaveGameParams;
use crate::state::LobbyStateRef;
use flo_net::packet::OptionalFieldExt;

pub async fn serve(state: LobbyStateRef) -> Result<()> {
  let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, crate::constants::LOBBY_GRPC_PORT);
  let server_impl = FloLobbyService::new(state.clone());
  let server = FloLobbyServer::with_interceptor(server_impl, state.api_client.into_interceptor());
  let server = Server::builder().add_service(server);
  server.serve(addr.into()).await?;
  Ok(())
}

pub struct FloLobbyService {
  state: LobbyStateRef,
}

impl FloLobbyService {
  pub fn new(state: LobbyStateRef) -> Self {
    FloLobbyService { state }
  }
}

#[tonic::async_trait]
impl FloLobby for FloLobbyService {
  async fn get_player(
    &self,
    request: Request<GetPlayerRequest>,
  ) -> Result<Response<GetPlayerReply>, Status> {
    let player_id = request.into_inner().player_id;
    let player = self
      .state
      .db
      .exec(move |conn| crate::player::db::get(conn, player_id))
      .await
      .map_err(Error::from)?;
    Ok(Response::new(GetPlayerReply {
      player: player.pack().map_err(Status::internal)?,
    }))
  }

  async fn get_player_by_token(
    &self,
    request: Request<GetPlayerByTokenRequest>,
  ) -> Result<Response<GetPlayerReply>, Status> {
    let token = request.into_inner().token;
    let player_id = crate::player::token::validate_player_token(&token)?.player_id;
    let player = self
      .state
      .db
      .exec(move |conn| crate::player::db::get(conn, player_id))
      .await
      .map_err(Error::from)?;
    Ok(Response::new(GetPlayerReply {
      player: player.pack().map_err(Status::internal)?,
    }))
  }

  async fn update_and_get_player(
    &self,
    request: Request<UpdateAndGetPlayerRequest>,
  ) -> Result<Response<UpdateAndGetPlayerReply>, Status> {
    use crate::player::db;
    use std::convert::TryFrom;
    let upsert: db::UpsertPlayer = TryFrom::try_from(request.into_inner())?;
    let player = self
      .state
      .db
      .exec(move |conn| db::upsert(conn, &upsert))
      .await
      .map_err(Error::from)?;
    let token = crate::player::token::create_player_token(player.id)?;
    Ok(Response::new(UpdateAndGetPlayerReply {
      player: player.pack().map_err(Status::internal)?,
      token,
    }))
  }

  async fn list_nodes(&self, _request: Request<()>) -> Result<Response<ListNodesReply>, Status> {
    use std::iter::FromIterator;

    let nodes = self
      .state
      .db
      .exec(move |conn| crate::node::db::get_all_nodes(conn))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;
    let nodes: Vec<_> = Result::<_, Status>::from_iter(nodes.into_iter().map(|node| {
      Ok(Node {
        id: node.id,
        name: node.name,
        location: node.location,
        ip_addr: node.ip_addr,
        created_at: node.created_at.pack().map_err(Status::internal)?,
        updated_at: node.updated_at.pack().map_err(Status::internal)?,
      })
    }))?;
    Ok(Response::new(ListNodesReply { nodes }))
  }

  async fn list_games(
    &self,
    request: Request<ListGamesRequest>,
  ) -> Result<Response<ListGamesReply>, Status> {
    let params =
      crate::game::db::QueryGameParams::unpack(request.into_inner()).map_err(Status::internal)?;
    let mut r = self
      .state
      .db
      .exec(move |conn| crate::game::db::query(conn, &params))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;

    self.state.mem.fetch_num_players(&mut r.games);

    Ok(Response::new(r.pack().map_err(Error::from)?))
  }

  async fn create_game(
    &self,
    request: Request<CreateGameRequest>,
  ) -> Result<Response<CreateGameReply>, Status> {
    let params =
      crate::game::db::CreateGameParams::unpack(request.into_inner()).map_err(Status::internal)?;
    let player_id = params.player_id;

    let game = {
      let mut player_state = self.state.mem.lock_player_state(player_id).await;

      if player_state.joined_game_id().is_some() {
        return Err(Error::MultiJoin.into());
      }

      let game = self
        .state
        .db
        .exec(move |conn| crate::game::db::create(conn, params))
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
      self.state.mem.register_game(game.id, &[player_id]).await;

      player_state.join_game(game.id);
      let update = player_state.get_session_update();
      if let Some(mut sender) = player_state.get_sender_mut() {
        let next_game = game.clone().into_packet();
        sender.with_buf(|buf| {
          buf.update_session(update);
          buf.set_game(next_game);
        });
      }
      game
    };

    Ok(Response::new(CreateGameReply {
      game: game.pack().map_err(Status::internal)?,
    }))
  }

  async fn join_game(
    &self,
    request: Request<JoinGameRequest>,
  ) -> Result<Response<JoinGameReply>, Status> {
    let params =
      crate::game::db::JoinGameParams::unpack(request.into_inner()).map_err(Error::from)?;
    let player_id = params.player_id;

    let game = {
      let mut player_state = self.state.mem.lock_player_state(player_id).await;
      if player_state.joined_game_id().is_some() {
        return Err(Error::MultiJoin.into());
      }

      let mut state = self
        .state
        .mem
        .lock_game_state(params.game_id)
        .await
        .ok_or_else(|| Error::GameNotFound)?;
      let game = self
        .state
        .db
        .exec(move |conn| {
          let id = params.game_id;
          crate::game::db::join(conn, params)?;
          crate::game::db::get_full(conn, id)
        })
        .await
        .map_err(Error::from)?;

      player_state.join_game(game.id);
      state.add_player(player_id);
      let update = player_state.get_session_update();
      if let Some(sender) = player_state.get_sender_mut() {
        let next_game = game.clone().into_packet();
        sender.with_buf(move |buf| {
          buf.update_session(update);
          buf.set_game(next_game);
        });
      }
      game
    };

    // send notification to other players in this game
    for (slot_idx, slot) in game.slots.iter().enumerate() {
      if let Some(ref player) = slot.player {
        if player.id != player_id {
          let mut player_state = self.state.mem.lock_player_state(player.id).await;
          if let Some(sender) = player_state.get_sender_mut() {
            sender.with_buf(|buf| {
              buf.add_player_enter(
                game.id,
                player.clone().into_packet(),
                slot_idx as i32,
                slot.settings.clone().into_packet(),
              )
            });
          }
        }
      }
    }

    Ok(Response::new(JoinGameReply {
      game: game.pack().map_err(Error::from)?,
    }))
  }

  async fn leave_game(&self, request: Request<LeaveGameRequest>) -> Result<Response<()>, Status> {
    let params =
      crate::game::db::LeaveGameParams::unpack(request.into_inner()).map_err(Error::from)?;
    let player_id = params.player_id;

    let mut player_state = self.state.mem.lock_player_state(player_id).await;

    let player_state_game_id = if let Some(id) = player_state.joined_game_id() {
      id
    } else {
      return Ok(Response::new(()));
    };

    if player_state_game_id != params.game_id {
      tracing::warn!("player joined game id mismatch: player_id = {}, player_state_game_id = {}, params.game_id = {}", 
        player_id,
        player_state_game_id,
        params.game_id
      );
    }

    let mut state = self
      .state
      .mem
      .lock_game_state(player_state_game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;

    let slots = self
      .state
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
      .await
      .map_err(Error::from)?;

    player_state.leave_game();
    state.remove_player(player_id);

    // send to self
    let update = player_state.get_session_update();
    if let Some(sender) = player_state.get_sender_mut() {
      sender.with_buf(move |buf| {
        buf.update_session(update);
        buf.add_player_leave(
          player_state_game_id,
          player_id,
          flo_connect::PlayerLeaveReason::Left,
        )
      });
    }

    // send notification to other players in this game
    for slot in &slots {
      if let Some(ref player) = slot.player {
        if player.id != player_id {
          let mut player_state = self.state.mem.lock_player_state(player.id).await;
          if let Some(sender) = player_state.get_sender_mut() {
            sender.with_buf(|buf| {
              buf.add_player_leave(
                player_state_game_id,
                player.id,
                flo_connect::PlayerLeaveReason::Left,
              )
            });
          }
        }
      }
    }

    Ok(Response::new(()))
  }

  async fn update_game_slot_settings(
    &self,
    request: Request<UpdateGameSlotSettingsRequest>,
  ) -> Result<Response<UpdateGameSlotSettingsReply>, Status> {
    let params = crate::game::db::UpdateGameSlotSettingsParams::unpack(request.into_inner())
      .map_err(Error::from)?;

    let state = self
      .state
      .mem
      .lock_game_state(params.game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;

    if !state.has_player(params.player_id) {
      return Err(Error::PlayerNotInGame.into());
    }

    let slots = self
      .state
      .db
      .exec(move |conn| crate::game::db::update_slot_settings(conn, params))
      .await
      .map_err(Error::from)?;

    Ok(Response::new(UpdateGameSlotSettingsReply {
      slots: slots.pack().map_err(Error::from)?,
    }))
  }

  async fn cancel_game(&self, request: Request<CancelGameRequest>) -> Result<Response<()>, Status> {
    let req = request.into_inner();
    let game_id = req.game_id;
    let player_id = req.player_id;
    let mut state = self
      .state
      .mem
      .lock_game_state(game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;
    self
      .state
      .db
      .exec(move |conn| crate::game::db::delete(conn, game_id, Some(player_id)))
      .await
      .map_err(Error::from)?;
    for player in state.players() {
      let mut player_state = self.state.mem.lock_player_state(*player).await;
      player_state.leave_game();
      let update = player_state.get_session_update();
      player_state.get_sender_mut().map(|sender| {
        sender.with_buf(move |buf| {
          buf.update_session(update);
          buf.add_player_leave(
            game_id,
            *player,
            flo_connect::PlayerLeaveReason::GameCancelled,
          )
        })
      });
    }
    state.close();

    Ok(Response::new(()))
  }

  async fn import_map_checksums(
    &self,
    request: Request<ImportMapChecksumsRequest>,
  ) -> Result<Response<ImportMapChecksumsReply>, Status> {
    let items =
      Vec::<crate::map::db::ImportItem>::unpack(request.into_inner().items).map_err(Error::from)?;
    let updated = self
      .state
      .db
      .exec(move |conn| crate::map::db::import(conn, items))
      .await
      .map_err(Error::from)?;
    Ok(Response::new(ImportMapChecksumsReply {
      updated: updated as u32,
    }))
  }

  async fn search_map_checksum(
    &self,
    request: Request<SearchMapChecksumRequest>,
  ) -> Result<Response<SearchMapChecksumReply>, Status> {
    let sha1 = request.into_inner().sha1;
    let checksum = self
      .state
      .db
      .exec(move |conn| crate::map::db::search_checksum(conn, sha1))
      .await
      .map_err(Error::from)?;
    Ok(Response::new(SearchMapChecksumReply { checksum }))
  }
}
