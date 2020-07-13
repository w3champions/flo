use bs_diesel_utils::executor::ExecutorError;
use flo_grpc::game::*;
use flo_grpc::lobby::flo_lobby_server::*;
use flo_grpc::lobby::*;
use flo_grpc::player;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use tonic::{Request, Response, Status};

use crate::db::ExecutorRef;
use crate::error::{Error, Result};
use crate::state::StorageHandle;

pub struct FloLobbyService {
  db: ExecutorRef,
  state_storage: StorageHandle,
}

#[tonic::async_trait]
impl FloLobby for FloLobbyService {
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
    let games = self
      .db
      .exec(move |conn| crate::game::db::query(conn, &params))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;
    unimplemented!()
  }

  async fn create_game(
    &self,
    request: Request<CreateGameRequest>,
  ) -> Result<Response<CreateGameReply>, Status> {
    let params =
      crate::game::db::CreateGameParams::unpack(request.into_inner()).map_err(Status::internal)?;
    let player_id = params.player_id;
    let game = self
      .db
      .exec(move |conn| crate::game::db::create(conn, params))
      .await
      .map_err(|e| Status::internal(e.to_string()))?;
    self
      .state_storage
      .register_game(game.id, &[player_id])
      .await?;
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
    unimplemented!()
  }

  async fn update_game_slot_settings(
    &self,
    request: Request<UpdateGameSlotSettingsRequest>,
  ) -> Result<Response<UpdateGameSlotSettingsReply>, Status> {
    unimplemented!()
  }

  async fn cancel_game(&self, request: Request<CancelGameRequest>) -> Result<Response<()>, Status> {
    unimplemented!()
  }
}
