use bs_diesel_utils::executor::ExecutorError;
use flo_grpc::game::*;
use flo_grpc::lobby::flo_lobby_server::*;
use flo_grpc::lobby::*;
use flo_grpc::player;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use crate::api_client::ApiClientStorageRef;
use crate::db::ExecutorRef;
use crate::error::{Error, Result};
use crate::game::db::LeaveGameParams;
use crate::state::StorageHandle;

pub async fn serve(
  db: ExecutorRef,
  state_storage: StorageHandle,
  api_client: ApiClientStorageRef,
) -> Result<()> {
  let addr = "0.0.0.0:4095".parse().expect("parse grpc bind addr");
  let server_impl = FloLobbyService::new(db, state_storage);
  let server = FloLobbyServer::with_interceptor(server_impl, api_client.into_interceptor());
  let server = Server::builder().add_service(server);
  server.serve(addr).await?;
  Ok(())
}

pub struct FloLobbyService {
  db: ExecutorRef,
  state_storage: StorageHandle,
}

impl FloLobbyService {
  pub fn new(db: ExecutorRef, state_storage: StorageHandle) -> Self {
    FloLobbyService { db, state_storage }
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
      .db
      .exec(move |conn| crate::game::db::query(conn, &params))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;

    self.state_storage.fetch_num_players(&mut r.games);

    Ok(Response::new(r.pack().map_err(Error::from)?))
  }

  async fn create_game(
    &self,
    request: Request<CreateGameRequest>,
  ) -> Result<Response<CreateGameReply>, Status> {
    let params =
      crate::game::db::CreateGameParams::unpack(request.into_inner()).map_err(Status::internal)?;
    let player_id = params.player_id;
    let mut player_state = self.state_storage.lock_player_state(player_id).await;

    if player_state.joined_game_id().is_some() {
      return Err(Error::MultiJoin.into());
    }

    let game = self
      .db
      .exec(move |conn| crate::game::db::create(conn, params))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;
    self
      .state_storage
      .register_game(game.id, &[player_id])
      .await;

    player_state.join_game(game.id);

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
    let mut player_state = self.state_storage.lock_player_state(player_id).await;

    if player_state.joined_game_id().is_some() {
      return Err(Error::MultiJoin.into());
    }

    let mut state = self
      .state_storage
      .lock_game_state(params.game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;
    let game = self
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

    Ok(Response::new(JoinGameReply {
      game: game.pack().map_err(Error::from)?,
    }))
  }

  async fn leave_game(&self, request: Request<LeaveGameRequest>) -> Result<Response<()>, Status> {
    let params =
      crate::game::db::LeaveGameParams::unpack(request.into_inner()).map_err(Error::from)?;
    let player_id = params.player_id;
    let mut player_state = self.state_storage.lock_player_state(player_id).await;

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
      .state_storage
      .lock_game_state(player_state_game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;

    self
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

    Ok(Response::new(()))
  }

  async fn update_game_slot_settings(
    &self,
    request: Request<UpdateGameSlotSettingsRequest>,
  ) -> Result<Response<UpdateGameSlotSettingsReply>, Status> {
    let params = crate::game::db::UpdateGameSlotSettingsParams::unpack(request.into_inner())
      .map_err(Error::from)?;

    let mut state = self
      .state_storage
      .lock_game_state(params.game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;

    if !state.has_player(params.player_id) {
      return Err(Error::PlayerNotInGame.into());
    }

    let slots = self
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
      .state_storage
      .lock_game_state(game_id)
      .await
      .ok_or_else(|| Error::GameNotFound)?;
    self
      .db
      .exec(move |conn| crate::game::db::delete(conn, game_id, Some(player_id)))
      .await
      .map_err(Error::from)?;
    for player in state.players() {
      let mut player_state = self.state_storage.lock_player_state(player_id).await;
      player_state.leave_game();
    }
    state.close();

    Ok(Response::new(()))
  }
}
