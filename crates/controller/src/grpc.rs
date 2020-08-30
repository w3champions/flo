use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::net::{Ipv4Addr, SocketAddrV4};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use flo_grpc::controller::flo_controller_server::*;
use flo_grpc::controller::*;
use flo_net::proto::flo_connect;

use crate::error::{Error, Result};
use crate::state::ControllerStateRef;

pub async fn serve(state: ControllerStateRef) -> Result<()> {
  let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, flo_constants::CONTROLLER_GRPC_PORT);
  let server_impl = FloControllerService::new(state.clone());
  let server = FloControllerServer::with_interceptor(server_impl, state.config.into_interceptor());
  let server = Server::builder().add_service(server);
  server.serve(addr.into()).await?;
  Ok(())
}

pub struct FloControllerService {
  state: ControllerStateRef,
}

impl FloControllerService {
  pub fn new(state: ControllerStateRef) -> Self {
    FloControllerService { state }
  }
}

#[tonic::async_trait]
impl FloController for FloControllerService {
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
    let nodes = self.state.config.with_nodes(|nodes| nodes.to_vec());
    Ok(Response::new(ListNodesReply {
      nodes: nodes.pack().map_err(Error::from)?,
    }))
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

    let game = crate::game::create_game(self.state.clone(), params).await?;

    Ok(Response::new(CreateGameReply {
      game: game.pack().map_err(Status::internal)?,
    }))
  }

  async fn join_game(
    &self,
    request: Request<JoinGameRequest>,
  ) -> Result<Response<JoinGameReply>, Status> {
    let params = request.into_inner();

    let game = crate::game::join_game(self.state.clone(), params.game_id, params.player_id).await?;

    Ok(Response::new(JoinGameReply {
      game: game.pack().map_err(Error::from)?,
    }))
  }

  async fn create_join_game_token(
    &self,
    request: Request<CreateJoinGameTokenRequest>,
  ) -> Result<Response<CreateJoinGameTokenReply>, Status> {
    let params = request.into_inner();
    let game_id = params.game_id;

    let game = self
      .state
      .db
      .exec(move |conn| crate::game::db::get(conn, game_id))
      .await
      .map_err(Error::from)?;

    if game.created_by.as_ref().map(|p| p.id) != Some(params.player_id) {
      return Err(Error::PlayerNotHost.into());
    }

    let token = crate::game::token::create_join_token(params.game_id)?;

    Ok(Response::new(CreateJoinGameTokenReply { token }))
  }

  async fn join_game_by_token(
    &self,
    request: Request<JoinGameByTokenRequest>,
  ) -> Result<Response<JoinGameReply>, Status> {
    let params = request.into_inner();
    let join_token = crate::game::token::validate_join_token(&params.token)?;

    let game =
      crate::game::join_game(self.state.clone(), join_token.game_id, params.player_id).await?;

    Ok(Response::new(JoinGameReply {
      game: game.pack().map_err(Error::from)?,
    }))
  }

  async fn leave_game(&self, request: Request<LeaveGameRequest>) -> Result<Response<()>, Status> {
    let params = request.into_inner();

    crate::game::leave_game(self.state.clone(), params.game_id, params.player_id).await?;

    Ok(Response::new(()))
  }

  async fn select_game_node(
    &self,
    request: Request<SelectGameNodeRequest>,
  ) -> Result<Response<()>, Status> {
    let SelectGameNodeRequest {
      game_id,
      player_id,
      node_id,
    } = request.into_inner();

    crate::game::select_game_node(self.state.clone(), game_id, player_id, node_id).await?;

    Ok(Response::new(()))
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
      player_state.leave_game(game_id);
      let update = player_state.get_session_update();
      player_state.get_sender_cloned().map(|mut sender| {
        tokio::spawn(async move {
          sender.send(update).await.ok();
          sender
            .send({
              use flo_net::proto::flo_connect::*;
              PacketGamePlayerLeave {
                game_id,
                player_id: sender.player_id(),
                reason: flo_connect::PlayerLeaveReason::GameCancelled.into(),
              }
            })
            .await
            .ok();
        });
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