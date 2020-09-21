use crate::error::Result;
use crate::game::db::CreateGameParams;
use crate::game::state::registry::Register;
use crate::game::state::GameRegistry;
use crate::game::{Game, GameStatus};
use crate::player::session::get_session_update_packet;
use flo_net::packet::FloPacket;
use flo_state::{async_trait, Context, Handler, Message};
use s2_grpc_utils::S2ProtoPack;

pub struct CreateGame {
  pub params: CreateGameParams,
}

impl Message for CreateGame {
  type Result = Result<Game>;
}

#[async_trait]
impl Handler<CreateGame> for GameRegistry {
  async fn handle(
    &mut self,
    _: &mut Context<Self>,
    CreateGame { params }: CreateGame,
  ) -> <CreateGame as Message>::Result {
    let player_id = params.player_id;
    let game = self
      .db
      .exec(move |conn| crate::game::db::create(conn, params))
      .await?;

    self.register(Register {
      id: game.id,
      status: GameStatus::Preparing,
      host_player: game.created_by.id,
      players: game.get_player_ids(),
    });

    let frames = {
      use flo_net::proto::flo_connect::*;
      vec![
        get_session_update_packet(Some(game.id)).encode_as_frame()?,
        PacketGameInfo {
          game: Some(game.clone().pack()?),
        }
        .encode_as_frame()?,
      ]
    };

    self.player_packet_sender.send(player_id, frames).await?;

    Ok(game)
  }
}
