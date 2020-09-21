use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use std::net::{Ipv4Addr, SocketAddrV4};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use flo_grpc::controller::flo_controller_server::*;
use flo_grpc::controller::*;

use crate::config::GetInterceptor;
use crate::error::{Error, Result};
use crate::game::db::CreateGameParams;
use crate::game::messages::{CreateGame, PlayerJoin, PlayerLeave};
use crate::game::state::cancel::CancelGame;
use crate::game::state::node::SelectNode;

use crate::game::state::registry::{AddGamePlayer, Remove, RemoveGamePlayer};
use crate::node::messages::ListNode;

use crate::state::{ActorMapExt, ControllerStateRef};

pub async fn serve(state: ControllerStateRef) -> Result<()> {
  let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, flo_constants::CONTROLLER_GRPC_PORT);
  let server_impl = FloControllerService::new(state.clone());

  let interceptor = state.config.send(GetInterceptor).await?;
  let server = FloControllerServer::with_interceptor(server_impl, interceptor);
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
    let nodes = self.state.nodes.send(ListNode).await.map_err(Error::from)?;
    Ok(Response::new(ListNodesReply {
      nodes: nodes.pack().map_err(Error::from)?,
    }))
  }

  async fn list_games(
    &self,
    _request: Request<ListGamesRequest>,
  ) -> Result<Response<ListGamesReply>, Status> {
    // let params =
    //   crate::game::db::QueryGameParams::unpack(request.into_inner()).map_err(Status::internal)?;
    // let mut r = self
    //   .state
    //   .db
    //   .exec(move |conn| crate::game::db::query(conn, &params))
    //   .await
    //   .map_err(|e| Status::internal(e.to_string()))?;
    //
    // self.state.state.fetch_num_players(&mut r.games);
    //
    // Ok(Response::new(r.pack().map_err(Error::from)?))
    return Err(Status::unimplemented("Unimplemented"));
  }

  async fn create_game(
    &self,
    request: Request<CreateGameRequest>,
  ) -> Result<Response<CreateGameReply>, Status> {
    let game = self
      .state
      .games
      .send(CreateGame {
        params: CreateGameParams::unpack(request.into_inner()).map_err(Error::from)?,
      })
      .await
      .map_err(Error::from)??;

    Ok(Response::new(CreateGameReply {
      game: game.pack().map_err(Status::internal)?,
    }))
  }

  async fn join_game(
    &self,
    request: Request<JoinGameRequest>,
  ) -> Result<Response<JoinGameReply>, Status> {
    let params = request.into_inner();

    let game = self
      .state
      .games
      .send_to(
        params.game_id,
        PlayerJoin {
          player_id: params.player_id,
        },
      )
      .await?;

    self
      .state
      .games
      .send(AddGamePlayer {
        game_id: params.game_id,
        player_id: params.player_id,
      })
      .await
      .map_err(Error::from)?;

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

    let game = self
      .state
      .games
      .send_to(
        join_token.game_id,
        PlayerJoin {
          player_id: params.player_id,
        },
      )
      .await?;

    self
      .state
      .games
      .send(AddGamePlayer {
        game_id: join_token.game_id,
        player_id: params.player_id,
      })
      .await
      .map_err(Error::from)?;

    Ok(Response::new(JoinGameReply {
      game: game.pack().map_err(Error::from)?,
    }))
  }

  async fn leave_game(&self, request: Request<LeaveGameRequest>) -> Result<Response<()>, Status> {
    let params = request.into_inner();

    let res = self
      .state
      .games
      .send_to(
        params.game_id,
        PlayerLeave {
          player_id: params.player_id,
        },
      )
      .await
      .map_err(Error::from)?;

    if res.game_ended {
      tracing::debug!(
        game_id = params.game_id,
        "shutting down: reason: PlayerLeave"
      );
      self
        .state
        .games
        .send(Remove {
          game_id: params.game_id,
        })
        .await
        .map_err(Error::from)?;
    } else {
      self
        .state
        .games
        .send(RemoveGamePlayer {
          game_id: params.game_id,
          player_id: params.player_id,
        })
        .await
        .map_err(Error::from)?;
    }

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

    self
      .state
      .games
      .send_to(game_id, SelectNode { player_id, node_id })
      .await?;

    Ok(Response::new(()))
  }

  async fn cancel_game(&self, request: Request<CancelGameRequest>) -> Result<Response<()>, Status> {
    let req = request.into_inner();
    let game_id = req.game_id;
    let player_id = req.player_id;

    self
      .state
      .games
      .send_to(game_id, CancelGame { player_id })
      .await?;

    tracing::debug!(game_id, "shutting down: reason: CancelGame");
    self
      .state
      .games
      .send(Remove { game_id })
      .await
      .map_err(Error::from)?;

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
