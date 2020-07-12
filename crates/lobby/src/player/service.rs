use bs_diesel_utils::executor::ExecutorError;
use flo_grpc::auth::flo_auth_server::*;
use flo_grpc::auth::*;
use s2_grpc_utils::{S2ProtoPack, S2ProtoUnpack};
use tonic::{Request, Response, Status};

use crate::db::ExecutorRef;
use crate::error::{Error, Result};
use crate::player::{db, token};

pub struct FloAuthService {
  db: ExecutorRef,
}

#[tonic::async_trait]
impl FloAuth for FloAuthService {
  async fn get_player(
    &self,
    request: Request<GetPlayerRequest>,
  ) -> Result<Response<GetPlayerReply>, Status> {
    let player_id = request.into_inner().player_id;
    let player = self
      .db
      .exec(move |conn| db::get(conn, player_id))
      .await
      .map_err(map_err)?;
    Ok(Response::new(GetPlayerReply {
      player: player.pack().map_err(Status::internal)?,
    }))
  }

  async fn update_and_get_player(
    &self,
    request: Request<UpdateAndGetPlayerRequest>,
  ) -> Result<Response<UpdateAndGetPlayerReply>, Status> {
    let upsert: db::UpsertPlayer =
      S2ProtoUnpack::unpack(request.into_inner()).map_err(Status::internal)?;
    let player = self
      .db
      .exec(move |conn| db::upsert(conn, &upsert))
      .await
      .map_err(map_err)?;
    let token = token::create_player_token(player.id).map_err(Status::internal)?;
    Ok(Response::new(UpdateAndGetPlayerReply {
      player: player.pack().map_err(Status::internal)?,
      token,
    }))
  }

  async fn validate_token(
    &self,
    request: Request<ValidateTokenRequest>,
  ) -> Result<Response<ValidateTokenReply>, Status> {
    let token = request.into_inner().token;
    let claims = token::validate_player_token(&token).map_err(|e| match e {
      e @ Error::PlayerTokenExpired => Status::unauthenticated(e),
      e => Status::internal(e),
    })?;
    let player_id = claims.player_id;
    let player = self
      .db
      .exec(move |conn| db::get(conn, player_id))
      .await
      .map_err(map_err)?;
    Ok(Response::new(ValidateTokenReply {
      player: player.pack().map_err(Status::internal)?,
    }))
  }
}

fn map_err(e: ExecutorError<Error>) -> Status {
  match e {
    ExecutorError::Task(e @ Error::PlayerNotFound) => Status::not_found(e),
    e => Status::internal(e.to_string()),
  }
}
